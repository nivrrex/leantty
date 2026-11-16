//! 探针：snapshot wrapped 标记与软折行语义（e2e 真实序列复现）。
use vt::VtTerminal;

fn row_text(snap: &vt::FrameSnapshot, row: usize) -> String {
    let mut s = String::new();
    if let Some(cells) = snap.cells.get(row) {
        for c in cells {
            if c.wide == vt::CellWide::SpacerTail || c.wide == vt::CellWide::SpacerHead {
                continue;
            }
            s.push_str(&c.chars);
        }
    }
    s.trim_end().to_string()
}

#[test]
fn wrapped_flags_probe() {
    let mut vt = VtTerminal::new(20, 6).expect("vt");
    vt.feed(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\nB1\r\n");
    let snap = vt.snapshot().expect("snapshot");
    for (i, w) in snap.wrapped.iter().enumerate() {
        println!("row {}: wrapped={} text=[{}]", i, w, row_text(&snap, i));
    }
}

#[test]
fn wrapped_flags_probe_80col() {
    // e2e 同口径：80 列+100A+\r\n+B1..B3
    let mut vt = VtTerminal::new(80, 24).expect("vt");
    let a100 = "A".repeat(100);
    let mut input = a100.into_bytes();
    input.extend_from_slice(b"\r\n");
    input.extend_from_slice(b"B1\r\nB2\r\nB3\r\n");
    vt.feed(&input);
    let snap = vt.snapshot().expect("snapshot");
    for (i, w) in snap.wrapped.iter().enumerate() {
        let t = row_text(&snap, i);
        if !t.is_empty() || i < 6 {
            println!(
                "row {}: wrapped={} text=[{}] len={}",
                i,
                w,
                t,
                t.chars().count()
            );
        }
    }
    // 提取断言在 app 侧 selection.rs 单测（vt 探针只验证标记填充）。
}
