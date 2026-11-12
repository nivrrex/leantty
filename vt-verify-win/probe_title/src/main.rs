// Feeds an OSC 0 title sequence and prints what the vt layer reports.
fn main() {
    let mut t = vt::VtTerminal::new(80, 24).expect("terminal");
    t.feed(b"hello\r\n");
    println!("before={:?}", t.title());
    t.feed(b"\x1b]0;T123\x07");
    println!("after_bel={:?}", t.title());
    t.feed(b"\x1b]2;T456\x1b\\");
    println!("after_st={:?}", t.title());
    let snap = t.snapshot().expect("snap");
    println!("snapshot_rows={}", snap.rows);
}
