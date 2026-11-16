//! 探针（阶段 3A 任务 3）：libghostty 脏语义可用性验证（write-once 报告）。
use vt::{ScrollViewport, VtTerminal};

fn dirty(vt: &VtTerminal, label: &str) {
    let snap = vt.snapshot_incremental().unwrap();
    let rows: Vec<usize> = (0..snap.dirty_rows.len())
        .filter(|i| snap.dirty_rows[*i])
        .collect();
    println!("[{label}] full={} dirty_rows={:?}", snap.full_redraw, rows);
}

#[test]
fn dirty_semantics_probe() {
    let mut vt = VtTerminal::new(10, 4).unwrap();
    dirty(&vt, "init");
    vt.feed(b"abc\r\n");
    dirty(&vt, "after feed 2 rows");
    dirty(&vt, "no change");
    vt.feed(b"X");
    dirty(&vt, "1 cell change");
    vt.feed(b"\x1b[2;1H"); // 光标移动到第 2 行
    dirty(&vt, "cursor move only");
    vt.resize(20, 6).unwrap();
    dirty(&vt, "after resize");
    for i in 0..30 {
        vt.feed(format!("line{i}\r\n").as_bytes());
    }
    dirty(&vt, "after scroll output");
    vt.scroll_viewport(ScrollViewport::Delta(-3));
    dirty(&vt, "after scroll viewport up");
    vt.scroll_viewport(ScrollViewport::Bottom);
    dirty(&vt, "after scroll back to bottom");
}
