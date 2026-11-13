//! render：字体回退链与宽字符对齐（系统 DejaVu Sans Mono + CJK 字体，缺则跳过）。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::{Cell, CellWide, Cursor, FrameSnapshot};

fn settings(fallback_family: &str) -> FontSettings {
    let spec = |family: &str, style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 14.0,
        normal: spec("DejaVu Sans Mono", "Regular"),
        bold: spec("DejaVu Sans Mono", "Bold"),
        italic: spec("DejaVu Sans Mono", "Italic"),
        bold_italic: spec("DejaVu Sans Mono", "Bold Italic"),
        fallback: vec![spec(fallback_family, "Regular")],
    }
}

fn opts() -> RenderOptions {
    RenderOptions::default()
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
            style: vt::CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
        wrapped: Vec::new(),
        dirty_rows: vec![true; 1],
        full_redraw: true,
    }
}

fn mk_cell(ch: &str, wide: CellWide) -> Cell {
    Cell {
        chars: ch.to_string(),
        wide,
        ..Default::default()
    }
}

#[test]
fn fallback_chain_ascii_main_cjk_fallback() {
    let mut renderer = match CpuRenderer::new(&settings("黑体"), opts(), Palette::default()) {
        Ok(r) => r,
        Err(e) => {
            println!("skip: 系统字体不可用（{}）", e);
            return;
        }
    };
    // ASCII 格用主字体（组内索引 0）；汉字走回退（索引 > 0）。
    // 懒加载：后台字体未到位时汉字索引 0（占位语义）；add_background_fonts 到位后断言 > 0。
    let snap = snap_of(vec![mk_cell("A", CellWide::Narrow), mk_cell("你", CellWide::Narrow)]);
    let before = renderer.font_indices(&snap);
    assert_eq!(before[0][0], 0, "ASCII 'A' 应用主字体（索引 0）");
    let fonts = render::load_font_list(&settings("黑体").fallback);
    if fonts.is_empty() {
        println!("skip: 系统无 CJK/兜底字体");
        return;
    }
    renderer.add_background_fonts(fonts);
    let after = renderer.font_indices(&snap);
    assert_eq!(after[0][0], 0, "ASCII 'A' 仍用主字体");
    assert!(
        after[0][1] > 0,
        "汉字 '你' 后台字体到位后应走回退链（索引 > 0），实测 {}",
        after[0][1]
    );
}

#[test]
fn wide_char_spans_two_cells_and_tail_not_drawn() {
    let mut renderer = match CpuRenderer::new(&settings("黑体"), opts(), Palette::default()) {
        Ok(r) => r,
        Err(e) => {
            println!("skip: 系统字体不可用（{}）", e);
            return;
        }
    };
    // 宽字符格 + SpacerTail（违规带字符，验证不绘制）+ 独立 'A' 格。
    let snap = snap_of(vec![
        mk_cell("你", CellWide::Wide),
        mk_cell("X", CellWide::SpacerTail),
        mk_cell("A", CellWide::Narrow),
    ]);
    renderer.render(&snap, 48, 24).expect("render");
    let fb = renderer.framebuffer();
    let w = 16u32;
    let h = 24u32;
    let bg = fb[0];
    let cell_has_fg = |x0: u32| {
        (x0..x0 + w).any(|x| (0..h).any(|y| {
            let p = fb[(y * 48 + x) as usize];
            p != bg
        }))
    };
    assert!(cell_has_fg(0), "宽字符格应有前景像素");
    assert!(
        (0..2 * w).any(|x| (0..h).any(|y| fb[(y * 48 + x) as usize] != bg)),
        "宽字符区域（2 格合并）应有前景像素"
    );
    assert!(cell_has_fg(2 * w), "'A' 格应有前景像素");
    // SpacerTail 格不单独绘制：清空其 chars 后重渲染，两帧应完全一致。
    let mut snap2 = snap.clone();
    snap2.cells[0][1].chars.clear();
    let mut renderer2 = match CpuRenderer::new(&settings("黑体"), opts(), Palette::default()) {
        Ok(r) => r,
        Err(e) => {
            println!("skip: 系统字体不可用（{}）", e);
            return;
        }
    };
    renderer2.render(&snap2, 48, 24).expect("render2");
    assert_eq!(
        renderer.framebuffer(),
        renderer2.framebuffer(),
        "SpacerTail 格字符不应被绘制（对照帧应一致）"
    );
}

#[test]
fn style_fallback_bold_italic_logs_not_panic() {
    // Bold Italic 缺失时回落 Bold→Italic→Regular（日志在 stderr，断言不 panic 即通过）。
    let s = FontSettings {
        size: 14.0,
        normal: FontSpec {
            family: "DejaVu Sans Mono".to_string(),
            style: "Regular".to_string(),
        },
        bold: FontSpec {
            family: "DejaVu Sans Mono".to_string(),
            style: "Bold".to_string(),
        },
        italic: FontSpec {
            family: "DejaVu Sans Mono".to_string(),
            style: "Italic".to_string(),
        },
        bold_italic: FontSpec {
            family: "DejaVu Sans Mono".to_string(),
            style: "Bold Italic".to_string(),
        },
        fallback: Vec::new(),
    };
    let renderer = CpuRenderer::new(&s, opts(), Palette::default());
    assert!(renderer.is_ok(), "Bold Italic 回落不应失败");
}
