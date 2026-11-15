//! 反色（inverse）绝对正确性测试（阶段 4C t4）。
//!
//! 与 `gpu_parity`（CPU↔GPU 相对对比）不同：这里断言**绝对预期像素**——
//! inverse 单元格应当"原背景色作底、原前景色作字"，而不是 CPU/GPU 互相一致地为黑。
//!
//! 已知缺陷（4B 报告）：`CpuRenderer::blit` 的混合底用的是**填充背景色**而非
//! inverse 交换后的 `bgc`，导致反色格渲染成"黑底黑字"（默认调色板下完全不可见）。
//! 本测试在修复前必须为红。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::Rgb;
use vt::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Underline};

fn settings() -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: "DejaVu Sans Mono".to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 13.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
    }
}

fn cell(ch: char) -> Cell {
    Cell {
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
    }
}

fn snap_of(cells: Vec<Cell>) -> FrameSnapshot {
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

fn eq_rgb(px: u32, c: Rgb) -> bool {
    ((px >> 16) & 0xFF) == u32::from(c.r)
        && ((px >> 8) & 0xFF) == u32::from(c.g)
        && (px & 0xFF) == u32::from(c.b)
}

/// 统计某格内"等于指定颜色"的像素数。
fn count_color(fb: &[u32], pw: u32, cw: u32, ch: u32, idx: u32, c: Rgb) -> usize {
    let mut n = 0;
    for y in 0..ch {
        for x in (idx * cw)..((idx + 1) * cw) {
            if eq_rgb(fb[(y * pw + x) as usize], c) {
                n += 1;
            }
        }
    }
    n
}

fn render_one(cells: Vec<Cell>, palette: Palette) -> (Vec<u32>, u32, u32, u32, u32) {
    let mut r = CpuRenderer::new(&settings(), RenderOptions::default(), palette).expect("CpuRenderer");
    let (cw, ch) = r.cell_metrics(1.0);
    let snap = snap_of(cells);
    let pw = cw * snap.cols as u32;
    r.render(&snap, pw, ch).expect("render");
    let fb = r.framebuffer().to_vec();
    (fb, pw, cw, ch, snap.cols as u32)
}

/// 1) 默认调色板：inverse 格应为"白底黑字"（原 fg=白、原 bg=黑）。
#[test]
fn inverse_cell_renders_white_background_black_glyph() {
    let p = Palette::default();
    let mut c = cell('H');
    c.inverse = true;
    let (fb, pw, cw, ch, _) = render_one(vec![c], p);
    let white = count_color(&fb, pw, cw, ch, 0, p.primary_fg);
    let black = count_color(&fb, pw, cw, ch, 0, p.primary_bg);
    let total = (cw * ch) as usize;
    eprintln!("inverse 'H': 白像素={white} 黑像素={black} 总={total}");
    assert!(
        white * 2 > total,
        "反色格底色应为主要为原前景色（白），实测白像素 {white}/{total}（缺陷：黑底黑字）"
    );
    assert!(black > 0, "反色格应有字形像素（原背景色=黑）");
}

/// 2) 带颜色的前景/背景：inverse 后底色=原 fg 色、字色=原 bg 色。
#[test]
fn inverse_cell_swaps_colored_fg_and_bg() {
    let p = Palette::default();
    let fg = Rgb { r: 0x20, g: 0x80, b: 0x40 }; // 绿
    let bg = Rgb { r: 0xC0, g: 0x40, b: 0x20 }; // 橙
    let mut c = cell('H');
    c.fg = Some(fg);
    c.bg = Some(bg);
    c.inverse = true;
    let (fb, pw, cw, ch, _) = render_one(vec![c], p);
    let n_fg = count_color(&fb, pw, cw, ch, 0, fg);
    let n_bg = count_color(&fb, pw, cw, ch, 0, bg);
    let total = (cw * ch) as usize;
    eprintln!("inverse 彩色: 原fg色像素={n_fg} 原bg色像素={n_bg} 总={total}");
    assert!(
        n_fg * 2 > total,
        "反色后底色应为原前景色（{fg:?}），实测 {n_fg}/{total}"
    );
    assert!(n_bg > 0, "反色后字形应为原背景色（{bg:?}），实测 0");
}

/// 3) 选区反色（sel=true）不受影响：选区底色=原 fg、字色=原 bg（既有正确行为）。
#[test]
fn selection_keeps_inverse_style_correct() {
    let p = Palette::default();
    let mut c = cell('H');
    c.inverse = true;
    let mut r =
        CpuRenderer::new(&settings(), RenderOptions::default(), p).expect("CpuRenderer");
    r.set_selection(Some((0, 0, 0, 0)));
    let (cw, ch) = r.cell_metrics(1.0);
    let snap = snap_of(vec![c]);
    let pw = cw * snap.cols as u32;
    r.render(&snap, pw, ch).expect("render");
    let fb = r.framebuffer().to_vec();
    // 选区 + inverse：CPU 的 sel 覆盖后 fg=orig_bg(黑)、bg=orig_fg(白) → 白底黑字
    let white = count_color(&fb, pw, cw, ch, 0, p.primary_fg);
    let black = count_color(&fb, pw, cw, ch, 0, p.primary_bg);
    let total = (cw * ch) as usize;
    eprintln!("选区 + inverse 'H': 白={white} 黑={black} 总={total}");
    assert!(white * 2 > total, "选区反色应为白底（实测白 {white}/{total}）");
    assert!(black > 0, "选区反色应有黑字");
}

/// 4) 光标格字符 + inverse：光标块覆盖后按 cursor_text 重画，字符应清晰可见。
#[test]
fn cursor_block_on_inverse_cell_keeps_glyph_visible() {
    let p = Palette::default();
    let mut c = cell('H');
    c.inverse = true;
    let mut snap = snap_of(vec![c]);
    snap.cursor.visible = true;
    snap.cursor.style = CursorStyle::Block;
    snap.cursor.x = 0;
    snap.cursor.y = 0;
    let mut r = CpuRenderer::new(&settings(), RenderOptions::default(), p).expect("CpuRenderer");
    let (cw, ch) = r.cell_metrics(1.0);
    let pw = cw * snap.cols as u32;
    r.render(&snap, pw, ch).expect("render");
    let fb = r.framebuffer().to_vec();
    // 光标块 = palette.cursor（默认黑），字符用 cursor_text（默认白）重画
    let white = count_color(&fb, pw, cw, ch, 0, p.cursor_text);
    eprintln!("光标 + inverse 'H': cursor_text 像素={white}");
    assert!(white > 0, "光标块上应能用 cursor_text 色看到字符");
}

/// 5) GPU 侧同样的绝对断言（无适配器显式 SKIP 并计数）。
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn inverse_cell_gpu_absolute_pixels() {
    use render::gpu::GpuRenderer;
    use render::Error;

    let p = Palette::default();
    let mut gpu = match GpuRenderer::new_offscreen(
        &settings(),
        RenderOptions::default(),
        p,
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
    let mut c = cell('H');
    c.inverse = true;
    let snap = snap_of(vec![c]);
    let pw = cw * snap.cols as u32;
    gpu.render(&snap, pw, ch).expect("gpu render");
    let rgba = gpu.framebuffer_rgba().to_vec();
    let (mut white, mut black) = (0usize, 0usize);
    for y in 0..ch {
        for x in 0..cw {
            let i = ((y * pw + x) * 4) as usize;
            let px = (u32::from(rgba[i]) << 16) | (u32::from(rgba[i + 1]) << 8) | u32::from(rgba[i + 2]);
            if eq_rgb(px, p.primary_fg) {
                white += 1;
            } else if eq_rgb(px, p.primary_bg) {
                black += 1;
            }
        }
    }
    let total = (cw * ch) as usize;
    eprintln!("GPU inverse 'H': 白={white} 黑={black} 总={total}");
    assert!(
        white * 2 > total,
        "GPU 反色格底色应主要为原前景色，实测 {white}/{total}"
    );
    assert!(black > 0, "GPU 反色格应有字形像素");
}
