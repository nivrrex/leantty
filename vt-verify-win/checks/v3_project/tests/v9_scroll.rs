//! W7：V9 滚动语义探针 a~e。Linux 段 REPORT.md 将 V9 列为"下一轮任务"，
//! 未给出 a~e 定义；此处按滚动语义的常规维度自行定义探针，只记录实测行为，不做预设断言。
//! 注：RenderState 的 viewport_y/viewport_row 是行/图片级 API，非顶层滚动偏移，
//! 故滚动行为以"视口首末可见行文本 + scrollback_rows"记录。

use libghostty_vt::{RenderState, Terminal};
use libghostty_vt::render::{CellIterator, RowIterator};

fn row_texts(term: &Terminal<'_, '_>) -> Vec<String> {
    let mut render_state = RenderState::new().expect("RenderState::new");
    let mut rows = RowIterator::new().expect("RowIterator::new");
    let mut cells = CellIterator::new().expect("CellIterator::new");
    let snapshot = render_state.update(term).expect("RenderState::update");
    let mut row_iter = rows.update(&snapshot).expect("RowIterator::update");
    let mut out = Vec::new();
    while let Some(row) = row_iter.next() {
        let mut cell_iter = cells.update(row).expect("CellIterator::update");
        let mut line = String::new();
        while let Some(cell) = cell_iter.next() {
            for ch in cell.graphemes().expect("graphemes") {
                line.push(ch);
            }
        }
        out.push(line.trim_end().to_string());
    }
    out
}

fn feed_lines(term: &mut Terminal<'_, '_>, n: usize) {
    for i in 1..=n {
        term.vt_write(format!("line {:02}\r\n", i).as_bytes());
    }
}

#[test]
fn v9_scroll_semantics_probe() {
    let log_path = "C:\\vt\\logs\\w7-v9-scroll.log";
    let mut log = std::fs::File::create(log_path).expect("create log");
    let t0 = std::time::Instant::now();
    let mut note = |m: &str| {
        use std::io::Write;
        let _ = writeln!(log, "[{:9.3}] {}", t0.elapsed().as_secs_f64(), m);
        println!("[{:9.3}] {}", t0.elapsed().as_secs_f64(), m);
    };

    let mut term = Terminal::new(80, 24).expect("Terminal::new");

    // a. 60 行输出到 24 行终端
    feed_lines(&mut term, 60);
    let rows = row_texts(&term);
    note(&format!(
        "a) 60 行输出后: scrollback_rows={:?}, 视口行数={}, 首行={:?}, 末行={:?}",
        term.scrollback_rows(),
        rows.len(),
        rows.first(),
        rows.last(),
    ));

    // b. Delta(5)：首末可见行与 scrollback 变化
    term.scroll_viewport(libghostty_vt::terminal::ScrollViewport::Delta(5));
    let rows = row_texts(&term);
    note(&format!(
        "b) Delta(5) 后: scrollback_rows={:?}, 首行={:?}, 末行={:?}",
        term.scrollback_rows(),
        rows.first(),
        rows.last(),
    ));

    // c. Top / Bottom
    term.scroll_viewport(libghostty_vt::terminal::ScrollViewport::Top);
    let rows = row_texts(&term);
    note(&format!(
        "c) Top 后: scrollback_rows={:?}, 首行={:?}, 末行={:?}",
        term.scrollback_rows(),
        rows.first(),
        rows.last(),
    ));
    term.scroll_viewport(libghostty_vt::terminal::ScrollViewport::Bottom);
    let rows = row_texts(&term);
    note(&format!(
        "c) Bottom 后: scrollback_rows={:?}, 首行={:?}, 末行={:?}",
        term.scrollback_rows(),
        rows.first(),
        rows.last(),
    ));

    // d. Row(3) 绝对行
    term.scroll_viewport(libghostty_vt::terminal::ScrollViewport::Row(3));
    let rows = row_texts(&term);
    note(&format!(
        "d) Row(3) 后: scrollback_rows={:?}, 首行={:?}, 末行={:?}",
        term.scrollback_rows(),
        rows.first(),
        rows.last(),
    ));
    term.scroll_viewport(libghostty_vt::terminal::ScrollViewport::Bottom);

    // e. alternate screen 下的滚动行为
    term.vt_write(b"\x1b[?1049h");
    note(&format!(
        "e) alt 屏: scrollback_rows={:?}, 首行={:?}",
        term.scrollback_rows(),
        row_texts(&term).first(),
    ));
    term.scroll_viewport(libghostty_vt::terminal::ScrollViewport::Delta(3));
    note(&format!(
        "e) alt 屏 Delta(3) 后: scrollback_rows={:?}, 首行={:?}",
        term.scrollback_rows(),
        row_texts(&term).first(),
    ));
    term.vt_write(b"\x1b[?1049l");
    note(&format!(
        "e) 回主屏后: scrollback_rows={:?}, 首行={:?}",
        term.scrollback_rows(),
        row_texts(&term).first(),
    ));
}
