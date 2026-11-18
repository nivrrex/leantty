#![cfg(any(target_os = "linux", target_os = "windows"))]
//! GPU 对拍门禁（阶段 4B t3）：`CpuRenderer` 与 `GpuRenderer` 渲染同一快照，
//! 逐像素比较。无真实 Vulkan 适配器时**显式 SKIP 并打印原因**（不静默通过）。
//!
//! 断言（不得放宽）：
//! - 背景区（CPU 像素 == 默认背景色处）差异像素占比 ≤ 0.5%；
//! - 字形区最大通道差 ≤ 1 LSB；
//! - 字形区差异像素占比 ≤ 4A 实测值的 2 倍（4A 最大为 4.15%，故上限 8.3%）。

use render::gpu::GpuRenderer;
use render::{CpuRenderer, Error, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::FrameSnapshot;

/// 串行化 GPU 初始化：并行创建多个 device + 2048² 图集会让驱动报
/// "Not enough memory left"（实测），因此测试间互斥。
static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 4A 实测字形区差异占比的最大值（120x30@1x 的 1306/31448），上限取其 2 倍。
const GLYPH_DIFF_RATIO_LIMIT: f64 = 0.083;
const BG_DIFF_RATIO_LIMIT: f64 = 0.005;

fn settings(size: f32) -> FontSettings {
    let spec = |family: &str, style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size,
        normal: spec("monospace", "Regular"),
        bold: spec("monospace", "Bold"),
        italic: spec("monospace", "Italic"),
        bold_italic: spec("monospace", "Bold Italic"),
        fallback: Vec::new(),
    }
}

/// 系统里第一个含 '中' 的 CJK 字体（与 app 的后台 fallback 同源）。
fn cjk_fonts() -> Vec<render::Font> {
    use fontdb::{Database, Source};
    let mut db = Database::new();
    db.load_system_fonts();
    let mut faces: Vec<_> = db
        .faces()
        .filter(|f| {
            f.families
                .iter()
                .any(|(n, _)| n.to_lowercase().contains("cjk"))
        })
        .collect();
    faces.sort_by_key(|f| {
        let name = match &f.source {
            Source::File(p) => p.display().to_string(),
            Source::SharedFile(p, _) => p.display().to_string(),
            Source::Binary(_) => String::new(),
        };
        (name.contains("Serif") || !name.contains("Regular"), name)
    });
    for face in faces {
        let bytes: Vec<u8> = match &face.source {
            Source::File(p) => match std::fs::read(p) {
                Ok(b) => b,
                Err(_) => continue,
            },
            Source::Binary(d) => d.as_ref().as_ref().to_vec(),
            Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
        };
        if let Ok(f) = render::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            && f.has_glyph('中')
        {
            return vec![f];
        }
    }
    Vec::new()
}

fn make_cpu(scale: f64) -> Option<CpuRenderer> {
    let mut r = CpuRenderer::new(
        &settings(16.0),
        RenderOptions::default(),
        Palette::default(),
    )
    .ok()?;
    r.set_scale_factor(scale);
    let fonts = cjk_fonts();
    if fonts.is_empty() {
        eprintln!("SKIP: 系统中未找到含中文的 CJK 字体");
        return None;
    }
    r.add_background_fonts(fonts);
    Some(r)
}

fn make_gpu(scale: f64) -> Option<GpuRenderer> {
    let mut r = match GpuRenderer::new_offscreen(
        &settings(16.0),
        RenderOptions::default(),
        Palette::default(),
        render::gpu::allow_software_from_env(),
    ) {
        Ok(r) => r,
        Err(Error::Backend(msg)) => {
            eprintln!("SKIP: 无可用真实 GPU 适配器（{msg}）");
            return None;
        }
        Err(e) => panic!("GpuRenderer 创建失败（非适配器原因）: {e}"),
    };
    r.set_scale_factor(scale);
    let fonts = cjk_fonts();
    if fonts.is_empty() {
        eprintln!("SKIP: 系统中未找到含中文的 CJK 字体");
        return None;
    }
    r.add_background_fonts(fonts);
    Some(r)
}

const CONTENT: &str = concat!(
    "=== 4B gpu parity ===\r\n",
    "Hello, World! 0123456789 quick brown fox\r\n",
    "\x1b[1mBOLD\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4munderline\x1b[0m \x1b[7mreverse\x1b[0m \x1b[2mfaint\x1b[0m\r\n",
    "\x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m \x1b[34mblue\x1b[0m \x1b[33myellow\x1b[0m\r\n",
    "中文测试：回回回 终端渲染验证，全角符号。日本語 한국어.\r\n",
    "\x1b[44m  blue bg  \x1b[0m mixed 混排 abc 汉字 def\r\n",
    "\x1b[4:3mcurly\x1b[0m \x1b[9mstrike\x1b[0m \x1b[53moverline\x1b[0m @#$%^&*()\r\n",
    "last line\r\n",
);

fn frame(cols: u16, rows: u16, text: &str) -> FrameSnapshot {
    let mut term = vt::VtTerminal::new(cols, rows).expect("VtTerminal");
    term.feed(b"\x1b[2J\x1b[H");
    term.feed(text.as_bytes());
    term.feed(b"\x1b[5;12H");
    let mut s = term.snapshot().expect("snapshot");
    s.full_redraw = true;
    for d in s.dirty_rows.iter_mut() {
        *d = true;
    }
    s
}

struct Diff {
    bg_diff: u64,
    bg_total: u64,
    glyph_diff: u64,
    glyph_total: u64,
    glyph_max: u32,
    all_max: u32,
}

fn compare(cpu: &[u32], gpu_rgba: &[u8], w: u32, h: u32) -> Diff {
    let bg = Palette::default().primary_bg;
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
    let n = (w as usize) * (h as usize);
    let mut d = Diff {
        bg_diff: 0,
        bg_total: 0,
        glyph_diff: 0,
        glyph_total: 0,
        glyph_max: 0,
        all_max: 0,
    };
    for i in 0..n.min(cpu.len()).min(gpu_rgba.len() / 4) {
        let c = cpu[i];
        let (cr, cg, cb) = (
            ((c >> 16) & 0xFF) as i32,
            ((c >> 8) & 0xFF) as i32,
            (c & 0xFF) as i32,
        );
        let (gr, gg, gb) = (
            gpu_rgba[i * 4] as i32,
            gpu_rgba[i * 4 + 1] as i32,
            gpu_rgba[i * 4 + 2] as i32,
        );
        let maxd = (cr - gr)
            .unsigned_abs()
            .max((cg - gg).unsigned_abs())
            .max((cb - gb).unsigned_abs());
        d.all_max = d.all_max.max(maxd);
        let is_bg = (c & 0x00FF_FFFF) == bg_u32;
        if is_bg {
            d.bg_total += 1;
            if maxd > 0 {
                d.bg_diff += 1;
            }
        } else {
            d.glyph_total += 1;
            if maxd > 0 {
                d.glyph_diff += 1;
                d.glyph_max = d.glyph_max.max(maxd);
            }
        }
    }
    d
}

/// 跑一次完整的 CPU/GPU 对拍并断言。
fn parity(cols: u16, rows: u16, scale: f64, text: &str) {
    let _guard = serial();
    let (Some(mut cpu), Some(mut gpu)) = (make_cpu(scale), make_gpu(scale)) else {
        return;
    };
    let mut snap = frame(cols, rows, text);
    // 选区覆盖第 4–6 行、第 0–20 列（含中文行，触发反色/选区路径）。
    let sel = (4u16, 0u16, 6u16, 20u16);
    cpu.set_selection(Some(sel));
    gpu.set_selection(Some(sel));
    // 滚动条几何（覆盖 t5 的轨道/拇指绘制路径）。
    let sb = Some((30u32, 120u32));
    cpu.set_scrollbar(sb);
    gpu.set_scrollbar(sb);
    snap.full_redraw = true;

    let (cw, ch) = cpu.cell_metrics(scale);
    let (pw, ph) = (u32::from(cols) * cw, u32::from(rows) * ch);
    assert_eq!(gpu.cell_metrics(scale), (cw, ch), "两侧格子尺寸必须一致");

    cpu.render(&snap, pw, ph).expect("cpu render");
    gpu.render(&snap, pw, ph).expect("gpu render");
    let d = compare(cpu.framebuffer(), gpu.framebuffer_rgba(), pw, ph);
    let glyph_ratio = if d.glyph_total == 0 {
        0.0
    } else {
        d.glyph_diff as f64 / d.glyph_total as f64
    };
    let bg_ratio = if d.bg_total == 0 {
        0.0
    } else {
        d.bg_diff as f64 / d.bg_total as f64
    };
    eprintln!(
        "  {cols}x{rows}@{scale}x {pw}x{ph}: bg_diff={}/{} ({:.3}%) glyph_diff={}/{} ({:.2}%) glyph_max={} all_max={}",
        d.bg_diff,
        d.bg_total,
        bg_ratio * 100.0,
        d.glyph_diff,
        d.glyph_total,
        glyph_ratio * 100.0,
        d.glyph_max,
        d.all_max
    );
    assert!(d.glyph_max <= 1, "字形区最大通道差 {} > 1 LSB", d.glyph_max);
    assert!(
        bg_ratio <= BG_DIFF_RATIO_LIMIT,
        "背景区差异占比 {:.3}% 超过 {:.1}%",
        bg_ratio * 100.0,
        BG_DIFF_RATIO_LIMIT * 100.0
    );
    assert!(
        glyph_ratio <= GLYPH_DIFF_RATIO_LIMIT,
        "字形区差异占比 {:.2}% 超过 {:.1}%",
        glyph_ratio * 100.0,
        GLYPH_DIFF_RATIO_LIMIT * 100.0
    );
}

#[test]
fn gpu_parity_120x30_1x() {
    parity(120, 30, 1.0, CONTENT);
}

#[test]
fn gpu_parity_120x30_2x() {
    parity(120, 30, 2.0, CONTENT);
}

#[test]
fn gpu_parity_200x60_1x() {
    parity(200, 60, 1.0, CONTENT);
}

#[test]
fn gpu_parity_200x60_2x() {
    parity(200, 60, 2.0, CONTENT);
}

/// 四种光标样式逐一对比（块/空心块/下划线/竖线）。
#[test]
fn gpu_parity_cursor_styles() {
    let _guard = serial();
    let (Some(mut cpu), Some(mut gpu)) = (make_cpu(1.0), make_gpu(1.0)) else {
        return;
    };
    let (cw, ch) = cpu.cell_metrics(1.0);
    let (pw, ph) = (40 * cw, 6 * ch);
    for style in [
        vt::CursorStyle::Block,
        vt::CursorStyle::HollowBlock,
        vt::CursorStyle::Underline,
        vt::CursorStyle::Bar,
    ] {
        let mut snap = frame(40, 6, "cursor 光标\r\nsecond");
        snap.cursor.style = style;
        snap.cursor.visible = true;
        snap.cursor.x = 2;
        snap.cursor.y = 0;
        cpu.render(&snap, pw, ph).expect("cpu");
        gpu.render(&snap, pw, ph).expect("gpu");
        let d = compare(cpu.framebuffer(), gpu.framebuffer_rgba(), pw, ph);
        eprintln!(
            "  cursor {style:?}: glyph_max={} all_max={}",
            d.glyph_max, d.all_max
        );
        assert!(d.all_max <= 1, "光标 {style:?} 差异 {} > 1 LSB", d.all_max);
    }
}

/// 图集压力：一次性绘制数千个不同 CJK 字符，强制图集增长（甚至清空重建），
/// 重建后画面必须与 CPU 参照一致。
///
/// 用 BMP 区连续码点（随机挑选以覆盖不同部首），每帧独立渲染。
#[test]
fn gpu_atlas_rebuild_with_thousands_of_cjk() {
    let _guard = serial();
    let (Some(mut cpu), Some(mut gpu)) = (make_cpu(1.0), make_gpu(1.0)) else {
        return;
    };
    let (cw, ch) = cpu.cell_metrics(1.0);
    // 200x60 = 12000 格；用 3000 个不同 CJK 字符（不重复）铺满前 50 行。
    let mut chars: Vec<char> = Vec::new();
    let mut cp = 0x4E00u32;
    while chars.len() < 3000 && cp < 0x9FA5 {
        if let Some(c) = char::from_u32(cp) {
            chars.push(c);
            cp += 3; // 跳跃取样，覆盖更多字形
        } else {
            cp += 1;
        }
    }
    let mut text = String::new();
    for (i, c) in chars.iter().enumerate() {
        text.push(*c);
        if (i + 1) % 50 == 0 {
            text.push_str("\r\n");
        }
    }
    let mut snap = frame(200, 60, &text);
    snap.full_redraw = true;
    let (pw, ph) = (200 * cw, 60 * ch);
    cpu.render(&snap, pw, ph).expect("cpu");
    gpu.render(&snap, pw, ph).expect("gpu");
    let stats = gpu.atlas_stats();
    eprintln!(
        "  atlas size={} glyphs={} grows={} resets={}",
        stats.size, stats.glyphs, stats.grows, stats.resets
    );
    assert!(
        stats.glyphs >= 2000,
        "图集应包含至少 2000 个字形，实际 {}",
        stats.glyphs
    );
    let d = compare(cpu.framebuffer(), gpu.framebuffer_rgba(), pw, ph);
    let ratio = d.glyph_diff as f64 / d.glyph_total.max(1) as f64;
    eprintln!(
        "  CJK 压力: bg_diff={} glyph_diff={}/{} ({:.2}%) glyph_max={}",
        d.bg_diff,
        d.glyph_diff,
        d.glyph_total,
        ratio * 100.0,
        d.glyph_max
    );
    assert!(
        d.glyph_max <= 1,
        "压力帧字形区最大通道差 {} > 1",
        d.glyph_max
    );
    assert!(
        ratio <= GLYPH_DIFF_RATIO_LIMIT,
        "压力帧字形差异占比 {:.2}% 超过上限",
        ratio * 100.0
    );
}

/// 最小复现：单个反色字符，排除行/选区/宽字符干扰。
#[test]
fn gpu_parity_minimal_inverse_char() {
    let _guard = serial();
    let (Some(mut cpu), Some(mut gpu)) = (make_cpu(1.0), make_gpu(1.0)) else {
        return;
    };
    let (cw, ch) = cpu.cell_metrics(1.0);
    let mut term = vt::VtTerminal::new(4, 1).expect("term");
    term.feed(b"\x1b[2J\x1b[H");
    term.feed(b"\x1b[7mr\x1b[0m");
    let mut snap = term.snapshot().expect("snap");
    snap.full_redraw = true;
    let (pw, ph) = (4 * cw, ch);
    eprintln!(
        "  cells: {:?}",
        snap.cells[0]
            .iter()
            .map(|c| (c.chars.clone(), c.inverse))
            .collect::<Vec<_>>()
    );
    cpu.render(&snap, pw, ph).expect("cpu");
    gpu.render(&snap, pw, ph).expect("gpu");
    let d = compare(cpu.framebuffer(), gpu.framebuffer_rgba(), pw, ph);
    eprintln!(
        "  minimal inverse: bg_diff={} glyph_diff={} glyph_max={}",
        d.bg_diff, d.glyph_diff, d.glyph_max
    );
    assert!(d.all_max <= 1, "最小复现差异 {}", d.all_max);
}

/// Preedit 叠加层（含组合光标反色与下划线）与 CPU 视觉等价。
#[test]
fn gpu_parity_preedit_overlay() {
    let _guard = serial();
    let (Some(mut cpu), Some(mut gpu)) = (make_cpu(1.0), make_gpu(1.0)) else {
        return;
    };
    let (cw, ch) = cpu.cell_metrics(1.0);
    let (pw, ph) = (40 * cw, 6 * ch);
    let snap = frame(40, 6, "preedit 预编辑\r\nsecond");
    for (text, range) in [
        ("nihao", None),
        ("你好世界", None),
        ("ab日本語", Some((0usize, 2usize))),
    ] {
        let overlay = render::PreeditOverlay::new(text.to_string(), range);
        cpu.set_preedit(Some(overlay.clone()));
        gpu.set_preedit(Some(overlay));
        cpu.render(&snap, pw, ph).expect("cpu");
        gpu.render(&snap, pw, ph).expect("gpu");
        let d = compare(cpu.framebuffer(), gpu.framebuffer_rgba(), pw, ph);
        if d.all_max > 1 {
            // 差异 bbox（定位是字形位置还是别处）
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
            for y in 0..ph {
                for x in 0..pw {
                    let i = (y * pw + x) as usize;
                    let c = cpu.framebuffer()[i];
                    let g = &gpu.framebuffer_rgba()[i * 4..i * 4 + 3];
                    let cr = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
                    let gc = (u32::from(g[0]), u32::from(g[1]), u32::from(g[2]));
                    if cr.0.abs_diff(gc.0) > 1 || cr.1.abs_diff(gc.1) > 1 || cr.2.abs_diff(gc.2) > 1
                    {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
            }
            eprintln!("  preedit {text:?} 差异 bbox=({x0},{y0})-({x1},{y1}) cell={cw}x{ch}");
            let mut shown = 0;
            for y in 0..ph {
                for x in 0..pw {
                    let i = (y * pw + x) as usize;
                    let c = cpu.framebuffer()[i];
                    let g = &gpu.framebuffer_rgba()[i * 4..i * 4 + 3];
                    let cr = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
                    let gc = (u32::from(g[0]), u32::from(g[1]), u32::from(g[2]));
                    if (cr.0.abs_diff(gc.0) > 1
                        || cr.1.abs_diff(gc.1) > 1
                        || cr.2.abs_diff(gc.2) > 1)
                        && shown < 8
                    {
                        eprintln!(
                            "    ({x},{y}) cpu=#{:02x}{:02x}{:02x} gpu=#{:02x}{:02x}{:02x}",
                            cr.0, cr.1, cr.2, gc.0, gc.1, gc.2
                        );
                        shown += 1;
                    }
                }
            }
        }
        eprintln!(
            "  preedit {text:?}: bg_diff={} glyph_diff={} all_max={}",
            d.bg_diff, d.glyph_diff, d.all_max
        );
        assert!(d.all_max <= 1, "preedit {text:?} 差异 {}", d.all_max);
        cpu.set_preedit(None);
        gpu.set_preedit(None);
    }
}

/// 滚动条列对拍（阶段 4S t3）：把滚动条几何设成两种真实形态——
/// (1) 回溯行数多、视口在底部（拇指贴底）；(2) 视口上滚到中段（拇指居中）。
/// 只比较**滚动条列**（右缘 `SCROLLBAR_WIDTH` 像素）内 CPU 与 GPU 的像素，
/// 阈值沿用既有背景区口径（最大通道差 ≤1、差异占比 ≤0.5%）。
fn parity_scrollbar(rows_sb: u64, at_bottom: bool) {
    use render::SCROLLBAR_WIDTH;
    let _guard = serial();
    let (cols, rows) = (80u16, 24u16);
    let scale = 1.0;
    let (Some(mut cpu), Some(mut gpu)) = (make_cpu(scale), make_gpu(scale)) else {
        return;
    };
    let mut snap = frame(cols, rows, "scrollback overview\nline two\n中文行\n");
    snap.full_redraw = true;
    // 与 app 相同的几何公式：拇指高度按"可见行 / 总行"比例，底部时贴底。
    let (cw, ch) = cpu.cell_metrics(scale);
    let ph = u32::from(rows) * ch;
    let _ = cw;
    let total = rows_sb + u64::from(rows);
    let thumb_h = ((u64::from(ph) * u64::from(rows)) / total).max(24) as u32;
    let thumb_h = thumb_h.min(ph);
    let thumb_y = if at_bottom {
        ph - thumb_h
    } else {
        (ph - thumb_h) / 2
    };
    cpu.set_scrollbar(Some((thumb_y, thumb_h)));
    gpu.set_scrollbar(Some((thumb_y, thumb_h)));
    let pw = u32::from(cols) * cpu.cell_metrics(scale).0;
    cpu.render(&snap, pw, ph).expect("cpu render");
    gpu.render(&snap, pw, ph).expect("gpu render");
    let fb = cpu.framebuffer();
    let rgba = gpu.framebuffer_rgba();
    let x0 = pw.saturating_sub(SCROLLBAR_WIDTH);
    let mut diff = 0u64;
    let mut total_px = 0u64;
    let mut max_d = 0u32;
    let mut outside_diff = 0u64;
    let mut outside_total = 0u64;
    for y in 0..ph {
        for x in 0..pw {
            let i = (y * pw + x) as usize;
            let c = fb[i];
            let (cr, cg, cb) = (
                ((c >> 16) & 0xFF) as i32,
                ((c >> 8) & 0xFF) as i32,
                (c & 0xFF) as i32,
            );
            let (gr, gg, gb) = (
                rgba[i * 4] as i32,
                rgba[i * 4 + 1] as i32,
                rgba[i * 4 + 2] as i32,
            );
            let d = (cr - gr)
                .unsigned_abs()
                .max((cg - gg).unsigned_abs())
                .max((cb - gb).unsigned_abs());
            if x >= x0 {
                total_px += 1;
                if d > 0 {
                    diff += 1;
                    max_d = max_d.max(d);
                }
            } else {
                outside_total += 1;
                if d > 0 {
                    outside_diff += 1;
                }
            }
        }
    }
    let ratio = diff as f64 / total_px.max(1) as f64;
    let outside_ratio = outside_diff as f64 / outside_total.max(1) as f64;
    let inside_share_of_all = diff as f64 / (total_px + outside_total).max(1) as f64;
    eprintln!(
        "滚动条列 rows_sb={rows_sb} at_bottom={at_bottom}: 列内差异 {diff}/{total_px}（{:.3}%）max={max_d}；\
         列外差异占比 {:.3}%；列内像素占全图 {:.3}%、列内差异占全图 {:.3}%",
        ratio * 100.0,
        outside_ratio * 100.0,
        total_px as f64 / (total_px + outside_total) as f64 * 100.0,
        inside_share_of_all * 100.0
    );
    assert!(max_d <= 1, "滚动条列最大通道差应 ≤1 LSB（实测 {max_d}）");
    assert!(
        ratio <= BG_DIFF_RATIO_LIMIT,
        "滚动条列差异占比应 ≤{:.1}%（实测 {:.3}%）",
        BG_DIFF_RATIO_LIMIT * 100.0,
        ratio * 100.0
    );
}

#[test]
fn gpu_parity_scrollbar_at_bottom_80x24() {
    parity_scrollbar(2000, true);
}

#[test]
fn gpu_parity_scrollbar_mid_scroll_80x24() {
    parity_scrollbar(2000, false);
}
