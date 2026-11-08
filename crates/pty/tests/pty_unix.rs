//! pty/unix：起 /bin/sh，echo 中文入网格且汉字占 2 格；resize 双侧一致；
//! close 顺序不卡死（带超时）。

use std::sync::mpsc;
use std::time::{Duration, Instant};

use pty::{NativePty, Pty};
use vt::{CellWide, VtTerminal};
#[test]
fn pty_echo_cn_resize_close() {
    let mut pty = NativePty::new();
    pty.spawn("/bin/sh", 80, 24).expect("spawn /bin/sh");

    let reader = pty.take_reader().expect("take_reader");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader_thread = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // 键盘写 → shell echo
    {
        let writer = pty.writer().expect("writer");
        writer.write_all("echo 你好\n".as_bytes()).expect("write echo");
        writer.flush().expect("flush");
    }

    // 主线程：Terminal 独占访问；收 PTY 输出喂 vt
    let mut term = VtTerminal::new(80, 24).expect("VtTerminal::new");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut collected = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => collected.extend_from_slice(&bytes),
            Err(_) => break,
        }
        if collected.windows(6).any(|w| w == "你好".as_bytes()) {
            break;
        }
    }
    assert!(
        collected.windows(6).any(|w| w == "你好".as_bytes()),
        "PTY 输出应含 你好，实际 {:?}",
        String::from_utf8_lossy(&collected)
    );
    term.feed(&collected);

    let snap = term.snapshot().expect("snapshot");
    let wide_cell = snap.cells.iter().flatten().find(|c| c.chars == "你");
    let wide_cell = wide_cell.expect("网格应含 你");
    assert_eq!(wide_cell.wide, CellWide::Wide, "你 应占 2 格");

    // resize 双侧一致
    pty.resize(120, 40).expect("pty resize");
    term.resize(120, 40).expect("vt resize");
    let snap = term.snapshot().expect("snapshot after resize");
    assert_eq!(snap.cols, 120, "resize 后 cols 应为 120");
    assert_eq!(snap.rows, 40, "resize 后 rows 应为 40");

    // close 须快速返回（先释放主端再回收子进程），不得卡死
    let t0 = Instant::now();
    pty.close().expect("close");
    let close_elapsed = t0.elapsed();
    assert!(
        close_elapsed < Duration::from_secs(5),
        "close 耗时 {}ms，超过 5s 契约上限",
        close_elapsed.as_millis()
    );

    // 读线程 join 带超时
    pty::unix::join_thread_timeout(reader_thread, Duration::from_secs(5))
        .expect("reader thread join 超时上限内完成");

    // 子进程已退出：try_wait 在 close 后无子进程，验证 close 幂等
    pty.close().expect("close 幂等");
}
