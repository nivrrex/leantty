//! V8：从 Terminal 拷贝一帧为自有 owned 结构（行/单元格/颜色/光标），
//! 继续写入并清屏后断言先前快照内容不变且无悬垂引用；记录快照耗时中位数。

use std::hint::black_box;
use std::time::Instant;

use libghostty_vt::{RenderState, Terminal};
use libghostty_vt::render::{CellIterator, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::RgbColor;

#[derive(Clone, Debug, PartialEq, Eq)]
struct OwnedCell {
    chars: String,
    fg: Option<RgbColor>,
    wide: CellWide,
}

/// 拷贝一帧为完全自有、不借用 Terminal 生命周期的结构。
fn snapshot_grid(term: &Terminal<'_, '_>) -> Vec<Vec<OwnedCell>> {
    let mut render_state = RenderState::new().expect("RenderState::new");
    let mut rows = RowIterator::new().expect("RowIterator::new");
    let mut cells = CellIterator::new().expect("CellIterator::new");
    let snapshot = render_state.update(term).expect("RenderState::update");
    let mut row_iter = rows.update(&snapshot).expect("RowIterator::update");
    let mut out = Vec::new();
    while let Some(row) = row_iter.next() {
        let mut cell_iter = cells.update(row).expect("CellIterator::update");
        let mut line = Vec::new();
        while let Some(cell) = cell_iter.next() {
            let chars: Vec<char> = cell.graphemes().expect("graphemes");
            let fg = cell.fg_color().expect("fg_color");
            let wide = cell.raw_cell().expect("raw_cell").wide().expect("wide");
            line.push(OwnedCell {
                chars: chars.into_iter().collect(),
                fg,
                wide,
            });
        }
        out.push(line);
    }
    out
}

fn row_text(row: &[OwnedCell]) -> String {
    row.iter().map(|c| c.chars.clone()).collect()
}

/// owned 快照在原 Terminal 被 drop 后仍然有效（无悬垂引用）。
#[test]
fn v8_owned_snapshot() {
    let mut term = Terminal::new(80, 24).expect("Terminal::new");
    term.vt_write(b"hello-v8-snapshot");
    let snap = snapshot_grid(&term);

    // 继续写入并清屏
    term.vt_write(b"more data after snapshot\r\n");
    term.vt_write(b"\x1b[2J");

    // 当前网格已变化（清屏后全空）
    let now = snapshot_grid(&term);
    assert!(
        now.iter().all(|row| row.iter().all(|c| c.chars.is_empty())),
        "清屏后当前网格应全空"
    );

    // 先前快照内容不变
    let s0 = row_text(&snap[0]);
    assert!(
        s0.contains("hello-v8-snapshot"),
        "快照应保留清屏前内容，实际 {:?}",
        s0
    );

    // drop 原始终端后快照仍可完整读取（无悬垂引用）
    drop(term);
    let s1 = row_text(&snap[0]);
    assert_eq!(s0, s1, "drop(term) 后快照内容应保持不变");
}

/// 快照耗时：80x24 与 200x60 各 30 次，输出中位数（硬件相关，仅记录不断言）。
#[test]
fn v8_snapshot_bench() {
    for (cols, rows) in [(80u16, 24u16), (200u16, 60u16)] {
        let mut term = Terminal::new(cols, rows).expect("Terminal::new");
        for i in 0..rows {
            term.vt_write(format!("row {:03} padding text\r\n", i).as_bytes());
        }
        let _ = snapshot_grid(&term); // 预热
        let n = 30;
        let mut times = Vec::with_capacity(n);
        for _ in 0..n {
            let t0 = Instant::now();
            let g = snapshot_grid(&term);
            times.push(t0.elapsed());
            black_box(g);
        }
        times.sort_by_key(|d| d.as_nanos());
        let med = times[n / 2];
        println!(
            "BENCH {}x{} 快照中位数耗时: {:?} ({} 次取样)",
            cols, rows, med, n
        );
    }
}
