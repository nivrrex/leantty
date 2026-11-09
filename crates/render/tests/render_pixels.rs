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

    // 背景格（左格）与有字符格（右格）逐像素断言：按 (row*width+col) 索引，
    // 替代连续切片 fb[..16*16]（行主序宽帧缓冲上连续切片实为前 8 整行，含右格字形像素）。
    let width = 32u32;
    let left_col_end = 16u32; // 左格列范围 [0,16)，右格 [16,32)
    let bg = fb[0];
    let mut bg_cell_all_bg = true;
    let mut char_cell_has_fg = false;
    for row in 0..16u32 {
        for col in 0..width {
            let px = fb[(row * width + col) as usize];
            if col < left_col_end {
                if px != bg {
                    bg_cell_all_bg = false;
                }
            } else if px != bg {
                char_cell_has_fg = true;
            }
        }
    }
    assert!(
        bg_cell_all_bg,
        "背景格（左格）内像素应全为背景色 {:08x}，发现差异像素",
        bg
    );
    assert!(
        char_cell_has_fg,
        "有字符格（右格）内至少应有一个与背景不同的像素（前景光栅化）"
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
