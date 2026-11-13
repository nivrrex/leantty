//! spike 公共部分：固定测试帧构造、CPU 参考渲染、像素对比与统计。
//!
//! 只读使用主仓库 `vt` 与 `render` crate（path 依赖），不修改其代码。

#![forbid(unsafe_code)]

pub mod atlas;
pub mod instances;

use std::time::Duration;

/// 固定测试内容：中英文、样式（粗体/斜体/下划线/反色/暗淡）、光标、选区。
///
/// 行内容在 `cols` 不足时会被终端截断，`rows` 不足时只保留前 `rows` 行，
/// 因此同一函数可用于 120x30 与 200x60 两种尺寸。
const CONTENT: &[u8] = concat!(
    "=== tty gpu spike ===\r\n",
    "Hello, World! The quick brown fox jumps over the lazy dog 0123456789\r\n",
    "\x1b[1mBOLD\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4munderline\x1b[0m \x1b[7mreverse\x1b[0m \x1b[2mfaint\x1b[0m\r\n",
    "\x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m \x1b[34mblue\x1b[0m \x1b[33myellow\x1b[0m \x1b[35mmagenta\x1b[0m \x1b[36mcyan\x1b[0m\r\n",
    "中文测试：终端渲染验证，包含标点，以及全角符号。日本語テスト、한국어 테스트.\r\n",
    "mixed 混排 abc 汉字 def かな 가나 0123 zzz\r\n",
    "\x1b[44m  blue background  \x1b[0m and \x1b[41m  red bg  \x1b[0m\r\n",
    "tab:\there  after tabs\r\n",
    "underline+dots: \x1b[4:3mcurly\x1b[0m  @#$%^&*()_+-=[]{}|;':\",./<>?\r\n",
    "last line before prompt\r\n",
    "\x1b[1;32m$\x1b[0m echo done\r\n",
)
.as_bytes();

/// 构造固定测试帧：返回渲染用快照（尺寸与内容固定，不含时间依赖）。
///
/// `cols`/`rows` 为终端网格尺寸；返回的快照已强制 `full_redraw = true`
/// （spike 只测全量重绘路径）。
pub fn build_frame(cols: u16, rows: u16) -> Result<vt::FrameSnapshot, String> {
    let mut term = vt::VtTerminal::new(cols, rows).map_err(|e| format!("VtTerminal::new: {e}"))?;
    // 用 ANSI 清屏 + 定位，避免依赖终端初始状态。
    term.feed(b"\x1b[2J\x1b[H");
    term.feed(CONTENT);
    for line in 0..rows.min(3) {
        term.feed(format!("\r\nline {line:02} 追加内容 padded to make width").as_bytes());
    }
    // 光标停在可见位置（非最后一行，避免滚动）。
    term.feed(b"\x1b[5;12H");
    let mut snap = term.snapshot().map_err(|e| format!("snapshot: {e}"))?;
    snap.full_redraw = true;
    for d in snap.dirty_rows.iter_mut() {
        *d = true;
    }
    Ok(snap)
}

/// 选区（内容行号，绝对坐标由调用方决定）：覆盖第 2–4 行、第 0–30 列。
pub const SELECTION: (u16, u16, u16, u16) = (2, 0, 4, 30);

/// CPU 参考渲染：用主仓库 `CpuRenderer` 渲染同一快照。
///
/// 返回 (framebuffer_u32, width, height, cell_w, cell_h)。
/// 帧缓冲格式为 0xAARRGGBB（低 24 位 RGB，同 app 提交给 softbuffer 的格式）。
pub fn cpu_reference(
    snap: &vt::FrameSnapshot,
    pixel_w: u32,
    pixel_h: u32,
    scale: f64,
) -> Result<(Vec<u32>, u32, u32, u32, u32), String> {
    let mut r = make_cpu_renderer(scale)?;
    r.set_selection(Some(SELECTION));
    use render::Renderer as _;
    r.render(snap, pixel_w, pixel_h)
        .map_err(|e| format!("CpuRenderer::render: {e}"))?;
    let (w, h) = r.framebuffer_size();
    let (cw, ch) = r.cell_metrics(scale);
    Ok((r.framebuffer().to_vec(), w, h, cw, ch))
}

/// 构造与 `cpu_reference` 相同参数的 CPU 渲染器（供逐帧计时复用）。
///
/// 关键：必须通过公开 API `add_background_fonts` 补上 CJK 兜底字体。主程序里这是
/// app 层的后台懒加载；spike 不经过 app，若跳过这一步，CPU 参考帧的中文字形会退化为
/// U+25A1 占位方块，与 GPU（用 NotoSansCJK）的差异就完全是"字体不同"而非渲染差异。
pub fn make_cpu_renderer(scale: f64) -> Result<render::CpuRenderer, String> {
    let settings = default_font_settings();
    let mut r = render::CpuRenderer::new(
        &settings,
        render::RenderOptions::default(),
        render::Palette::default(),
    )
    .map_err(|e| format!("CpuRenderer::new: {e}"))?;
    r.set_scale_factor(scale);
    r.add_background_fonts(cjk_fonts()?);
    Ok(r)
}

/// CJK 兜底字体：按 family 名含 "CJK" 扫描系统字体库并验证确实含中文字形。
///
/// 不用 `Family::Monospace` generic 查询：fontdb 的 generic 解析在本机得到的是
/// 不含 CJK 的等宽字体（DejaVu 系），主程序里 CJK 实际由 app 层显式加载的 fallback
/// 字体（config font.fallback / Noto Sans CJK）提供。两侧必须用同一来源，
/// 否则对比差异会全部来自"字体不同"。
pub fn cjk_fonts() -> Result<Vec<render::Font>, String> {
    // 进程级缓存：NotoSansCJK 是 19MB 的 ttc，重复解析一次约 200ms；
    // 四组字体链与 CPU/GPU 两侧都要用同一份，缓存后只解析一次。
    static CACHE: std::sync::OnceLock<Vec<render::Font>> = std::sync::OnceLock::new();
    if let Some(v) = CACHE.get() {
        return Ok(v.clone());
    }
    let fonts = load_cjk_fonts_uncached()?;
    let _ = CACHE.set(fonts.clone());
    Ok(fonts)
}

/// face 的来源文件路径（用于候选排序；无文件源返回 None）。
fn source_path(face: &fontdb::FaceInfo) -> Option<String> {
    match &face.source {
        fontdb::Source::File(p) => Some(p.display().to_string()),
        fontdb::Source::SharedFile(p, _) => Some(p.display().to_string()),
        fontdb::Source::Binary(_) => None,
    }
}

fn load_cjk_fonts_uncached() -> Result<Vec<render::Font>, String> {
    use fontdb::{Database, Source};
    let mut db = Database::new();
    db.load_system_fonts();
    let mut out = Vec::new();
    let mut visited: Vec<String> = Vec::new();
    // 候选按"文件名含 Regular 且不含 Serif"优先，只取第一个真正含中文字形的字体：
    // 本机有 14 个 CJK ttc（各 18-26MB），全部解析会让 fontdue 内存暴涨（实测被 OOM 杀）。
    let mut faces: Vec<_> = db
        .faces()
        .filter(|f| f.families.iter().any(|(n, _)| n.to_lowercase().contains("cjk")))
        .collect();
    faces.sort_by_key(|f| {
        let name = source_path(f).unwrap_or_default();
        let bad = name.contains("Serif") || name.contains("Bold") || name.contains("Black")
            || name.contains("Light") || name.contains("Medium") || name.contains("Thin");
        (bad, !name.contains("Regular"))
    });
    for face in faces {
        let (path_key, bytes) = match &face.source {
            Source::File(path) => {
                let p = path.display().to_string();
                if visited.contains(&p) {
                    continue;
                }
                (p, std::fs::read(path).map_err(|e| format!("read font: {e}"))?)
            }
            Source::Binary(data) => (format!("mem:{:p}", data), data.as_ref().as_ref().to_vec()),
            Source::SharedFile(path, data) => {
                let p = path.display().to_string();
                if visited.contains(&p) {
                    continue;
                }
                (p, data.as_ref().as_ref().to_vec())
            }
        };
        if let Ok(f) = render::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            && f.has_glyph('中')
        {
            visited.push(path_key);
            out.push(f);
            // 只取一个 CJK 兜底字体（更多候选对渲染无益，且显著增加内存）。
            break;
        }
    }
    if out.is_empty() {
        // 回落：仍用 generic（此时两侧一致，但中文会退化为占位方块，由调用方报告）。
        use fontdb::{Family, Query, Style as DbStyle, Weight};
        for fam in [Family::Monospace, Family::SansSerif] {
            let q = Query {
                families: &[fam],
                weight: Weight::NORMAL,
                style: DbStyle::Normal,
                ..Default::default()
            };
            let Some(id) = db.query(&q) else { continue };
            let Some(face) = db.face(id) else { continue };
            let bytes: Vec<u8> = match &face.source {
                Source::File(path) => std::fs::read(path).map_err(|e| format!("read font: {e}"))?,
                Source::Binary(data) => data.as_ref().as_ref().to_vec(),
                Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
            };
            if let Ok(f) =
                render::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            {
                out.push(f);
            }
        }
    }
    Ok(out)
}

/// 渲染一帧（计时用；调用方负责 set_selection 等状态）。
pub fn cpu_render(
    r: &mut render::CpuRenderer,
    snap: &vt::FrameSnapshot,
    pixel_w: u32,
    pixel_h: u32,
) -> Result<(), String> {
    use render::Renderer as _;
    r.render(snap, pixel_w, pixel_h)
        .map_err(|e| format!("CpuRenderer::render: {e}"))
}

fn default_font_settings() -> render::FontSettings {
    let spec = |style: &str| render::FontSpec {
        family: "monospace".to_string(),
        style: style.to_string(),
    };
    render::FontSettings {
        size: 16.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
    }
}

/// 默认背景色（Palette::default 的 primary_bg），用于区分"背景格"与"字形像素"。
pub fn default_bg() -> vt::Rgb {
    render::Palette::default().primary_bg
}

/// 像素差异报告。
#[derive(Debug, Default, Clone)]
pub struct DiffReport {
    /// 总像素数。
    pub total: u64,
    /// 全帧差异像素数（任一通道不等）。
    pub diff_pixels: u64,
    /// 最大通道绝对差。
    pub max_channel_diff: u32,
    /// 背景区域（CPU 侧等于默认背景色的像素）差异数。
    pub bg_diff_pixels: u64,
    /// 前景/字形区域（CPU 侧 ≠ 默认背景色）差异数。
    pub glyph_diff_pixels: u64,
    /// 前景/字形区域像素总数。
    pub glyph_pixels: u64,
    /// 前景区域最大通道差。
    pub glyph_max_channel_diff: u32,
}

/// 比较 CPU 参考帧（u32 0xAARRGGBB）与 GPU 回读帧（u8 RGBA8）。
pub fn compare(cpu: &[u32], gpu_rgba: &[u8], w: u32, h: u32) -> DiffReport {
    let bg = default_bg();
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
    let n = (w as usize) * (h as usize);
    let mut rep = DiffReport {
        total: n as u64,
        ..Default::default()
    };
    for i in 0..n.min(cpu.len()).min(gpu_rgba.len() / 4) {
        let c = cpu[i];
        let cr = ((c >> 16) & 0xFF) as i32;
        let cg = ((c >> 8) & 0xFF) as i32;
        let cb = (c & 0xFF) as i32;
        let gr = gpu_rgba[i * 4] as i32;
        let gg = gpu_rgba[i * 4 + 1] as i32;
        let gb = gpu_rgba[i * 4 + 2] as i32;
        let d = [(cr - gr).unsigned_abs(), (cg - gg).unsigned_abs(), (cb - gb).unsigned_abs()];
        let maxd = d[0].max(d[1]).max(d[2]);
        let is_bg = (c & 0x00FF_FFFF) == bg_u32;
        if !is_bg {
            rep.glyph_pixels += 1;
        }
        if maxd > 0 {
            rep.diff_pixels += 1;
            rep.max_channel_diff = rep.max_channel_diff.max(maxd);
            if is_bg {
                rep.bg_diff_pixels += 1;
            } else {
                rep.glyph_diff_pixels += 1;
                rep.glyph_max_channel_diff = rep.glyph_max_channel_diff.max(maxd);
            }
        }
    }
    rep
}

/// 把 CPU 与 GPU 的同一单元格打成 ASCII 灰度图（差异定位用）。
pub fn dump_cell(cpu: &[u32], gpu: &[u8], w: u32, cell: (u32, u32), cx: u32, cy: u32) {
    let (cw, ch) = cell;
    let ramp = b" .:-=+*#%@";
    let mut a = String::new();
    let mut b = String::new();
    for y in 0..ch {
        for x in 0..cw {
            let px = cx * cw + x;
            let py = cy * ch + y;
            let i = (py * w + px) as usize;
            let c = cpu[i];
            let cg = ((c >> 16) & 0xFF) as usize;
            let gg = usize::from(gpu[i * 4]);
            a.push(ramp[(cg * 9 / 255).min(9)] as char);
            b.push(ramp[(gg * 9 / 255).min(9)] as char);
        }
        a.push('\n');
        b.push('\n');
    }
    eprintln!("[dump] cell ({cx},{cy}) CPU:\n{a}[dump] GPU:\n{b}");
}

/// 差异定位：按 cell 归因，返回前 `top` 个差异最多的 cell。
///
/// 每项：(cell_x, cell_y, 差异像素数, CPU 该格样例颜色, GPU 该格样例颜色)。
pub fn diff_by_cell(
    cpu: &[u32],
    gpu_rgba: &[u8],
    w: u32,
    h: u32,
    cell: (u32, u32),
    top: usize,
) -> Vec<(u32, u32, u32, u32, u32)> {
    let (cw, ch) = cell;
    if cw == 0 || ch == 0 {
        return Vec::new();
    }
    let cols = w.div_ceil(cw);
    let rows = h.div_ceil(ch);
    let mut counts = vec![0u32; (cols * rows) as usize];
    let mut sample_cpu = vec![0u32; counts.len()];
    let mut sample_gpu = vec![0u32; counts.len()];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if i >= cpu.len() || i * 4 + 2 >= gpu_rgba.len() {
                continue;
            }
            let c = cpu[i];
            let g = (u32::from(gpu_rgba[i * 4]) << 16)
                | (u32::from(gpu_rgba[i * 4 + 1]) << 8)
                | u32::from(gpu_rgba[i * 4 + 2]);
            if c & 0x00FF_FFFF == g {
                continue;
            }
            let ci = ((y / ch) * cols + (x / cw)) as usize;
            counts[ci] += 1;
            if counts[ci] == 1 {
                sample_cpu[ci] = c & 0x00FF_FFFF;
                sample_gpu[ci] = g;
            }
        }
    }
    let mut idx: Vec<usize> = (0..counts.len()).filter(|i| counts[*i] > 0).collect();
    idx.sort_unstable_by_key(|i| std::cmp::Reverse(counts[*i]));
    idx.into_iter()
        .take(top)
        .map(|i| {
            (
                (i as u32) % cols,
                (i as u32) / cols,
                counts[i],
                sample_cpu[i],
                sample_gpu[i],
            )
        })
        .collect()
}

/// 中位与 p95（毫秒）。空样本返回 0。
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub median_ms: f64,
    pub p95_ms: f64,
}

/// 由耗时样本计算中位与 p95（nearest-rank）。
pub fn stats(mut samples: Vec<Duration>) -> Stats {
    if samples.is_empty() {
        return Stats::default();
    }
    samples.sort_unstable();
    let n = samples.len();
    let median = samples[n / 2].as_secs_f64() * 1000.0;
    let idx = ((n as f64) * 0.95).ceil() as usize;
    let p95 = samples[idx.saturating_sub(1).min(n - 1)].as_secs_f64() * 1000.0;
    Stats {
        median_ms: median,
        p95_ms: p95,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_has_cjk_and_styles() {
        let snap = build_frame(120, 30).expect("build_frame");
        assert_eq!(snap.cols, 120);
        assert_eq!(snap.rows, 30);
        let text: String = snap
            .cells
            .iter()
            .flat_map(|r| r.iter().map(|c| c.chars.clone()))
            .collect();
        assert!(text.contains('中'), "应包含中文字形");
        assert!(text.contains('H'), "应包含 ASCII");
        let has_bold = snap.cells.iter().flatten().any(|c| c.bold);
        let has_underline = snap
            .cells
            .iter()
            .flatten()
            .any(|c| c.underline != vt::Underline::None);
        let has_inverse = snap.cells.iter().flatten().any(|c| c.inverse);
        assert!(has_bold && has_underline && has_inverse, "样式应存在");
        assert!(snap.full_redraw);
    }

    #[test]
    fn stats_median_and_p95() {
        let s = stats((1..=100).map(Duration::from_millis).collect());
        assert!((s.median_ms - 51.0).abs() < 1e-6);
        assert!((s.p95_ms - 95.0).abs() < 1e-6);
        assert_eq!(stats(Vec::new()).median_ms, 0.0);
    }

    #[test]
    fn compare_detects_channel_diff() {
        let cpu = vec![0xFF00_0000u32; 4];
        let mut gpu = vec![0u8; 16];
        for i in 0..4 {
            gpu[i * 4 + 3] = 255;
        }
        assert_eq!(compare(&cpu, &gpu, 2, 2).diff_pixels, 0);
        gpu[0] = 1;
        let rep = compare(&cpu, &gpu, 2, 2);
        assert_eq!(rep.diff_pixels, 1);
        assert_eq!(rep.max_channel_diff, 1);
        assert_eq!(rep.bg_diff_pixels, 1);
    }
}
