//! W5: ConPTY special. portable-pty spawns cmd.exe and powershell.exe, one round each.
//! Structural constraint: a background thread reads the PTY master and forwards
//! chunks over a channel; Terminal is touched only from the main thread.
//! Records observed behavior only; no inference.

use libghostty_vt::terminal::DeviceAttributes;
use libghostty_vt::{RenderState, Terminal};
use libghostty_vt::render::{CellIterator, RowIterator};
use libghostty_vt::screen::CellWide;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG_DIR: &str = "C:\\vt\\logs";

enum Msg {
    Data(Vec<u8>),
    Eof,
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn count_sub(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0;
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            n += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    n
}

fn grid_text(term: &Terminal<'_, '_>) -> String {
    let mut render_state = RenderState::new().expect("RenderState::new");
    let mut row_iter_factory = RowIterator::new().expect("RowIterator::new");
    let mut cell_iter_factory = CellIterator::new().expect("CellIterator::new");
    let snapshot = render_state.update(term).expect("RenderState::update");
    let mut row_iter = row_iter_factory
        .update(&snapshot)
        .expect("RowIterator::update");
    let mut out = String::new();
    while let Some(row) = row_iter.next() {
        let mut ci = cell_iter_factory.update(row).expect("CellIterator::update");
        let mut line = String::new();
        while let Some(cell) = ci.next() {
            for ch in cell.graphemes().expect("cell.graphemes") {
                line.push(ch);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn all_rows_cells(term: &Terminal<'_, '_>) -> Vec<Vec<(String, CellWide)>> {
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
            let chars: Vec<char> = cell.graphemes().expect("graphemes").into_iter().collect();
            let wide = cell.raw_cell().expect("raw_cell").wide().expect("wide");
            line.push((chars.into_iter().collect(), wide));
        }
        out.push(line);
    }
    out
}

fn drain_until_quiet(
    rx: &mpsc::Receiver<Msg>,
    quiet: Duration,
    cap: Duration,
) -> (Vec<u8>, Vec<f64>) {
    let t0 = Instant::now();
    let mut out = Vec::new();
    let mut times = Vec::new();
    let mut last = Instant::now();
    loop {
        if t0.elapsed() > cap {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Msg::Data(d)) => {
                times.push(t0.elapsed().as_secs_f64());
                out.extend_from_slice(&d);
                last = Instant::now();
            }
            Ok(Msg::Eof) => {
                times.push(t0.elapsed().as_secs_f64());
                break;
            }
            Err(_) => {
                if last.elapsed() > quiet {
                    break;
                }
            }
        }
    }
    (out, times)
}

struct Ctx {
    name: String,
    t0: Instant,
    log: File,
    raw: File,
    cb_log: Arc<Mutex<File>>,
}

impl Ctx {
    fn line(&mut self, msg: &str) {
        let el = self.t0.elapsed().as_secs_f64();
        let _ = writeln!(self.log, "[{:9.3}] {}", el, msg);
        let _ = self.log.flush();
        println!("[{}] [{}] {}", el, self.name, msg);
    }
    fn raw_bytes(&mut self, data: &[u8]) {
        let _ = self.raw.write_all(data);
        let _ = self.raw.flush();
    }
}

fn do_write(
    ctx: &mut Ctx,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    data: &[u8],
    note: &str,
) {
    ctx.line(&format!("WRITE({}): {}", note, hex(data)));
    let mut w = writer.lock().expect("writer lock");
    w.write_all(data).expect("write pty");
    w.flush().expect("flush pty");
}

#[allow(clippy::too_many_arguments)]
fn run_round(
    name: &str,
    program: &str,
    args: &[&str],
    cwd: &str,
    cmds: &[&str],
    do_dsr_probe: bool,
    exit_cmd: &str,
    drop_master_first: bool,
) {
    let t0 = Instant::now();
    let log_path = format!("{}\\w5-{}-run.log", LOG_DIR, name);
    let raw_path = format!("{}\\w5-{}-raw.bin", LOG_DIR, name);
    let cb_path = format!("{}\\w5-{}-callbacks.log", LOG_DIR, name);
    let mut ctx = Ctx {
        name: name.to_string(),
        t0,
        log: File::create(&log_path).expect("create run log"),
        raw: File::create(&raw_path).expect("create raw bin"),
        cb_log: Arc::new(Mutex::new(File::create(&cb_path).expect("create callbacks log"))),
    };
    ctx.line(&format!("spawn {} {:?} cwd={}", program, args, cwd));

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut cmd = CommandBuilder::new(program);
    cmd.cwd(cwd);
    for a in args {
        cmd.arg(a);
    }
    let mut child = pair.slave.spawn_command(cmd).expect("spawn");
    drop(pair.slave);
    let master = pair.master;
    let reader = master.try_clone_reader().expect("clone reader");
    let writer = Arc::new(Mutex::new(
        master.take_writer().expect("take writer"),
    ));

    let (tx, rx) = mpsc::channel::<Msg>();
    let reader_thread = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(Msg::Eof);
                    break;
                }
                Err(_) => {
                    let _ = tx.send(Msg::Eof);
                    break;
                }
                Ok(n) => {
                    if tx.send(Msg::Data(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let cb_log = Arc::clone(&ctx.cb_log);
    let w_for_cb = Arc::clone(&writer);
    {
        let mut f = ctx.cb_log.lock().expect("cb lock");
        let _ = writeln!(f, "callbacks begin");
    }
    let mut term = Terminal::new(80, 24).expect("Terminal::new");
    let _ = term
        .on_pty_write(move |_term, data| {
            let mut f = cb_log.lock().expect("cb lock");
            let _ = writeln!(f, "pty_write: {}", hex(data));
            let _ = f.flush();
            let mut w = w_for_cb.lock().expect("writer lock");
            let _ = w.write_all(data);
            let _ = w.flush();
        })
        .expect("on_pty_write register");
    let cb_log2 = Arc::clone(&ctx.cb_log);
    let _ = term
        .on_device_attributes(move |_term| {
            let mut f = cb_log2.lock().expect("cb lock");
            let _ = writeln!(f, "device_attributes queried (answered None)");
            let _ = f.flush();
            None::<DeviceAttributes>
        })
        .expect("on_device_attributes register");

    let mut all: Vec<u8> = Vec::new();

    // (b) startup raw bytes
    let (startup, _) = drain_until_quiet(&rx, Duration::from_millis(800), Duration::from_secs(12));
    all.extend_from_slice(&startup);
    ctx.raw_bytes(&startup);
    ctx.line(&format!(
        "startup bytes: {} hex: {}",
        startup.len(),
        hex(&startup)
    ));
    let n_6n = count_sub(&startup, b"\x1b[6n");
    let n_9001 = count_sub(&startup, b"\x1b[?9001h");
    let n_1004 = count_sub(&startup, b"\x1b[?1004h");
    let n_osc = count_sub(&startup, b"\x1b]");
    ctx.line(&format!("marker ESC[6n (DSR cursor query): {} times", n_6n));
    ctx.line(&format!("marker ESC[?9001h: {} times", n_9001));
    ctx.line(&format!("marker ESC[?1004h: {} times", n_1004));
    ctx.line(&format!("marker OSC (ESC]): {} times", n_osc));
    let found_6n = n_6n > 0;
    for b in &startup {
        term.vt_write(std::slice::from_ref(b));
    }
    ctx.line(&format!(
        "startup fed to Terminal; first grid line: {:?}",
        grid_text(&term).lines().next().unwrap_or("")
    ));

    // (c) ESC[6n no-reply probe + callback write-back observation
    if do_dsr_probe && found_6n {
        ctx.line("ESC[6n seen: no-reply probe (send no DSR answer)");
        do_write(&mut ctx, &writer, b"ver\r", "probe-cmd");
        let (out, times) = drain_until_quiet(&rx, Duration::from_millis(600), Duration::from_secs(8));
        ctx.raw_bytes(&out);
        all.extend_from_slice(&out);
        let first_dt = times.first().copied().unwrap_or(-1.0);
        ctx.line(&format!(
            "no-reply: first byte after probe at {:?}s, {} bytes out, hex={}",
            first_dt,
            out.len(),
            hex(&out)
        ));
        for b in &out {
            term.vt_write(std::slice::from_ref(b));
        }
        let cy = term.cursor_y().expect("cursor_y");
        let cx = term.cursor_x().expect("cursor_x");
        let reply = format!("\x1b[{};{}R", cy + 1, cx + 1);
        ctx.line(&format!(
            "manual DSR reply (embedder action, not lib auto-reply): {}",
            hex(reply.as_bytes())
        ));
        do_write(&mut ctx, &writer, reply.as_bytes(), "manual-dsr-reply");
        let (out2, times2) =
            drain_until_quiet(&rx, Duration::from_millis(500), Duration::from_secs(3));
        ctx.raw_bytes(&out2);
        all.extend_from_slice(&out2);
        ctx.line(&format!(
            "after manual reply: first byte {:?}s, {} bytes, hex={}",
            times2.first().copied().unwrap_or(-1.0),
            out2.len(),
            hex(&out2)
        ));
        for b in &out2 {
            term.vt_write(std::slice::from_ref(b));
        }
    } else {
        ctx.line("ESC[6n not present in startup; no-reply probe skipped");
    }
    let cb_len = {
        let f = ctx.cb_log.lock().expect("cb lock");
        f.metadata().map(|m| m.len()).unwrap_or(0)
    };
    ctx.line(&format!(
        "on_pty_write/on_device_attributes callback log bytes so far: {}",
        cb_len
    ));

    // (d)(g) chcp + echo with CJK
    for c in cmds {
        do_write(&mut ctx, &writer, c.as_bytes(), "cmd");
        let (out, _) = drain_until_quiet(&rx, Duration::from_millis(800), Duration::from_secs(6));
        ctx.raw_bytes(&out);
        all.extend_from_slice(&out);
        ctx.line(&format!(
            "after cmd {:?}: PTY output {} bytes hex={}",
            c,
            out.len(),
            hex(&out)
        ));
        for b in &out {
            term.vt_write(std::slice::from_ref(b));
        }
        let gt = grid_text(&term);
        ctx.line(&format!("after cmd {:?} grid text: {:?}", c, gt));
        if c.contains("你好") {
            let rows_cells = all_rows_cells(&term);
            let hit = rows_cells
                .iter()
                .position(|r| r.iter().any(|(s, _)| s.contains('你')));
            match hit {
                None => ctx.line(&format!(
                    "ASSERT FAIL: grid has no 你; grid={:?}",
                    gt
                )),
                Some(ri) => {
                    let row = &rows_cells[ri];
                    let pi = row.iter().position(|(s, _)| s == "你").expect("你 exists");
                    assert_eq!(row[pi].1, CellWide::Wide, "你 must be Wide");
                    assert_eq!(row[pi + 1].1, CellWide::SpacerTail, "cell after 你");
                    assert_eq!(row[pi + 2].0, "好", "好 follows");
                    assert_eq!(row[pi + 2].1, CellWide::Wide, "好 must be Wide");
                    assert_eq!(row[pi + 3].1, CellWide::SpacerTail, "cell after 好");
                    ctx.line(&format!(
                        "ASSERT PASS: row {} has 你/好 each 2 cells with SpacerTail",
                        ri
                    ));
                }
            }
        }
    }

    // (e) resize 80x24 -> 120x40
    master
        .resize(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty resize");
    term.resize(120, 40, 8, 16).expect("Terminal::resize");
    ctx.line(&format!(
        "after resize Terminal: cols={} rows={}",
        term.cols().expect("cols"),
        term.rows().expect("rows")
    ));
    assert_eq!(term.cols().expect("cols"), 120);
    assert_eq!(term.rows().expect("rows"), 40);
    let (out, _) = drain_until_quiet(&rx, Duration::from_millis(600), Duration::from_secs(4));
    ctx.raw_bytes(&out);
    all.extend_from_slice(&out);
    ctx.line(&format!(
        "post-resize PTY redraw bytes: {} hex={}",
        out.len(),
        hex(&out)
    ));
    for b in &out {
        term.vt_write(std::slice::from_ref(b));
    }
    do_write(&mut ctx, &writer, b"mode\r", "mode-cmd");
    let (mode_out, _) = drain_until_quiet(&rx, Duration::from_millis(600), Duration::from_secs(4));
    ctx.raw_bytes(&mode_out);
    all.extend_from_slice(&mode_out);
    let mode_text = String::from_utf8_lossy(&mode_out).to_string();
    ctx.line(&format!("mode output text: {:?}", mode_text));
    for b in &mode_out {
        term.vt_write(std::slice::from_ref(b));
    }
    assert!(
        mode_text.contains("120") && mode_text.contains("40"),
        "PTY side mode output must contain 120/40, got {:?}",
        mode_text
    );
    ctx.line("ASSERT PASS: Terminal and PTY sides agree 120x40");

    // (f) exit: EOF + drop(master)/wait ordering
    if drop_master_first {
        ctx.line("exit order A: drop(master) -> wait(child)");
        drop(master);
        let wait_t0 = Instant::now();
        let (tx2, rx2) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx2.send(child.wait());
        });
        match rx2.recv_timeout(Duration::from_secs(15)) {
            Ok(s) => ctx.line(&format!(
                "drop-master-first: wait returned {:?} after {:?}",
                s,
                wait_t0.elapsed()
            )),
            Err(_) => ctx.line("drop-master-first: wait still pending after 15s (stuck)"),
        }
    } else {
        do_write(&mut ctx, &writer, exit_cmd.as_bytes(), "exit");
        let wait_t0 = Instant::now();
        let (tx2, rx2) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx2.send(child.wait());
        });
        let mut eof_at: Option<Duration> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Msg::Eof) => {
                    eof_at = Some(t0.elapsed());
                    ctx.line(&format!("reader thread EOF at {:?}", eof_at.unwrap()));
                    break;
                }
                Ok(Msg::Data(d)) => {
                    ctx.raw_bytes(&d);
                    all.extend_from_slice(&d);
                    for b in &d {
                        term.vt_write(std::slice::from_ref(b));
                    }
                }
                Err(_) => {}
            }
            if t0.elapsed() > Duration::from_secs(15) {
                ctx.line("EOF wait timeout 15s");
                break;
            }
        }
        match rx2.recv_timeout(Duration::from_secs(15)) {
            Ok(s) => ctx.line(&format!(
                "after exit cmd: wait returned {:?} after {:?}",
                s,
                wait_t0.elapsed()
            )),
            Err(_) => ctx.line("after exit cmd: wait still pending after 15s (stuck)"),
        }
    }
    // recorded platform behavior: after child exit the ConPTY master reader
    // does NOT return EOF promptly - observed EOF latency ~83s post-exit on
    // Windows ConPTY via portable-pty 0.8.1 (join blocked that long, then
    // returned). Wait up to 120s so the test stays bounded either way.
    let (txj, rxj) = mpsc::channel::<()>();
    let jh = std::thread::spawn(move || {
        let _ = reader_thread.join();
        let _ = txj.send(());
    });
    match rxj.recv_timeout(Duration::from_secs(120)) {
        Ok(_) => ctx.line("reader thread join returned (EOF observed after the long latency)"),
        Err(_) => ctx.line("reader thread join still pending after 120s; detaching watcher"),
    }
    drop(jh);
    ctx.line(&format!("total elapsed {:?}", t0.elapsed()));
    let _ = all;
}

#[test]
fn w5_conpty_cmd_round() {
    run_round(
        "cmd",
        "cmd.exe",
        &[],
        "C:\\vt",
        &["chcp\r", "echo 你好\r", "chcp 65001\r", "echo 你好\r"],
        true,
        "exit\r",
        false,
    );
}

#[test]
fn w5_conpty_powershell_round() {
    run_round(
        "powershell",
        "powershell.exe",
        &["-NoLogo"],
        "C:\\vt",
        &[
            "[Console]::OutputEncoding\r",
            "chcp 65001\r",
            "echo 你好\r",
        ],
        true,
        "exit\r",
        true,
    );
}
