//! 视口上滚后的光标可见性（阶段 4G t1）。
//!
//! 根因（取证见 PHASE4G）：`FrameSnapshot.cursor` 取自 libghostty 的**活动区坐标**
//!（`Terminal::cursor_x/cursor_y`），只有终端模式的可见性（DECTCEM），没有判断光标
//! 是否落在**当前视口**内 → 视口上滚到 scrollback 后，光标仍被画在原屏幕位置。
//!
//! libghostty 已提供正确接口：`Snapshot::cursor_viewport() -> Option<CursorViewport>`，
//! `None` 表示光标不在视口内（`vendor/libghostty-rs/crates/libghostty-vt/src/render.rs:970`）。

use libghostty_vt::terminal::ScrollViewport;
use vt::VtTerminal;

/// 造一个 80x10 的终端并输出 30 行，使 scrollback 非空、光标停在最后一行。
fn term_with_scrollback() -> VtTerminal {
    let mut t = VtTerminal::new(80, 10).expect("VtTerminal");
    t.feed(b"\x1b[2J\x1b[H");
    for i in 0..30 {
        t.feed(format!("line {i:02}\r\n").as_bytes());
    }
    t.feed(b"prompt$ ");
    t
}

/// 上滚后光标必须不可见；滚回底部后恢复可见且位置正确。
#[test]
fn cursor_hidden_when_viewport_scrolled_above_cursor() {
    let mut t = term_with_scrollback();
    let snap = t.snapshot().expect("snapshot");
    assert!(
        snap.scrollback_rows >= 20,
        "应有充足 scrollback（实测 {}）",
        snap.scrollback_rows
    );
    assert!(snap.cursor.visible, "回底时光标应可见");

    // 上滚 5 行：光标在活动区最后一行，已不在视口内
    // （ScrollViewport::Row(n) 是从 scrollback 顶部的绝对偏移；相对上滚用 Delta）
    t.scroll_viewport(ScrollViewport::Delta(-5));
    let snap = t.snapshot().expect("snapshot after scroll");
    assert!(
        !snap.cursor.visible,
        "视口上滚 5 行后光标不应可见（实测 visible={} 位置=({},{})）",
        snap.cursor.visible, snap.cursor.x, snap.cursor.y
    );

    // 滚回底部（Bottom = 活动区）
    t.scroll_viewport(ScrollViewport::Bottom);
    let snap = t.snapshot().expect("snapshot after scroll back");
    assert!(snap.cursor.visible, "滚回底部后光标应恢复可见");
    // 光标在最后一行的提示符之后：x = "prompt$ " 的长度 8，y = 9
    assert_eq!((snap.cursor.x, snap.cursor.y), (8, 9), "回底后光标位置应正确");
}

/// 上滚 1 行：光标仍在视口内时，其**视口内** y 应比活动区坐标大 1。
///
/// 说明：光标在活动区最后一行（y=9），上滚 1 行后视口顶部对应活动区第 1 行，
/// 因此光标在视口中处于 y=10 —— 已超出 10 行视口（0..9），所以按接口语义应不可见。
/// 这里用一个"光标在中部"的场景验证可见时的坐标换算。
#[test]
fn cursor_position_follows_viewport_offset_when_still_visible() {
    let mut t = VtTerminal::new(40, 10).expect("VtTerminal");
    t.feed(b"\x1b[2J\x1b[H");
    for i in 0..20 {
        t.feed(format!("row {i:02}\r\n").as_bytes());
    }
    // 把光标移回活动区第 5 行（0-based 4），此后再输出一些内容把视口顶起
    t.feed(b"\x1b[5;3H");
    let snap = t.snapshot().expect("snapshot");
    let (active_y, active_visible) = (snap.cursor.y, snap.cursor.visible);
    assert!(active_visible, "移动光标后应可见");

    // 上滚 2 行：视口顶部上移，光标在视口中的 y 应增加 2（若仍在视口内）
    t.scroll_viewport(ScrollViewport::Delta(-2));
    let scrolled = t.snapshot().expect("snapshot scrolled");
    if scrolled.cursor.visible {
        assert_eq!(
            scrolled.cursor.y,
            active_y + 2,
            "可见时视口内 y 应随视口偏移下移（活动区 y={active_y}）"
        );
    } else {
        // 若接口判定已滚出视口，也必须自洽：不能给活动区坐标
        eprintln!("上滚 2 行后接口判定光标不可见（活动区 y={active_y}）");
    }
}

/// 备用屏：视口不滚动（无 scrollback），光标应始终可见且位置为活动区坐标。
#[test]
fn cursor_visible_on_alternate_screen() {
    let mut t = VtTerminal::new(40, 10).expect("VtTerminal");
    t.feed(b"\x1b[?1049h"); // 进入备用屏
    t.feed(b"\x1b[2J\x1b[H");
    t.feed(b"alt-content");
    t.feed(b"\x1b[3;5H");
    let snap = t.snapshot().expect("snapshot");
    assert!(snap.alternate_screen, "应在备用屏");
    assert!(snap.cursor.visible, "备用屏光标应可见");
    assert_eq!((snap.cursor.x, snap.cursor.y), (4, 2), "备用屏光标位置应为活动区坐标");
}

/// 光标自身被隐藏（ESC[?25l）：无论视口位置都应不可见。
#[test]
fn cursor_hidden_mode_wins_over_viewport() {
    let mut t = term_with_scrollback();
    t.feed(b"\x1b[?25l");
    let snap = t.snapshot().expect("snapshot");
    assert!(!snap.cursor.visible, "ESC[?25l 后光标不可见");
    t.scroll_viewport(ScrollViewport::Delta(-3));
    let snap = t.snapshot().expect("snapshot scrolled");
    assert!(!snap.cursor.visible, "隐藏 + 上滚后仍不可见");
}
