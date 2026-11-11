#![cfg(unix)]
//! 后台懒加载序列单测（任务 2B.1）：请求 → U+25A1 占位 → 到位 → 重绘替换。
//! 系统字体缺失时明确跳过并说明原因。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::{Cell, CellWide, Cursor, FrameSnapshot};

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
        fallback: vec![FontSpec {
            family: "黑体".to_string(),
            style: "Regular".to_string(),
        }],
    }
}

fn opts() -> RenderOptions {
    RenderOptions::default()
}

fn snap(text: &str) -> FrameSnapshot {
    FrameSnapshot {
        cols: 1,
        rows: 1,
        cells: vec![vec![Cell {
            chars: text.to_string(),
            wide: if text.chars().count() == 1 && text.chars().next().unwrap().is_ascii() {
                CellWide::Narrow
            } else {
                CellWide::Wide
            },
            ..Default::default()
        }]],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: vt::CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
    }
}

#[test]
fn background_load_sequence_placeholder_then_glyph() {
    let mut r = match CpuRenderer::new(&settings(), opts(), Palette::default()) {
        Ok(r) => r,
        Err(render::Error::Font(msg)) => {
            println!("skip: 系统字体不可用（{}）", msg);
            return;
        }
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    // 阶段1（占位）：后台字体未到位 → 汉字格应画 U+25A1（与直接渲染 U+25A1 帧逐像素一致）。
    r.render(&snap("你"), 32, 32).expect("render 占位");
    let fb_ph = r.framebuffer().to_vec();
    r.render(&snap('\u{25A1}'.to_string().as_str()), 32, 32).expect("render 方块");
    let fb_box = r.framebuffer().to_vec();
    assert_eq!(fb_ph, fb_box, "后台字体未到位时汉字格应画 U+25A1 占位方块");

    // 阶段2（到位→重绘替换）：add_background_fonts 后重绘应不同于占位帧。
    let fonts = render::load_font_list(&settings().fallback);
    assert!(!fonts.is_empty(), "系统应有 CJK/兜底字体（home-pc 实测 Noto CJK）");
    r.add_background_fonts(fonts);
    r.render(&snap("你"), 32, 32).expect("render 到位");
    let fb_real = r.framebuffer().to_vec();
    assert_ne!(fb_ph, fb_real, "后台字体到位后重绘应替换占位为真实字形");

    // 阶段3：ASCII 格走主字体，不受后台字体影响。
    r.render(&snap("A"), 32, 32).expect("render ascii");
    let fb_ascii = r.framebuffer().to_vec();
    assert_ne!(fb_real, fb_ascii, "ASCII 格应仍走主字体（与汉字帧不同）");
}

#[test]
fn add_empty_background_fonts_is_noop() {
    let mut r = match CpuRenderer::new(&settings(), opts(), Palette::default()) {
        Ok(r) => r,
        Err(render::Error::Font(msg)) => {
            println!("skip: 系统字体不可用（{}）", msg);
            return;
        }
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    // 失败路径：空列表不 panic，占位状态不变。
    r.add_background_fonts(Vec::new());
    r.render(&snap("你"), 32, 32).expect("render");
    let fb_ph = r.framebuffer().to_vec();
    r.render(&snap('\u{25A1}'.to_string().as_str()), 32, 32).expect("render 方块");
    assert_eq!(fb_ph, r.framebuffer().to_vec(), "空列表并入后占位状态应不变");
}
