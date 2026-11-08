//! render：CpuRenderer 合成快照像素断言。
//! 系统无可用字体时明确跳过并说明原因。

use render::{CpuRenderer, Error, Renderer};
use vt::{Cell, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline, CellWide};

/// 合成快照：一行两列，左格空（背景格），右格 'X' 红色前景。
fn synth_snapshot() -> FrameSnapshot {
    FrameSnapshot {
        cols: 2,
        rows: 1,
        cells: vec![vec![
            Cell::default(),
            Cell {
                chars: "X".to_string(),
                fg: Some(Rgb { r: 255, g: 0, b: 0 }),
                bg: Some(Rgb { r: 0, g: 0, b: 0 }),
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
            },
        ]],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: true,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
    }
}

#[test]
fn render_char_cell_differs_from_bg() {
    let mut renderer = match CpuRenderer::new(None, None) {
        Ok(r) => r,
        Err(Error::Font(msg)) => {
            println!("skip: 系统无可用字体（{}）", msg);
            return;
        }
        Err(e) => panic!("CpuRenderer::new 失败: {}", e),
    };
    let snap = synth_snapshot();
    renderer.render(&snap, 32, 16).expect("render");
    let fb = renderer.framebuffer();
    assert_eq!(fb.len(), 32 * 16, "帧缓冲像素数应等于尺寸");

    // 背景格（左半 16 列）应全为背景色
    let bg = fb[0];
    let bg_region = &fb[..16 * 16];
    assert!(
        bg_region.iter().all(|&p| p == bg),
        "背景格区域应全为背景色 {:08x}，发现差异像素",
        bg
    );

    // 有字符格（右半 16 列）至少一像素与背景不同
    let char_region = &fb[16 * 16..];
    assert!(
        char_region.iter().any(|&p| p != bg),
        "有字符格应含与背景不同的像素（前景光栅化），实际全为 {:08x}",
        bg
    );
}

#[test]
fn render_default_font_family_and_size() {
    let renderer = CpuRenderer::new(None, None);
    if let Err(Error::Font(msg)) = renderer {
        println!("skip: 系统无可用字体（{}）", msg);
        return;
    }
    let renderer = renderer.expect("CpuRenderer::new");
    assert_eq!(renderer.font_family(), render::DEFAULT_FONT_FAMILY);
    assert_eq!(renderer.font_size(), render::DEFAULT_FONT_SIZE);
}
