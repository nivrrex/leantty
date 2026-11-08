//! V8：FrameSnapshot 为自有拷贝；原始终端 drop 后快照不变且无悬垂引用。

use vt::VtTerminal;

#[test]
fn v8_owned_snapshot() {
    let mut term = VtTerminal::new(80, 24).expect("VtTerminal::new");
    term.feed(b"hello-v8-snapshot");
    let snap = term.snapshot().expect("snapshot");

    // 继续写入并清屏
    term.feed(b"more data after snapshot\r\n");
    term.feed(b"\x1b[2J");

    // 当前网格已变化（清屏后无内容）
    let now = term.snapshot().expect("snapshot after clear");
    assert!(
        now.cells
            .iter()
            .all(|row| row.iter().all(|c| c.chars.is_empty())),
        "清屏后当前网格应无内容"
    );

    // 先前快照内容不变
    let s0: String = snap.cells[0].iter().map(|c| c.chars.clone()).collect();
    assert!(
        s0.contains("hello-v8-snapshot"),
        "快照应保留清屏前内容，实际 {:?}",
        s0
    );

    // drop 原始终端后快照仍可完整读取（无悬垂引用）
    drop(term);
    let s1: String = snap.cells[0].iter().map(|c| c.chars.clone()).collect();
    assert_eq!(s0, s1, "drop(term) 后快照内容应保持不变");
}
