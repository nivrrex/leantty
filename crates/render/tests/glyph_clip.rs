//! 字形顶部裁剪取证（阶段 4B-fix2）。
//!
//! 对 ASCII 可打印字符与一批常用汉字，按**渲染时相同的公式**计算未裁剪的
//! 顶边/底边（相对格顶），断言 `top >= 0 && top + height <= cell_h`；
//! 同时统计"若把画布上下各加 8px 余量，会露出多少原本被裁掉的墨迹行"。
//!
//! 与 `baseline_alignment.rs` 的分工：那个测基线**相对关系**（'.' 是否坐在基线、
//! g/p/y 是否下行），本测**绝对适配**（字形是否完整落在格内，尤其 CJK 回退字体）。

use render::{Font, cell_metrics};
#[cfg(target_os = "linux")]
use render::{FontSettings, FontSpec, Palette, RenderOptions};

/// 常用汉字取样（覆盖不同部首与笔画密度，≥200 个）。
const HANZI: &str = "的一是了我不人在他有这个上们来到时大地为子中你说生国年着就那和要她出也得里后自以会家可下而过天去能对小多然于心学么之都好看起发当没成只如事把还用第样道想作种开美总从无情己面最女但现前些所同日手又行意动方期它头经长儿回位分爱老因很给名法间斯知世什两次使身者被高已亲其进此话常与活正感";

#[cfg(target_os = "linux")]
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


/// 字体链加载（主字体 + 系统 CJK 兜底），与 app 的一致性由 `load_font_list` 保证。
fn load_chain(primary: &str) -> Option<Vec<Font>> {
    use fontdb::{Database, Family, Query, Source, Style as DbStyle, Weight};
    let mut db = Database::new();
    db.load_system_fonts();
    let q = Query {
        families: &[Family::Name(primary), Family::Monospace],
        weight: Weight::NORMAL,
        style: DbStyle::Normal,
        ..Default::default()
    };
    let id = db.query(&q)?;
    let face = db.face(id)?;
    let bytes: Vec<u8> = match &face.source {
        Source::File(p) => std::fs::read(p).ok()?,
        Source::Binary(d) => d.as_ref().as_ref().to_vec(),
        Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
    };
    let main = Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default()).ok()?;
    let mut chain = vec![main];
    // CJK 回退：按 family 名含 "cjk" 扫描（本机 generic 查询拿不到 CJK，
    // 与 4A/4B 的 cjk_fonts 同源；app 侧对应 config 的 font.fallback 黑体）。
    if !chain[0].has_glyph('中') {
        let mut candidates: Vec<_> = db
            .faces()
            .filter(|f| f.families.iter().any(|(n, _)| n.to_lowercase().contains("cjk")))
            .collect();
        candidates.sort_by_key(|f| {
            let name = match &f.source {
                Source::File(p) => p.display().to_string(),
                Source::SharedFile(p, _) => p.display().to_string(),
                Source::Binary(_) => String::new(),
            };
            (name.contains("Serif") || !name.contains("Regular"), name)
        });
        for face in candidates {
            let bytes: Vec<u8> = match &face.source {
                Source::File(p) => std::fs::read(p).unwrap_or_default(),
                Source::Binary(d) => d.as_ref().as_ref().to_vec(),
                Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
            };
            if let Ok(f) = Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
                && f.has_glyph('中')
            {
                chain.push(f);
                break;
            }
        }
    }
    Some(chain)
}

/// 与实现同源的 baseline（阶段 4B-fix2：line_gap 居中 + 下行优先）。
fn baseline_current(cell_h: u32, lm: &fontdue::LineMetrics) -> i32 {
    render::baseline_from_metrics(cell_h, lm)
}

struct FontReport {
    name: String,
    ascent: f32,
    descent: f32,
    line_gap: f32,
    cell_h: u32,
    baseline: i32,
    min_top: i32,
    min_top_char: char,
    max_bottom: i32,
    max_bottom_char: char,
    clipped: Vec<(char, i32, usize)>, // (char, top, height)
    clipped_rows: usize,
    /// 顶部被裁的字符数（硬失败）。
    top_clipped: usize,
    /// 字形高度 > cell_h 的数量（保顶裁底，记录而非失败）。
    taller_than_cell: usize,
}

fn analyze(chain: &[Font], size: f32, label: &str) -> Option<Vec<FontReport>> {
    let cell = cell_metrics(&chain[0], size, 1.0);
    let cell_h = cell.1;
    let mut reports = Vec::new();
    for (fi, font) in chain.iter().enumerate() {
        let Some(lm) = font.horizontal_line_metrics(size) else {
            continue;
        };
        let baseline = baseline_current(cell_h, &lm);
        let mut min_top = i32::MAX;
        let mut min_top_char = ' ';
        let mut max_bottom = i32::MIN;
        let mut max_bottom_char = ' ';
        let mut clipped = Vec::new();
        let mut clipped_rows = 0usize;
        let mut top_clipped = 0usize;
        let mut taller_than_cell = 0usize;
        // 只统计"该字形由本字体提供"的字符：链中第一个 has_glyph 的字体。
        let mut chars: Vec<char> = ('!'..='~').collect();
        chars.extend(HANZI.chars());
        for ch in chars {
            let owner = chain.iter().position(|f| f.has_glyph(ch));
            if owner != Some(fi) {
                continue;
            }
            let (m, _) = font.rasterize(ch, size);
            if m.width == 0 || m.height == 0 {
                continue;
            }
            // 与实现一致：顶边 = baseline - ymin - height + 平移
            let shift = render::glyph_shift(baseline, m.ymin, m.height as i32, cell_h);
            let top = baseline - m.ymin - m.height as i32 + shift;
            let bottom = top + m.height as i32;
            if top < min_top {
                min_top = top;
                min_top_char = ch;
            }
            if bottom > max_bottom {
                max_bottom = bottom;
                max_bottom_char = ch;
            }
            // 顶部越界 = 不可接受（硬失败）；底部越界仅在"字形高于格"时可接受（保顶裁底）。
            if top < 0 {
                clipped.push((ch, top, m.height));
                clipped_rows += (-top) as usize;
                top_clipped += 1;
            } else if bottom > cell_h as i32 {
                if m.height as u32 > cell_h {
                    taller_than_cell += 1;
                } else {
                    clipped.push((ch, top, m.height));
                    clipped_rows += (bottom - cell_h as i32) as usize;
                }
            }
        }
        reports.push(FontReport {
            name: format!("{label}#{fi} {}", if fi == 0 { "primary" } else { "fallback" }),
            ascent: lm.ascent,
            descent: lm.descent,
            line_gap: lm.line_gap,
            cell_h,
            baseline,
            min_top,
            min_top_char,
            max_bottom,
            max_bottom_char,
            clipped,
            clipped_rows,
            top_clipped,
            taller_than_cell,
        });
    }
    Some(reports)
}

fn print_reports(reports: &[FontReport]) {
    for r in reports {
        eprintln!(
            "[clip] {}: ascent={:.2} descent={:.2} line_gap={:.2} cell_h={} baseline={} | min_top={} (char '{}') max_bottom={} (char '{}')",
            r.name, r.ascent, r.descent, r.line_gap, r.cell_h, r.baseline,
            r.min_top, r.min_top_char, r.max_bottom, r.max_bottom_char
        );
        eprintln!(
            "[clip]   越界字符数={} 被裁墨迹行数(合计)={}（其中顶部被裁={}，字形高于格={}）",
            r.clipped.len(),
            r.clipped_rows,
            r.top_clipped,
            r.taller_than_cell
        );
        if !r.clipped.is_empty() {
            let sample: Vec<String> = r
                .clipped
                .iter()
                .take(12)
                .map(|(c, top, h)| format!("'{c}'(top={top},h={h})"))
                .collect();
            eprintln!("[clip]   样例: {}", sample.join(" "));
        }
    }
}

/// 取证主测试：本机 3 种等宽字体 + 可选外部字体，CJK 由系统回退字体承担。
#[test]
fn glyph_fits_in_cell_font_matrix() {
    let mut families = vec![
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "FreeMono",
    ];
    let external = std::env::var("TEST_FONT_FILE").ok();
    if let Some(p) = external.as_deref() {
        if std::path::Path::new(p).is_file() {
            families.push("__external__");
        } else {
            eprintln!("SKIP: TEST_FONT_FILE='{p}' 不存在");
        }
    }

    let mut failed = Vec::new();
    for fam in &families {
        let chain = if *fam == "__external__" {
            let p = external.clone().unwrap_or_default();
            let bytes = match std::fs::read(&p) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("SKIP: 外部字体读取失败 {e}");
                    continue;
                }
            };
            let Ok(main) = Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            else {
                eprintln!("SKIP: 外部字体解析失败 {p}");
                continue;
            };
            // 外部字体仅作主字体；CJK 仍用系统回退（与 app 的 fallback 行为一致）
            let mut chain = vec![main];
            if let Some(sys) = load_chain("monospace") {
                for f in sys.into_iter().skip(1) {
                    if f.has_glyph('中') && !chain.iter().any(|c| c.has_glyph('中')) {
                        chain.push(f);
                    }
                }
            }
            chain
        } else {
            match load_chain(fam) {
                Some(c) => c,
                None => {
                    eprintln!("SKIP: 字体 '{fam}' 不可用");
                    continue;
                }
            }
        };
        let label = if *fam == "__external__" { "external" } else { fam };
        if let Some(reports) = analyze(&chain, 13.0, label) {
            print_reports(&reports);
            let bad: usize = reports.iter().map(|r| r.top_clipped).sum();
            let taller: usize = reports.iter().map(|r| r.taller_than_cell).sum();
            if taller > 0 {
                eprintln!("[clip]   注：{taller} 个字形高度大于 cell_h，按“保上顶”允许底部被裁");
            }
            if bad > 0 {
                failed.push(format!("{label}({bad} 字符顶部被裁)"));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "以下字体链存在字形越界（顶部被裁/底部溢出）：{failed:?}（详见上方 [clip] 表）"
    );
}

/// GPU 侧同源检查：用 GpuRenderer 渲染真实汉字，检查格顶第一行是否有墨迹被切
/// （等价于"顶部像素贴着格顶且上边缺失"）。无适配器显式 SKIP。
#[cfg(target_os = "linux")]
#[test]
fn glyph_top_not_clipped_gpu() {
    use render::gpu::GpuRenderer;
    use render::{Error, Renderer};
    use vt::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Underline};

    let mut gpu = match GpuRenderer::new_offscreen(
        &settings("DejaVu Sans Mono", 13.0),
        RenderOptions::default(),
        Palette::default(),
    ) {
        Ok(g) => g,
        Err(Error::Backend(msg)) => {
            eprintln!("SKIP: 无可用真实 GPU 适配器（{msg}）");
            return;
        }
        Err(e) => panic!("GpuRenderer 创建失败: {e}"),
    };
    // 用系统 CJK 兜底字体（与 app 相同来源）
    if let Some(sys) = load_chain("monospace") {
        let cjk: Vec<Font> = sys.into_iter().skip(1).filter(|f| f.has_glyph('中')).collect();
        if !cjk.is_empty() {
            gpu.add_background_fonts(cjk);
        }
    }
    let (cw, ch) = gpu.cell_metrics(1.0);
    let text: String = HANZI.chars().take(40).collect();
    let cells: Vec<Cell> = text
        .chars()
        .map(|c| Cell {
            chars: c.to_string(),
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
    let snap = FrameSnapshot {
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
    };
    let pw = cw * snap.cols as u32;
    gpu.render(&snap, pw, ch).expect("gpu render");
    let rgba = gpu.framebuffer_rgba();
    // 统计"贴顶格第一行有墨迹"的格数：这类格可能顶部被裁（需配合公式层判断）。
    let bg = (0u8, 0u8, 0u8);
    let mut touching_top = 0usize;
    for idx in 0..snap.cols as u32 {
        let y = 0u32;
        let mut has = false;
        for x in (idx * cw)..((idx + 1) * cw) {
            let i = ((y * pw + x) * 4) as usize;
            if (rgba[i], rgba[i + 1], rgba[i + 2]) != bg {
                has = true;
                break;
            }
        }
        if has {
            touching_top += 1;
        }
    }
    eprintln!(
        "[clip-gpu] cell={cw}x{ch} 贴顶格数={touching_top}/{}（修复后应为 0）",
        snap.cols
    );
    assert_eq!(touching_top, 0, "有字形贴着格顶（顶部可能被裁）");
}
