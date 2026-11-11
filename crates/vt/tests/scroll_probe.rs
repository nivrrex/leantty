//! 探针（任务 4 前置）：scroll_viewport 上滚后 feed 新输出，视口是否自动回底。
//! 结论驱动 auto-follow 层位（app 状态机 vs 依赖 vt 语义）。
use vt::{ScrollViewport, VtTerminal};

fn feed_lines(vt: &mut VtTerminal, n: usize, tag: &str) {
    for i in 0..n {
        let line = format!("{}{:03}\r\n", tag, i);
        vt.feed(line.as_bytes());
    }
}

fn first_visible(vt: &mut VtTerminal) -> String {
    let snap = vt.snapshot().expect("snapshot");
    let mut first = String::new();
    if let Some(row) = snap.cells.first() {
        for cell in row {
            if cell.chars.is_empty() || cell.wide == vt::CellWide::SpacerTail {
                continue;
            }
            first.push_str(&cell.chars);
        }
    }
    first.trim_end().to_string()
}

#[test]
fn scroll_up_then_feed_stays_or_follows() {
    let mut vt = VtTerminal::new(80, 10).expect("vt");
    feed_lines(&mut vt, 30, "A");
    let before = first_visible(&mut vt);
    // 上滚 5 行（Delta 负=上滚，scrollback 方向）
    vt.scroll_viewport(ScrollViewport::Delta(-5));
    let scrolled = first_visible(&mut vt);
    // 新输出
    feed_lines(&mut vt, 3, "B");
    let after = first_visible(&mut vt);
    println!("BEFORE={} SCOOLED={} AFTER={}", before, scrolled, after);
    println!("scrollback_rows={}", vt.snapshot().map(|s| s.scrollback_rows).unwrap_or(0));
    // 断言视口内容变化可观测（上滚生效）
    assert_ne!(before, scrolled, "上滚后首行应变化（视口移动生效）");
    // 关键结论：新输出后视口是否自动回底（AFTER 首行=滚动前的底部行或 B 标签行）
    if after.contains("B") || after != scrolled {
        println!("结论：新输出后视口内容变化（自动回底或继续移动）");
    } else {
        println!("结论：新输出后视口保持上滚位置（不自动回底）");
    }
}

#[test]
fn scroll_bottom_restores() {
    let mut vt = VtTerminal::new(80, 10).expect("vt");
    feed_lines(&mut vt, 30, "A");
    vt.scroll_viewport(ScrollViewport::Delta(-5));
    vt.scroll_viewport(ScrollViewport::Bottom);
    let bottom = first_visible(&mut vt);
    println!("BOTTOM first={}", bottom);
    assert_ne!(bottom, "", "Bottom 后视口非空");
}
