//! V6：portable-pty 起 /bin/sh，后台线程读 PTY 经 channel 送入 Terminal。
//! Terminal 仅在主线程（本测试线程）访问；reader 线程只接触 PTY master。

use libghostty_vt::{RenderState, Terminal};
use libghostty_vt::render::{CellIterator, RowIterator};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 收集网格全部单元格文本（每行 trim_end 后换行连接）。
fn grid_text(term: &Terminal<'_, '_>) -> String {
    let mut render_state = RenderState::new().expect("RenderState::new");
    let mut row_iter_factory = RowIterator::new().expect("RowIterator::new");
    let mut cell_iter_factory = CellIterator::new().expect("CellIterator::new");
    let snapshot = render_state.update(term).expect("RenderState::update");
    let mut row_iter = row_iter_factory.update(&snapshot).expect("RowIterator::update");
    let mut out = String::new();
    while let Some(row) = row_iter.next() {
        let mut cell_iter = cell_iter_factory.update(row).expect("CellIterator::update");
        let mut line = String::new();
        while let Some(cell) = cell_iter.next() {
            for ch in cell.graphemes().expect("cell.graphemes") {
                line.push(ch);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn v6_pty_echo_cn_and_resize() {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .expect("openpty");

    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.cwd("/tmp");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn /bin/sh");
    drop(pair.slave);

    let master = pair.master;
    let reader = master.try_clone_reader().expect("clone reader");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    // 后台线程：PTY master -> channel。不接触 Terminal。
    let reader_thread = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // 主线程：Terminal 独占访问。
    let mut term = Terminal::new(80, 24).expect("Terminal::new");
    assert_eq!(term.cols().expect("cols"), 80);
    assert_eq!(term.rows().expect("rows"), 24);

    // 写入 echo 你好
    {
        let mut writer = master.take_writer().expect("take_writer");
        writer.write_all("echo 你好\n".as_bytes()).expect("write echo");
        writer.flush().expect("flush");
    }

    // 经 channel 喂入 Terminal，直到网格出现 你好
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_hello = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(data) => term.vt_write(&data),
            Err(_) => {}
        }
        if grid_text(&term).contains("你好") {
            saw_hello = true;
            break;
        }
    }
    assert!(saw_hello, "网格中未出现 你好；当前网格:\n{}", grid_text(&term));

    // resize：PTY 与 Terminal 同步，断言行列变化
    master
        .resize(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .expect("pty resize");
    term.resize(120, 40, 8, 16).expect("Terminal::resize");
    assert_eq!(term.cols().expect("cols after resize"), 120);
    assert_eq!(term.rows().expect("rows after resize"), 40);

    // 收尾：直接 kill bash，避免交互 shell 的 history/exit 时序问题
    let _ = child.kill();
    drop(master);
    let _ = child.wait();
    let _ = reader_thread.join();
}
