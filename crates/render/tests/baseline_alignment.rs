//! 绝对基线（垂直对齐）集成测试（阶段 4B-fix1 任务 A）。
//!
//! 与"CPU/GPU 相对对拍"不同，本测试不比较两个渲染器，而是检查**字形相对基线
//! 的位置是否符合字体度量**：句点/逗号应坐在基线上、g/p/y 应下行、H 顶边不得低于 x。
//!
//! 字体矩阵：本机多种等宽字体 + `TEST_FONT_FILE`（用户提供的 Consolas，不入库）。
//! 打印 fontdue 的 ascent/descent/line_gap、cell_h、基线 y，以及每字符的
//! ymin/height 与实际墨迹落点，便于定位是"公式错"还是"该字体度量特殊"。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Underline};

const SAMPLE: &str = "xH.,-_'|gpyQ[]{}";

fn settings(family: &str, size: f32) -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
    }
}

/// 单行快照：每个字符占一格。
fn one_row(text: &str) -> FrameSnapshot {
    let cells: Vec<Cell> = text
        .chars()
        .map(|ch| Cell {
            chars: ch.to_string(),
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            faint: false,
            blink: false,
            inverse: false,
            invisible: false,
            strikethrough: false,
            overline: false,
            underline: Underline::None,
            wide: CellWide::Narrow,
        })
        .collect();
    FrameSnapshot {
        cols: cells.len() as u16,
        rows: 1,
        cells: vec![cells],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
        wrapped: Vec::new(),
        dirty_rows: vec![true],
        full_redraw: true,
    }
}

/// 某格内墨迹的 (min_y, max_y)，无墨迹返回 None。
fn ink_rows(fb: &[u32], pw: u32, cell: (u32, u32), idx: u32, ch_h: u32) -> Option<(u32, u32)> {
    let (cw, _) = cell;
    let x0 = idx * cw;
    let mut min_y = u32::MAX;
    let mut max_y = 0u32;
    let bg = Palette::default().primary_bg;
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
    for y in 0..ch_h {
        for x in x0..x0 + cw {
            let i = (y * pw + x) as usize;
            if i < fb.len() && (fb[i] & 0x00FF_FFFF) != bg_u32 {
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if min_y == u32::MAX {
        None
    } else {
        Some((min_y, max_y))
    }
}

/// 对一种字体跑绝对基线断言，返回是否通过（并打印度量与落点）。
fn check_font(family: &str, size: f32) -> bool {
    let mut r = match CpuRenderer::new(
        &settings(family, size),
        RenderOptions::default(),
        Palette::default(),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("SKIP: 字体 '{family}' 不可用（{e}）");
            return true;
        }
    };
    let (cw, ch) = r.cell_metrics(1.0);
    let snap = one_row(SAMPLE);
    let pw = cw * SAMPLE.chars().count() as u32;
    r.render(&snap, pw, ch).expect("render");
    let (fw, fh) = r.framebuffer_size();
    assert_eq!((fw, fh), (pw, ch));
    let fb = r.framebuffer();

    // fontdue 度量
    let fonts = r.cell_metrics(1.0);
    let _ = fonts;
    let lm = {
        // 用 CpuRenderer 内部同一字体的度量：通过 cell_metrics 反推不可行，
        // 这里独立加载一次同 family 拿 ascent/descent/line_gap 供打印。
        use fontdb::{Database, Family, Query};
        let mut db = Database::new();
        db.load_system_fonts();
        let q = Query {
            families: &[Family::Name(family), Family::Monospace],
            ..Default::default()
        };
        db.query(&q).and_then(|id| db.face(id).cloned())
    };
    let lm_text = match &lm {
        Some(face) => format!(
            "post_script='{}' families={:?}",
            face.post_script_name,
            face.families
                .iter()
                .map(|(n, _)| n.as_str())
                .take(2)
                .collect::<Vec<_>>()
        ),
        None => "<未找到>".to_string(),
    };

    let mut rows = std::collections::HashMap::new();
    for (idx, sample_ch) in SAMPLE.chars().enumerate() {
        let band = ink_rows(fb, pw, (cw, ch), idx as u32, ch);
        eprintln!("  [{family}] '{sample_ch}' cell#{idx} 墨迹 y = {band:?}");
        rows.insert(sample_ch, band);
    }
    // 度量实测（fontdue）：ascent/descent/line_gap 与按公式算出的 baseline。
    let mut descent_limit = 5i32;
    let dims = {
        use fontdb::Source;
        let mut text = String::from("<未取得度量>");
        if let Some(face) = &lm {
            let bytes: Vec<u8> = match &face.source {
                Source::File(p) => std::fs::read(p).unwrap_or_default(),
                Source::Binary(d) => d.as_ref().as_ref().to_vec(),
                Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
            };
            if let Ok(f) =
                render::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
                && let Some(l) = f.horizontal_line_metrics(size)
            {
                let descent_px = (-l.descent).ceil().max(0.0);
                let gap_px = l.line_gap.ceil().max(0.0);
                let baseline = (ch as f32 - descent_px - gap_px).clamp(0.0, ch as f32) as i32;
                // 逗号等标点自带下行（字体设计），允许其底边到基线下方 descent+1 像素。
                descent_limit = descent_px as i32 + 1;
                text = format!(
                    "ascent={:.2} descent={:.2} line_gap={:.2} new_line_size={:.2} → baseline={baseline} (old formula baseline=0)",
                    l.ascent, l.descent, l.line_gap, l.new_line_size
                );
                for c in ['x', '.', 'g', 'H'] {
                    let m = f.rasterize(c, size).0;
                    text.push_str(&format!(
                        "\n    '{c}': w={} h={} ymin={} ymax_rel={}",
                        m.width,
                        m.height,
                        m.ymin,
                        m.ymin + m.height as i32
                    ));
                }
            }
        }
        text
    };
    eprintln!("[perf-baseline] family='{family}' size={size} cell={cw}x{ch} {lm_text}");
    eprintln!("[perf-baseline] 度量: {dims}");

    let x_band = rows.get(&'x').and_then(|v| *v);
    let Some((_, x_bottom)) = x_band else {
        eprintln!("  SKIP: '{family}' 的 'x' 无墨迹");
        return true;
    };

    let mut ok = true;
    // 1) '.' 底边与 'x' 底边相差 ≤1px（点应坐在基线上）。
    if let Some((_, bottom)) = rows.get(&'.').and_then(|v| *v) {
        let d = (bottom as i32 - x_bottom as i32).abs();
        eprintln!("  [{family}] '.' 底边 {bottom} vs 'x' 底边 {x_bottom} → 差 {d}px");
        if d > 1 {
            eprintln!("  FAIL: [{family}] '.' 基线错位 {d}px（>1）");
            ok = false;
        }
    }
    // 1b) ',' 自带下行（字体设计，实测 DejaVu/Liberation/Noto 均低于基线 2-3px）：
    //     只要求"不得高于 'x' 底边（即不得跑到行上部）"且"不得低于基线下 descent+1"。
    if let Some((_, bottom)) = rows.get(&',').and_then(|v| *v) {
        let d = bottom as i32 - x_bottom as i32;
        eprintln!(
            "  [{family}] ',' 底边 {bottom} vs 'x' 底边 {x_bottom} → 偏移 {d}px（允许 0..={descent_limit}）"
        );
        if d < -1 || d > descent_limit {
            eprintln!("  FAIL: [{family}] ',' 位置异常（偏移 {d}px，允许 -1..={descent_limit}）");
            ok = false;
        }
    }
    // 2) g/p/y 下行：底边 > x 底边
    for desc in ['g', 'p', 'y'] {
        if let Some((_, bottom)) = rows.get(&desc).and_then(|v| *v) {
            eprintln!("  [{family}] '{desc}' 底边 {bottom} vs 'x' 底边 {x_bottom}");
            if bottom <= x_bottom {
                eprintln!("  FAIL: [{family}] '{desc}' 未下行（底边 {bottom} ≤ {x_bottom}）");
                ok = false;
            }
        }
    }
    // 3) H 顶边不低于 x 顶边
    if let (Some((h_top, _)), Some((x_top, _))) = (
        rows.get(&'H').and_then(|v| *v),
        rows.get(&'x').and_then(|v| *v),
    ) {
        eprintln!("  [{family}] 'H' 顶边 {h_top} vs 'x' 顶边 {x_top}");
        if h_top > x_top {
            eprintln!("  FAIL: [{family}] 'H' 顶边 {h_top} 低于 'x' 顶边 {x_top}");
            ok = false;
        }
    }
    ok
}

#[test]
fn baseline_alignment_font_matrix() {
    let mut families = vec![
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "FreeMono",
    ];
    // 测试专用：用户提供的字体文件（不入库）。
    let external = std::env::var("TEST_FONT_FILE").ok();
    if let Some(path) = external.as_deref() {
        if !std::path::Path::new(path).is_file() {
            eprintln!("SKIP: TEST_FONT_FILE='{path}' 不存在");
        } else {
            eprintln!("外部字体文件：{path}");
            families.push("__external__");
        }
    } else {
        eprintln!("提示：设置 TEST_FONT_FILE=<本地 Consolas 字体文件路径> 可把 Consolas 纳入矩阵");
    }

    let mut failed = Vec::new();
    for fam in &families {
        if *fam == "__external__" {
            let path = external.clone().unwrap_or_default();
            match run_external(&path) {
                Ok(true) => {}
                Ok(false) => failed.push("external(TEST_FONT_FILE)"),
                Err(e) => eprintln!("SKIP: 外部字体 {path} 无法加载（{e}）"),
            }
            continue;
        }
        if !check_font(fam, 13.0) {
            failed.push(fam);
        }
    }
    assert!(
        failed.is_empty(),
        "以下字体的绝对基线错位：{failed:?}（详见上方逐字符打印）"
    );
}

/// 直接按文件加载字形度量（不经 fontdb），用于外部字体（Consolas）。
fn run_external(path: &str) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let font = render::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
        .map_err(|e| format!("fontdue: {e}"))?;
    let (px, size) = (13.0f32, 13.0);
    let (cw, ch) = render::cell_metrics(&font, size, 1.0);
    eprintln!("[perf-baseline] external '{path}' cell={cw}x{ch} px={px}");

    // 用与 CpuRenderer 相同的度量与落点公式（基线 = cell_h - ceil(-descent) - ceil(line_gap)，
    // 顶边 = 基线 - ymin - height + gy），复现真实渲染结果（不依赖 fontdb）。
    let (baseline, descent_px) = match font.horizontal_line_metrics(px) {
        Some(l) => {
            let d = (-l.descent).ceil().max(0.0);
            let g = l.line_gap.ceil().max(0.0);
            let b = (ch as f32 - d - g).clamp(0.0, ch as f32) as i32;
            eprintln!(
                "  [external] ascent={:.2} descent={:.2} line_gap={:.2} cell_h={ch} → baseline={b}",
                l.ascent, l.descent, l.line_gap
            );
            (b, d as i32 + 1)
        }
        None => (0, 5),
    };
    let mut ok = true;
    let mut ink = std::collections::HashMap::new();
    for ch in SAMPLE.chars() {
        let (m, bitmap) = font.rasterize(ch, px);
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for gy in 0..m.height {
            for gx in 0..m.width {
                if bitmap[gy * m.width + gx] != 0 {
                    // CpuRenderer 的落点公式（阶段 4B-fix1 修正后）：
                    //   py = cell_y + baseline - ymin - height + gy（cell_y = 0）
                    let py = baseline - m.ymin - m.height as i32 + gy as i32;
                    min_y = min_y.min(py);
                    max_y = max_y.max(py);
                }
            }
        }
        eprintln!(
            "  [external] '{ch}' metrics w={} h={} xmin={} ymin={} → 墨迹 y {min_y}..{max_y}",
            m.width, m.height, m.xmin, m.ymin
        );
        if min_y != i32::MAX {
            ink.insert(ch, (min_y, max_y));
        }
    }
    if let Some(&(_, x_bottom)) = ink.get(&'x') {
        if let Some(&(_, bottom)) = ink.get(&'.') {
            let d = (bottom - x_bottom).abs();
            eprintln!("  [external] '.' 底边 {bottom} vs 'x' {x_bottom} → 差 {d}px");
            if d > 1 {
                eprintln!("  FAIL: [external] '.' 基线错位 {d}px（>1）");
                ok = false;
            }
        }
        if let Some(&(_, bottom)) = ink.get(&',') {
            let d = bottom - x_bottom;
            eprintln!("  [external] ',' 底边 {bottom} vs 'x' {x_bottom} → 偏移 {d}px");
            if d < -1 || d > descent_px {
                eprintln!("  FAIL: [external] ',' 位置异常（偏移 {d}px）");
                ok = false;
            }
        }
        for desc in ['g', 'p', 'y'] {
            if let Some(&(_, bottom)) = ink.get(&desc) {
                eprintln!("  [external] '{desc}' 底边 {bottom} vs 'x' {x_bottom}");
                if bottom <= x_bottom {
                    eprintln!("  FAIL: [external] '{desc}' 未下行");
                    ok = false;
                }
            }
        }
        if let (Some(&(h_top, _)), Some(&(x_top, _))) = (ink.get(&'H'), ink.get(&'x'))
            && h_top > x_top
        {
            eprintln!("  FAIL: [external] 'H' 顶边 {h_top} 低于 'x' 顶边 {x_top}");
            ok = false;
        }
    }
    Ok(ok)
}

/// GPU 后端的绝对基线（任务 A 要求 CPU/GPU 各跑；无适配器显式 SKIP 并计数）。
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn baseline_alignment_gpu() {
    use render::gpu::GpuRenderer;
    use render::{Error, Renderer};

    let mut gpu = match GpuRenderer::new_offscreen(
        &settings("DejaVu Sans Mono", 13.0),
        RenderOptions::default(),
        Palette::default(),
        render::gpu::allow_software_from_env(),
    ) {
        Ok(g) => g,
        Err(Error::Backend(msg)) => {
            eprintln!("SKIP: 无可用真实 GPU 适配器（{msg}）");
            return;
        }
        Err(e) => panic!("GpuRenderer 创建失败: {e}"),
    };
    let (cw, ch) = gpu.cell_metrics(1.0);
    let snap = one_row(SAMPLE);
    let pw = cw * SAMPLE.chars().count() as u32;
    gpu.render(&snap, pw, ch).expect("gpu render");
    let rgba = gpu.framebuffer_rgba().to_vec();

    // 转成与 CPU 相同的 0x00RRGGBB 视图后复用同一套判据。
    let mut fb = vec![0u32; (pw * ch) as usize];
    for y in 0..ch {
        for x in 0..pw {
            let i = (y * pw + x) as usize;
            fb[i] = (u32::from(rgba[i * 4]) << 16)
                | (u32::from(rgba[i * 4 + 1]) << 8)
                | u32::from(rgba[i * 4 + 2]);
        }
    }
    let mut rows = std::collections::HashMap::new();
    for (idx, ch_ch) in SAMPLE.chars().enumerate() {
        let band = ink_rows(&fb, pw, (cw, ch), idx as u32, ch);
        eprintln!("  [gpu] '{ch_ch}' cell#{idx} 墨迹 y = {band:?}");
        rows.insert(ch_ch, band);
    }
    let (_, x_bottom) = rows.get(&'x').and_then(|v| *v).expect("'x' 无墨迹");
    if let Some((_, bottom)) = rows.get(&'.').and_then(|v| *v) {
        let d = (bottom as i32 - x_bottom as i32).abs();
        eprintln!("  [gpu] '.' 底边 {bottom} vs 'x' {x_bottom} → 差 {d}px");
        assert!(d <= 1, "GPU '.' 基线错位 {d}px");
    }
    for desc in ['g', 'p', 'y'] {
        if let Some((_, bottom)) = rows.get(&desc).and_then(|v| *v) {
            eprintln!("  [gpu] '{desc}' 底边 {bottom} vs 'x' {x_bottom}");
            assert!(bottom > x_bottom, "GPU '{desc}' 未下行");
        }
    }
    if let (Some((h_top, _)), Some((x_top, _))) = (
        rows.get(&'H').and_then(|v| *v),
        rows.get(&'x').and_then(|v| *v),
    ) {
        eprintln!("  [gpu] 'H' 顶边 {h_top} vs 'x' 顶边 {x_top}");
        assert!(h_top <= x_top, "GPU 'H' 顶边低于 'x'");
    }
}
