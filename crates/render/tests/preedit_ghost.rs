//! 预编辑残影回归（阶段 4B-fix1 任务 C）。
//!
//! 场景：预编辑叠加层**跨多行**时清除，旧覆盖区域的所有行都必须重绘。
//! 修复前只把起始行标脏，第二行及之后会留下残影。
//!
//! 与 e2e case30 的分工：e2e 通过截图验证"清除后无残影"，但 shell 回显会覆盖
//! 预编辑所在行，无法单独证伪跨行标脏；本测试在渲染层直接对拍（无 shell 干扰）。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, PreeditOverlay, RenderOptions, Renderer};
use vt::{Cursor, CursorStyle, FrameSnapshot};

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

/// 空网格快照（rows 行的空白），光标在首行。
fn blank_frame(cols: u16, rows: u16) -> FrameSnapshot {
    let cell = || vt::Cell {
        chars: " ".to_string(),
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
        underline: vt::Underline::None,
        wide: vt::CellWide::Narrow,
    };
    FrameSnapshot {
        cols,
        rows,
        cells: (0..rows).map(|_| (0..cols).map(|_| cell()).collect()).collect(),
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
        wrapped: Vec::new(),
        dirty_rows: vec![false; rows as usize],
        full_redraw: false,
    }
}

fn new_renderer() -> CpuRenderer {
    CpuRenderer::new(&settings(), RenderOptions::default(), Palette::default()).expect("CpuRenderer")
}

#[test]
fn preedit_clear_repaints_all_spanned_rows() {
    let (cols, rows) = (80u16, 10u16);
    let mut snap = blank_frame(cols, rows);
    snap.full_redraw = true;
    for d in snap.dirty_rows.iter_mut() {
        *d = true;
    }

    let mut r = new_renderer();
    let (cw, ch) = r.cell_metrics(1.0);
    let (pw, ph) = (u32::from(cols) * cw, u32::from(rows) * ch);

    // 参考帧：从未设置过预编辑。
    r.render(&snap, pw, ph).expect("render blank");
    let reference = r.framebuffer().to_vec();

    // 注入跨行预编辑（120 个半角字符 > 80 列 → 至少跨 2 行）。
    let long: String = "W".repeat(120);
    r.set_preedit(Some(PreeditOverlay::new(long.clone(), None)));
    let mut snap2 = snap.clone();
    snap2.full_redraw = false; // 模拟静止画面：仅叠加层变化
    r.render(&snap2, pw, ph).expect("render preedit");
    let with_preedit = r.framebuffer().to_vec();
    let changed: usize = reference
        .iter()
        .zip(with_preedit.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 0, "预编辑叠加层应改变画面（实际 0 像素）");
    eprintln!(
        "预编辑({} 字符)覆盖 {changed} 像素；cell={cw}x{ch} pixel={pw}x{ph}",
        long.chars().count()
    );

    // 清除预编辑：只允许"叠加层脏"，此时必须把**所有被覆盖的行**都重绘。
    r.set_preedit(None);
    r.render(&snap2, pw, ph).expect("render cleared");
    let after_clear = r.framebuffer().to_vec();

    let residual: Vec<usize> = reference
        .iter()
        .zip(after_clear.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    if !residual.is_empty() {
        let pw_usize = pw as usize;
        let rows_hit: Vec<usize> = {
            let mut v: Vec<usize> = residual.iter().map(|i| i / pw_usize / ch as usize).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        eprintln!(
            "清除后残留 {} 像素，涉及行 {:?}（应为空）",
            residual.len(),
            rows_hit
        );
    }
    assert!(
        residual.is_empty(),
        "清除预编辑后仍有 {} 像素残影（跨行未重绘）",
        residual.len()
    );
}

/// 预编辑**位置移动**（文本变化导致起始列/行变化）时，旧位置也必须清干净。
#[test]
fn preedit_move_clears_old_position() {
    let (cols, rows) = (40u16, 5u16);
    let mut snap = blank_frame(cols, rows);
    snap.full_redraw = true;
    for d in snap.dirty_rows.iter_mut() {
        *d = true;
    }
    let mut r = new_renderer();
    let (cw, ch) = r.cell_metrics(1.0);
    let (pw, ph) = (u32::from(cols) * cw, u32::from(rows) * ch);
    r.render(&snap, pw, ph).expect("blank");
    let reference = r.framebuffer().to_vec();

    // 先在光标 (0,0) 处放一段，再把光标移到 (5,2) 放另一段（模拟位置移动）。
    r.set_preedit(Some(PreeditOverlay::new("AAAA".to_string(), None)));
    r.render(&snap, pw, ph).expect("preedit at 0,0");
    let mut moved = snap.clone();
    moved.cursor.x = 5;
    moved.cursor.y = 2;
    r.set_preedit(Some(PreeditOverlay::new("BBBB".to_string(), None)));
    r.render(&moved, pw, ph).expect("preedit moved");
    // 再清除：两处旧位置都必须回到背景。
    r.set_preedit(None);
    r.render(&moved, pw, ph).expect("cleared");

    let mut snap_no_cursor = moved.clone();
    snap_no_cursor.full_redraw = true;
    let mut fresh = new_renderer();
    fresh.render(&snap_no_cursor, pw, ph).expect("fresh");
    let residual: usize = fresh
        .framebuffer()
        .iter()
        .zip(r.framebuffer().iter())
        .filter(|(a, b)| a != b)
        .count();
    let _ = reference;
    eprintln!("移动+清除后与全新渲染的差异像素：{residual}");
    assert_eq!(residual, 0, "预编辑移动后清除仍有残留");
}
