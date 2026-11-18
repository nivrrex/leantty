//! rows_text 单测：跨视口历史行只读读取+视口不变+软折标记。
use vt::{ScrollViewport, VtTerminal};

fn feed_lines(vt: &mut VtTerminal, n: usize, tag: &str) {
    for i in 0..n {
        let line = format!("{}{:03}\r\n", tag, i);
        vt.feed(line.as_bytes());
    }
}

#[test]
fn rows_text_reads_history_without_moving_viewport() {
    let mut vt = VtTerminal::new(20, 6).expect("vt");
    feed_lines(&mut vt, 30, "H"); // 30 行→scrollback 24
    // 上滚 10 行后读历史行（含视口外）；rows_text 前后视口对比基准
    vt.scroll_viewport(ScrollViewport::Delta(-10));
    let snap_before = vt.snapshot().expect("snap");
    let first_before = snap_before.cells.first().cloned().unwrap_or_default();
    let rows = vt.rows_text(0, 5, 20).expect("rows_text");
    assert_eq!(rows.len(), 6, "请求 6 行应返回 6 行");
    let text_of = |cells: &[vt::Cell]| cells.iter().map(|c| c.chars.as_str()).collect::<String>();
    assert_eq!(
        text_of(&rows[0].0).trim_end(),
        "H000",
        "scrollback 顶行可读（视口外）"
    );
    assert_eq!(text_of(&rows[5].0).trim_end(), "H005");
    // 视口不变（rows_text 只读）
    let snap_after = vt.snapshot().expect("snap");
    let first_after = snap_after.cells.first().cloned().unwrap_or_default();
    assert_eq!(
        first_before
            .iter()
            .map(|c| c.chars.clone())
            .collect::<Vec<_>>(),
        first_after
            .iter()
            .map(|c| c.chars.clone())
            .collect::<Vec<_>>(),
        "rows_text 不改变视口"
    );
    // 软折标记：20 列下 40 字符串行软折 2 行（上行 wrapped=true）
    let mut vt2 = VtTerminal::new(10, 4).expect("vt2");
    vt2.feed(b"0123456789ABCDEF\r\n"); // 18 字符>10 列→软折
    let rows2 = vt2.rows_text(0, 1, 10).expect("rows2");
    assert!(rows2[0].1, "软折上行 wrapped=true");
    assert!(!rows2[1].1, "续行 wrapped=false");
    assert_eq!(text_of(&rows2[0].0).trim_end(), "0123456789", "上行文本");
    assert_eq!(text_of(&rows2[1].0).trim_end(), "ABCDEF", "续行文本");
}
