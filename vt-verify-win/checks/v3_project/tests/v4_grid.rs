//! V4：向 Terminal 喂 VT 输入，逐条断言网格内容。

use libghostty_vt::{RenderState, Terminal};
use libghostty_vt::render::{CellIterator, RowIterator};
use libghostty_vt::screen::{CellWide, Screen};

struct CellData {
    chars: String,
    fg: Option<libghostty_vt::style::RgbColor>,
    wide: CellWide,
}

/// 读取视口全部行/格（graphemes + 前景色 + 宽度标记）。
fn grid(term: &Terminal<'_, '_>) -> Vec<Vec<CellData>> {
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
            line.push(CellData { chars: chars.into_iter().collect(), fg, wide });
        }
        out.push(line);
    }
    out
}

fn new_term() -> Terminal<'static, 'static> {
    Terminal::new(80, 24).expect("Terminal::new")
}

/// a. 普通文本 "hello"，断言首行前 5 格字符。
#[test]
fn v4a_plain_text() {
    let mut term = new_term();
    term.vt_write(b"hello");
    let g = grid(&term);
    let first: String = g[0][0..5].iter().map(|c| c.chars.clone()).collect();
    assert_eq!(first, "hello", "首行前 5 格应为 hello，实际 {:?}", first);
}

/// b. "\x1b[31mred\x1b[0m"，断言 "red" 三格前景色为红，其后格恢复默认。
#[test]
fn v4b_sgr_red() {
    let mut term = new_term();
    term.vt_write(b"\x1b[31mred\x1b[0m");
    let g = grid(&term);
    for i in 0..3 {
        let fg = g[0][i].fg.expect("red 格应有前景色");
        // ghostty 默认调色板 red=(204,102,102)，断言红色主导即可
        assert!(
            fg.r > 150 && fg.r > fg.g + 50 && fg.r > fg.b + 50,
            "第 {} 格应为红色，实际 rgb=({},{},{})",
            i, fg.r, fg.g, fg.b
        );
    }
    // \x1b[0m 之后的格子恢复默认前景
    assert!(
        g[0][3].fg.is_none(),
        "red 之后的格应恢复默认前景，实际 {:?}",
        g[0][3].fg
    );
}

/// c. "\x1b[2J\x1b[H" 清屏后光标回到 (0,0)，网格全空。
#[test]
fn v4c_clear_screen() {
    let mut term = new_term();
    term.vt_write(b"hello world\r\nsecond line");
    term.vt_write(b"\x1b[2J\x1b[H");
    assert_eq!(term.cursor_x().expect("cursor_x"), 0, "光标 x 应为 0");
    assert_eq!(term.cursor_y().expect("cursor_y"), 0, "光标 y 应为 0");
    let g = grid(&term);
    let nonempty: usize = g.iter()
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
        term.vt_write(format!("line {:02}\r\n", i).as_bytes());
    }
    let sb = term.scrollback_rows().expect("scrollback_rows");
    assert!(
        sb >= 36,
        "scrollback 行数应 ≥ 36（60 行输出 - 24 行视口），实际 {}",
        sb
    );
    let g = grid(&term);
    // 末行 \r\n 使光标落在空行，line 60 是视口最后一个非空行
    let last_nonempty = g.iter().rev().find(|row| row.iter().any(|c| !c.chars.is_empty()))
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
    term.vt_write("你好abc".as_bytes());
    let g = grid(&term);
    let row = &g[0];
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
    term.vt_write(b"main-screen-content");
    assert_eq!(term.active_screen().expect("active_screen"), Screen::Primary);

    term.vt_write(b"\x1b[?1049h");
    assert_eq!(
        term.active_screen().expect("active_screen"),
        Screen::Alternate,
        "1049h 后应为 alternate screen"
    );
    // alternate screen 无 scrollback
    let sb_alt = term.scrollback_rows().expect("scrollback_rows in alt");
    assert_eq!(sb_alt, 0, "alternate screen 的 scrollback 应不可用（为 0），实际 {}", sb_alt);

    term.vt_write(b"alt-content");
    let g_alt = grid(&term);
    let alt_text: String = g_alt[0].iter().map(|c| c.chars.clone()).collect();
    assert!(alt_text.contains("alt-content"), "alt 屏应含 alt-content，实际 {:?}", alt_text);

    term.vt_write(b"\x1b[?1049l");
    assert_eq!(
        term.active_screen().expect("active_screen"),
        Screen::Primary,
        "1049l 后应恢复主屏"
    );
    let g = grid(&term);
    let main_text: String = g[0].iter().map(|c| c.chars.clone()).collect();
    assert!(
        main_text.contains("main-screen-content"),
        "主屏内容应恢复，实际 {:?}",
        main_text
    );
}
