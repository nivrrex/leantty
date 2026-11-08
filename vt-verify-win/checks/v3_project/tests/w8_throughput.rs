//! W8：100MB 带换行 ASCII 喂入吞吐（release 基线）。只记录耗时与吞吐，不做断言。

use libghostty_vt::Terminal;
use std::time::Instant;

#[test]
fn w8_feed_100mb_ascii_throughput() {
    let mut term = Terminal::new(80, 24).expect("Terminal::new");
    // 64KiB chunk，每 80 字节一个换行（带换行 ASCII）
    let chunk: Vec<u8> = (0..64 * 1024)
        .map(|i| if i % 80 == 79 { b'\n' } else { b'a' + (i % 26) as u8 })
        .collect();
    let total = 100 * 1024 * 1024;
    let n = total / chunk.len();
    let t0 = Instant::now();
    for _ in 0..n {
        term.vt_write(&chunk);
    }
    let el = t0.elapsed();
    let mbps = total as f64 / 1024.0 / 1024.0 / el.as_secs_f64();
    println!(
        "THROUGHPUT feed={} bytes ({} x {}B) elapsed={:?} throughput={:.2} MiB/s",
        n * chunk.len(),
        n,
        chunk.len(),
        el,
        mbps
    );
    println!(
        "THROUGHPUT-MBPS {:.4}",
        mbps
    );
    // 收尾状态记录
    println!(
        "THROUGHPUT scrollback_rows={:?}",
        term.scrollback_rows()
    );
}
