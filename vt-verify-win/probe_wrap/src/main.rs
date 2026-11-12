// Feeds a wrapped line into the vt engine and prints the snapshot's per-row
// soft-wrap flags, so the copy-newline semantics can be checked without the app.
fn main() {
    let cols: u16 = std::env::var("PROBE_COLS").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    let rows: u16 = 30;
    let mut t = vt::VtTerminal::new(cols, rows).expect("terminal");
    let n: usize = std::env::var("PROBE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let mut data = String::new();
    for _ in 0..n { data.push('A'); }
    data.push_str("\r\nB1\r\nB2\r\n");
    t.feed(data.as_bytes());
    let snap = t.snapshot().expect("snapshot");
    println!("cols={} rows={} wrapped_len={}", snap.cols, snap.rows, snap.wrapped.len());
    for (i, w) in snap.wrapped.iter().enumerate() {
        let text: String = snap.cells[i].iter().map(|c| c.chars.as_str()).collect();
        let t = text.trim_end();
        if !t.is_empty() {
            println!("row={} wrapped={} text_len={} head={}", i, w, t.chars().count(), &t[..t.len().min(12)]);
        }
    }
}
