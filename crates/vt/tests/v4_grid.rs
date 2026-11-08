//! V4（针对自有 FrameSnapshot）：喂 VT 输入，逐条断言网格内容。

use vt::{CellWide, VtTerminal};

fn grid(term: &VtTerminal) -> vt::FrameSnapshot {
    term.snapshot().expect("snapshot")
}

fn new_term() -> VtTerminal {
    VtTerminal::new(80, 24).expect("VtTerminal::new")
}

/// a. 普通文本 "hello"，断言首行前 5 格字符。
#[test]
fn v4a_plain_text() {
    let mut term = new_term();
    term.feed(b"hello");
    let snap = grid(&term);
    let first: String = snap.cells[0][0..5].iter().map(|c| c.chars.clone()).collect();
    assert_eq!(first, "hello", "首行前 5 格应为 hello，实际 {:?}", first);
}

/// b. "\x1b[31mred\x1b[0m"，断言 "red" 三格前景红，其后格恢复默认。
#[test]
fn v4b_sgr_red() {
    let mut term = new_term();
    term.feed(b"\x1b[31mred\x1b[0m");
    let snap = grid(&term);
    for i in 0..3 {
        let fg = snap.cells[0][i].fg.expect("red 格应有前景色");
        // ghostty 默认调色板 red=(204,102,102)，断言红色主导即可
        assert!(
            fg.r > 150 && fg.r > fg.g + 50 && fg.r > fg.b + 50,
            "第 {} 格应为红色，实际 rgb=({},{},{})",
            i, fg.r, fg.g, fg.b
        );
    }
    // \x1b[0m 之后的格子恢复默认前景
    assert!(
        snap.cells[0][3].fg.is_none(),
        "red 之后的格应恢复默认前景，实际 {:?}",
        snap.cells[0][3].fg
    );
}

/// c. "\x1b[2J\x1b[H" 清屏后光标回到 (0,0)，网格全空。
#[test]
fn v4c_clear_screen() {
    let mut term = new_term();
    term.feed(b"hello world\r\nsecond line");
    term.feed(b"\x1b[2J\x1b[H");
    let snap = grid(&term);
    assert_eq!(snap.cursor.x, 0, "光标 x 应为 0");
    assert_eq!(snap.cursor.y, 0, "光标 y 应为 0");
    let nonempty: usize = snap
        .cells
        .iter()
        .map(|row| row.iter().filter(|c| !c.chars.is_empty()).count())
        .sum();
    assert_eq!(nonempty, 0, "清屏后网格应全空，实际非空格 {} 个", nonempty);
}

/// d. 24 行终端输出 60 行带编号文本，断言 scrollback 行数 ≥ 36，
///    且视口底部为最后一行。
#[test]
fn v4d_scrollback() {
    let mut term = new_term();
    for i in 1..=60 {
        term.feed(format!("line {:02}\r\n", i).as_bytes());
    }
    let snap = grid(&term);
    assert!(
        snap.scrollback_rows >= 36,
        "scrollback 行数应 ≥ 36（60 行输出 - 24 行视口），实际 {}",
        snap.scrollback_rows
    );
    // 末行 \r\n 使光标落在空行，line 60 是视口最后一个非空行
    let last_nonempty = snap
        .cells
        .iter()
        .rev()
        .find(|row| row.iter().any(|c| !c.chars.is_empty()))
        .expect("视口应有非空行");
    let last: String = last_nonempty.iter().map(|c| c.chars.clone()).collect();
    assert!(
        last.contains("60"),
        "视口底部最后非空行应为 line 60，实际 {:?}",
        last
    );
}

/// e. "你好abc"，断言 "你" 占 2 格（第 2 格为宽字符续格标记），"好" 同，
///    a/b/c 依次落在其后各 1 格。
#[test]
fn v4e_wide_chars() {
    let mut term = new_term();
    term.feed("你好abc".as_bytes());
    let snap = grid(&term);
    let row = &snap.cells[0];
    assert_eq!(row[0].chars, "你", "第 1 格应为 你");
    assert_eq!(row[0].wide, CellWide::Wide, "你 应占 2 格");
    assert_eq!(row[1].wide, CellWide::SpacerTail, "第 2 格应为宽字符续格标记");
    assert_eq!(row[2].chars, "好", "第 3 格应为 好");
    assert_eq!(row[2].wide, CellWide::Wide, "好 应占 2 格");
    assert_eq!(row[3].wide, CellWide::SpacerTail, "第 4 格应为宽字符续格标记");
    assert_eq!(row[4].chars, "a", "第 5 格应为 a");
    assert_eq!(row[5].chars, "b", "第 6 格应为 b");
    assert_eq!(row[6].chars, "c", "第 7 格应为 c");
}

/// f. "\x1b[?1049h" 后处于 alternate screen，scrollback 对该屏不可用；
///    "\x1b[?1049l" 后恢复主屏内容。
#[test]
fn v4f_alternate_screen() {
    let mut term = new_term();
    term.feed(b"main-screen-content");
    assert!(!term.snapshot().expect("snapshot").alternate_screen);

    term.feed(b"\x1b[?1049h");
    term.feed(b"alt-content");
    let snap_alt = grid(&term);
    assert!(snap_alt.alternate_screen, "1049h 后应为 alternate screen");
    // alternate screen 无 scrollback
    assert_eq!(
        snap_alt.scrollback_rows, 0,
        "alternate screen 的 scrollback 应不可用（为 0），实际 {}",
        snap_alt.scrollback_rows
    );
    let alt_text: String = snap_alt.cells[0].iter().map(|c| c.chars.clone()).collect();
    assert!(
        alt_text.contains("alt-content"),
        "alt 屏应含 alt-content，实际 {:?}",
        alt_text
    );

    term.feed(b"\x1b[?1049l");
    let snap = grid(&term);
    assert!(!snap.alternate_screen, "1049l 后应恢复主屏");
    let main_text: String = snap.cells[0].iter().map(|c| c.chars.clone()).collect();
    assert!(
        main_text.contains("main-screen-content"),
        "主屏内容应恢复，实际 {:?}",
        main_text
    );
}
