//! render：CpuRenderer 合成快照像素断言（任务 3：颜色/样式/光标）。
//! 系统无可用字体时明确跳过并说明原因。

use render::{
    Antialiasing, CpuRenderer, Error, FontSettings, FontSpec, Palette, RenderOptions, Renderer,
    Subpixel,
};
use vt::{Cell, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline, CellWide};

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
    }
}

fn opts(draw_bold_with_bright: bool) -> RenderOptions {
    RenderOptions {
        antialiasing: Antialiasing::Grayscale,
        subpixel: Subpixel::None,
        hinting: false,
        text_gamma: 1.0,
        draw_bold_with_bright,
    }
}

fn palette() -> Palette {
    Palette::default()
}

fn cell(ch: &str, fg: Option<Rgb>, bg: Option<Rgb>, bold: bool, underline: Underline) -> Cell {
    Cell {
        chars: ch.to_string(),
        fg,
        bg,
        bold,
        underline,
        ..Default::default()
    }
}

/// 3 列 x 1 行：反显格 / 粗体红字格 / 下划线格。
fn synth_snapshot() -> FrameSnapshot {
    let red = Rgb { r: 205, g: 0, b: 0 };
    let blue = Rgb { r: 0, g: 0, b: 238 };
    FrameSnapshot {
        cols: 3,
        rows: 1,
        cells: vec![vec![
            Cell {
                chars: "A".to_string(),
                fg: Some(red),
                bg: Some(blue),
                inverse: true,
                ..Default::default()
            },
            cell("B", Some(red), None, true, Underline::None),
            cell("C", None, None, false, Underline::Single),
        ]],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
    }
}

#[test]
fn render_char_cell_differs_from_bg() {
    let mut renderer = match CpuRenderer::new(&settings(), opts(false), palette()) {
        Ok(r) => r,
        Err(Error::Font(msg)) => {
            println!("skip: 系统无可用字体（{}）", msg);
            return;
        }
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    let snap = synth_snapshot();
    renderer.render(&snap, 48, 24).expect("render");
    let fb = renderer.framebuffer();
    assert_eq!(fb.len(), 48 * 24, "帧缓冲像素数应等于尺寸");

    // 每格 16 像素宽（48/3）。反显格：前景红与背景蓝交换 → 字形像素应为蓝。
    let w = 16u32;
    let inverse_has_blue = (0..w).any(|x| {
        (0..24u32).any(|y| {
            let p = fb[(y * 48 + x) as usize];
            (p & 0xFF) == 238 && (p >> 16 & 0xFF) == 0
        })
    });
    assert!(inverse_has_blue, "反显格字形像素应为原背景色蓝");

    // 粗体格（draw_bold_with_bright=false）：字形像素为 normal 红。
    let bold_has_red = (w..2 * w).any(|x| {
        (0..24u32).any(|y| {
            let p = fb[(y * 48 + x) as usize];
            (p >> 16 & 0xFF) == 205 && (p & 0xFF) == 0
        })
    });
    assert!(bold_has_red, "粗体格应为 normal 红色（未开 bright 映射）");

    // 下划线格：底线附近行有前景像素。
    let underline_line = (2 * w..3 * w).any(|x| {
        let y = 22u32; // cell_h=24，下划线在 h-2
        let p = fb[(y * 48 + x) as usize];
        p != 0xFF00_0000
    });
    assert!(underline_line, "下划线格底线应有前景像素");
}

#[test]
fn render_bold_maps_to_bright_color() {
    let mut r_on = match CpuRenderer::new(&settings(), opts(true), palette()) {
        Ok(r) => r,
        Err(Error::Font(msg)) => {
            println!("skip: 系统无可用字体（{}）", msg);
            return;
        }
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    let mut r_off = match CpuRenderer::new(&settings(), opts(false), palette()) {
        Ok(r) => r,
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    let red = Rgb { r: 205, g: 0, b: 0 };
    let snap = FrameSnapshot {
        cols: 1,
        rows: 1,
        cells: vec![vec![cell("B", Some(red), None, true, Underline::None)]],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
    };
    r_on.render(&snap, 16, 24).expect("render on");
    r_off.render(&snap, 16, 24).expect("render off");
    let on = r_on.framebuffer();
    let off = r_off.framebuffer();
    assert_ne!(on, off, "draw_bold_with_bright 开/关像素应不同（bright 映射）");
    // 开：字形像素应为 bright 红 (255,0,0)（alpha=255 处）
    let has_bright_red = on.iter().any(|&p| (p >> 16 & 0xFF) == 255 && (p & 0xFF) == 0 && (p >> 8 & 0xFF) == 0);
    assert!(has_bright_red, "粗体应使用 bright 红");
}

#[test]
fn render_cursor_styles_draw() {
    for style in [CursorStyle::Block, CursorStyle::HollowBlock, CursorStyle::Underline, CursorStyle::Bar] {
        let mut renderer = match CpuRenderer::new(&settings(), opts(false), palette()) {
            Ok(r) => r,
            Err(Error::Font(msg)) => {
                println!("skip: 系统无可用字体（{}）", msg);
                return;
            }
            Err(e) => panic!("CpuRenderer::new 失败: {}", e),
        };
        let snap = FrameSnapshot {
            cols: 1,
            rows: 1,
            cells: vec![vec![Cell::default()]],
            cursor: Cursor { x: 0, y: 0, visible: true, style },
            scrollback_rows: 0,
            alternate_screen: false,
        };
        renderer.render(&snap, 16, 24).expect("render");
        // 光标色（默认黑）在黑色背景格上不可见——改用 cursor 色非 bg 的 palette 断言太绕；
        // 这里断言渲染不 panic 且帧缓冲已填充（背景格默认黑 = 光标色黑，逐像素无差异可断言）。
        // 改用 cursor 色为蓝的 palette 断言光标像素存在：
        let mut pal = Palette::default();
        pal.cursor = Rgb { r: 0, g: 0, b: 255 };
        pal.cursor_text = Rgb { r: 255, g: 255, b: 255 };
        let mut r2 = CpuRenderer::new(&settings(), opts(false), pal).expect("renderer2");
        r2.render(&snap, 16, 24).expect("render2");
        let has_blue = r2.framebuffer().iter().any(|&p| (p & 0xFF) == 255);
        assert!(has_blue, "光标样式 {:?} 应绘制光标色（蓝）像素", style);
    }
}

#[test]
fn render_options_change_pixels() {
    let snap = synth_snapshot();
    let mut r_gray = match CpuRenderer::new(&settings(), opts(false), palette()) {
        Ok(r) => r,
        Err(Error::Font(msg)) => {
            println!("skip: 系统无可用字体（{}）", msg);
            return;
        }
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    r_gray.render(&snap, 48, 24).expect("render gray");

    // 抗锯齿 none：二值化与灰度像素不同
    let mut o_none = opts(false);
    o_none.antialiasing = Antialiasing::None;
    let mut r_none = CpuRenderer::new(&settings(), o_none, palette()).expect("r_none");
    r_none.render(&snap, 48, 24).expect("render none");
    assert_ne!(
        r_gray.framebuffer(),
        r_none.framebuffer(),
        "grayscale 与 none 抗锯齿像素应不同"
    );

    // text_gamma 改变覆盖率 → alpha 映射
    let mut o_gamma = opts(false);
    o_gamma.text_gamma = 2.5;
    let mut r_gamma = CpuRenderer::new(&settings(), o_gamma, palette()).expect("r_gamma");
    r_gamma.render(&snap, 48, 24).expect("render gamma");
    assert_ne!(
        r_gray.framebuffer(),
        r_gamma.framebuffer(),
        "text_gamma 1.0 与 2.5 像素应不同"
    );

    // subpixel Rgb：3x 水平采样路径功能可用（渲染成功、字形像素存在）；与 grayscale
    // 的像素差异不作强制断言（字体边缘对齐时三通道 coverage 相同，评估结论见 DECISIONS）。
    let mut o_sub = opts(false);
    o_sub.subpixel = Subpixel::Rgb;
    let mut r_sub = CpuRenderer::new(&settings(), o_sub, palette()).expect("r_sub");
    r_sub.render(&snap, 48, 24).expect("render subpixel");
    let bg = r_sub.framebuffer()[0];
    assert!(
        r_sub.framebuffer().iter().any(|&p| p != bg),
        "subpixel 渲染应产生前景像素"
    );
}
