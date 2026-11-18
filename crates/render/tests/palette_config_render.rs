//! 配置调色板 → 渲染像素（阶段 4F t2）。
//!
//! VT 层测试（`crates/vt/tests/palette_config.rs`）验证快照里的颜色；这里验证
//! **最终像素**：默认色反色块（ESC[7m）必须是"配置前景色作底、配置背景色作字"，
//! 而不是 libghostty 内置默认色（#C5C8C6 一类）。

use render::{CpuRenderer, FontSettings, FontSpec, RenderOptions, Renderer};
use vt::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline, VtPalette};

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

/// 与 VT 测试同一份"每项都不同"的调色板，转成 render 的 Palette。
fn palette() -> (VtPalette, render::Palette) {
    let c = |r: u8, g: u8, b: u8| Rgb { r, g, b };
    let normal = [
        c(0x01, 0x02, 0x03),
        c(0x11, 0x12, 0x13),
        c(0x21, 0x22, 0x23),
        c(0x31, 0x32, 0x33),
        c(0x41, 0x42, 0x43),
        c(0x51, 0x52, 0x53),
        c(0x61, 0x62, 0x63),
        c(0x71, 0x72, 0x73),
    ];
    let bright = [
        c(0x81, 0x82, 0x83),
        c(0x91, 0x92, 0x93),
        c(0xA1, 0xA2, 0xA3),
        c(0xB1, 0xB2, 0xB3),
        c(0xC1, 0xC2, 0xC3),
        c(0xD1, 0xD2, 0xD3),
        c(0xE4, 0xE5, 0xE6),
        c(0xF1, 0xF2, 0xF3),
    ];
    let vt = VtPalette {
        foreground: c(0xE1, 0xE2, 0xE3),
        background: c(0x10, 0x11, 0x12),
        cursor: c(0x20, 0x21, 0x22),
        normal,
        bright,
    };
    let rp = render::Palette {
        primary_bg: vt.background,
        primary_fg: vt.foreground,
        cursor_text: vt.foreground,
        cursor: vt.cursor,
        normal,
        bright,
    };
    (vt, rp)
}

fn render_frame(snap: &FrameSnapshot, rp: render::Palette) -> (Vec<u32>, u32, u32, u32, u32) {
    let mut r = CpuRenderer::new(&settings(), RenderOptions::default(), rp).expect("CpuRenderer");
    let (cw, ch) = r.cell_metrics(1.0);
    let pw = cw * snap.cols as u32;
    r.render(snap, pw, ch).expect("render");
    (r.framebuffer().to_vec(), pw, cw, ch, snap.cols as u32)
}

fn eq_rgb(px: u32, c: Rgb) -> bool {
    ((px >> 16) & 0xFF) == u32::from(c.r)
        && ((px >> 8) & 0xFF) == u32::from(c.g)
        && (px & 0xFF) == u32::from(c.b)
}

fn count(fb: &[u32], pw: u32, cw: u32, ch: u32, idx: u32, c: Rgb) -> usize {
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

fn one_cell(inverse: bool, fg: Option<Rgb>, bg: Option<Rgb>) -> FrameSnapshot {
    FrameSnapshot {
        cols: 1,
        rows: 1,
        cells: vec![vec![Cell {
            chars: "H".to_string(),
            fg,
            bg,
            bold: false,
            italic: false,
            faint: false,
            blink: false,
            inverse,
            invisible: false,
            strikethrough: false,
            overline: false,
            underline: Underline::None,
            wide: CellWide::Narrow,
        }]],
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

/// 默认色反色格：底色 = 配置 foreground，字形 = 配置 background。
#[test]
fn inverse_default_cell_uses_configured_colors() {
    let (vt, rp) = palette();
    let (fb, pw, cw, ch, _) = render_frame(&one_cell(true, None, None), rp);
    let base = count(&fb, pw, cw, ch, 0, vt.foreground);
    let glyph = count(&fb, pw, cw, ch, 0, vt.background);
    let total = (cw * ch) as usize;
    eprintln!("默认反色格: 配置前景色像素={base} 配置背景色像素={glyph} 总={total}");
    assert!(
        base * 2 > total,
        "反色格底色应为配置 foreground（实测 {base}/{total}）"
    );
    assert!(glyph > 0, "反色格字形应为配置 background（实测 0）");
}

/// ESC[31m 类：显式索引色（cell.fg = normal[1]）应画成配置色。
#[test]
fn indexed_fg_cell_uses_configured_palette_color() {
    let (vt, rp) = palette();
    let (fb, pw, cw, ch, _) = render_frame(&one_cell(false, Some(vt.normal[1]), None), rp);
    let n = count(&fb, pw, cw, ch, 0, vt.normal[1]);
    eprintln!("索引红字: 配置色像素={n}");
    assert!(n > 0, "显式索引前景应画出配置色");
}

/// draw_bold_text_with_bright_colors 语义（阶段 4F t2d）：
/// 开启时 bold + normal 0..7 → bright 8..15；关闭时保持原色。
#[test]
fn bold_with_bright_maps_normal_to_bright_only_when_enabled() {
    let (vt, rp) = palette();
    // 构造一个 bold + 普通红（normal[1]）的格
    let mut snap = one_cell(false, Some(vt.normal[1]), None);
    snap.cells[0][0].bold = true;

    let opts_on = RenderOptions {
        draw_bold_with_bright: true,
        ..Default::default()
    };
    let mut r_on = CpuRenderer::new(&settings(), opts_on, rp).expect("r_on");
    let (cw, ch) = r_on.cell_metrics(1.0);
    r_on.render(&snap, cw, ch).expect("render on");
    let fb_on = r_on.framebuffer().to_vec();
    let bright = count(&fb_on, cw, cw, ch, 0, vt.bright[1]);
    eprintln!("bold+bright 开启: bright[1] 像素={bright}");
    assert!(bright > 0, "开启时应映射为 bright[1]");

    let opts_off = RenderOptions {
        draw_bold_with_bright: false,
        ..Default::default()
    };
    let mut r_off = CpuRenderer::new(&settings(), opts_off, rp).expect("r_off");
    r_off.render(&snap, cw, ch).expect("render off");
    let fb_off = r_off.framebuffer().to_vec();
    let normal = count(&fb_off, cw, cw, ch, 0, vt.normal[1]);
    let bright_off = count(&fb_off, cw, cw, ch, 0, vt.bright[1]);
    eprintln!("bold+bright 关闭: normal[1] 像素={normal} bright[1] 像素={bright_off}");
    assert!(normal > 0, "关闭时应保持 normal[1]");
    assert_eq!(bright_off, 0, "关闭时不得出现 bright[1]");
}
