//! 阶段 9I：下划线样式（SGR 4:1/2/3/4/5）必须渲染成不同图案。
//!
//! 背景（leantty-zig 的对照清单 T1）：此前 `Curly` 直接按单实线画，点线/虚线用"每 N 像素点一次"，
//! 与 alacritty 的 1 亮 1 灭 / 2 亮 2 灭不一致。本文件锁定四种图案的像素差异。

use render::{
    Antialiasing, CpuRenderer, Error, FontSettings, FontSpec, Palette, RenderOptions, Renderer,
    Subpixel,
};
use vt::{Cell, Cursor, CursorStyle, FrameSnapshot, Underline};

fn settings() -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: "DejaVu Sans Mono".to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 14.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
        synthetic_bold: true,
    }
}

fn opts() -> RenderOptions {
    RenderOptions {
        antialiasing: Antialiasing::Grayscale,
        subpixel: Subpixel::None,
        hinting: false,
        text_gamma: 1.0,
        draw_bold_with_bright: false,
    }
}

/// 单格单字符，只有下划线而字形为空格，便于只观察装饰线。
fn render_with(style: Underline) -> Option<Vec<u32>> {
    let mut r = match CpuRenderer::new(&settings(), opts(), Palette::default()) {
        Ok(r) => r,
        Err(Error::Font(_)) => return None,
        Err(e) => panic!("{e}"),
    };
    let snap = FrameSnapshot {
        cols: 1,
        rows: 1,
        cells: vec![vec![Cell {
            chars: " ".into(),
            underline: style,
            ..Default::default()
        }]],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
        wrapped: vec![false],
        dirty_rows: vec![true],
        full_redraw: true,
    };
    r.render(&snap, 40, 32).expect("render");
    Some(r.framebuffer().to_vec())
}

fn ink_per_row(fb: &[u32], w: u32, h: u32) -> Vec<usize> {
    (0..h)
        .map(|y| (0..w).filter(|x| fb[(y * w + x) as usize] != fb[0]).count())
        .collect()
}

#[test]
fn styles_differ_from_each_other() {
    let Some(single) = render_with(Underline::Single) else {
        return;
    };
    let (w, h) = (40u32, 32u32);
    let rows_single = ink_per_row(&single, w, h);
    let single_rows: Vec<usize> = rows_single
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(y, _)| y)
        .collect();
    // 线宽 = max(cell_h/16, 1)：32 px 格高下为 2 px。
    let thick = (h / 16).max(1) as usize;
    assert_eq!(single_rows.len(), thick, "单实线应占 {thick} 行");

    for (name, style) in [
        ("double", Underline::Double),
        ("curly", Underline::Curly),
        ("dotted", Underline::Dotted),
        ("dashed", Underline::Dashed),
    ] {
        let fb = render_with(style).unwrap_or_else(|| panic!("{name}: 渲染失败"));
        let rows: Vec<usize> = ink_per_row(&fb, w, h)
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(y, _)| y)
            .collect();
        assert_ne!(fb, single, "{name} 的像素必须与单实线不同");
        match style {
            Underline::Double => {
                assert_eq!(rows.len(), thick * 2, "双线应占 {} 行", thick * 2)
            }
            Underline::Curly => assert!(
                rows.len() > thick,
                "波浪线应至少跨 {} 行，实测 {rows:?}",
                thick + 1
            ),
            _ => {}
        }
    }
}

#[test]
fn dotted_and_dashed_have_gaps() {
    let (w, h) = (40u32, 32u32);
    for style in [Underline::Dotted, Underline::Dashed] {
        let fb = render_with(style).expect("render");
        // 找到有墨迹的行，统计该行亮像素数：点线/虚线不能铺满整行。
        let per_row = ink_per_row(&fb, w, h);
        let (row, count) = per_row
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| **c)
            .expect("至少一行");
        assert!(*count > 0, "样式 {style:?} 未画出任何像素");
        assert!(
            *count < w as usize,
            "{style:?} 第 {row} 行应有点/虚间隔，实测铺满 {count}/{w}"
        );
    }
}
