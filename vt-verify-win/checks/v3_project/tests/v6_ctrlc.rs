//! W6 extra: Ctrl-C through ConPTY - does cmd.exe really interrupt a foreground
//! command (ping -t)? Records observed behavior only.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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

fn drain(
    rx: &mpsc::Receiver<Option<Vec<u8>>>,
    quiet: Duration,
    cap: Duration,
    collected: &mut Vec<u8>,
    raw: &mut File,
) -> Vec<f64> {
    let t0 = Instant::now();
    let mut times = Vec::new();
    let mut last = Instant::now();
    loop {
        if t0.elapsed() > cap {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Some(d)) => {
                times.push(t0.elapsed().as_secs_f64());
                collected.extend_from_slice(&d);
                let _ = raw.write_all(&d);
                last = Instant::now();
            }
            Ok(None) => break,
            Err(_) => {
                if last.elapsed() > quiet {
                    break;
                }
            }
        }
    }
    times
}

#[test]
fn w6_conpty_ctrl_c_interrupts_ping() {
    let log_path = "C:\\vt\\logs\\w6-ctrlc.log";
    let raw_path = "C:\\vt\\logs\\w6-ctrlc-raw.bin";
    let mut log = File::create(log_path).expect("log");
    let mut raw = File::create(raw_path).expect("raw");
    let t0 = Instant::now();
    let mut note = |m: &str| {
        use std::io::Write;
        let _ = writeln!(log, "[{:9.3}] {}", t0.elapsed().as_secs_f64(), m);
        let _ = log.flush();
        println!("[{:9.3}] {}", t0.elapsed().as_secs_f64(), m);
    };

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut cmd = CommandBuilder::new("cmd.exe");
    cmd.cwd("C:\\vt");
    let mut child = pair.slave.spawn_command(cmd).expect("spawn cmd");
    drop(pair.slave);
    let master = pair.master;
    let reader = master.try_clone_reader().expect("clone reader");
    let mut writer = master.take_writer().expect("take writer");

    let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
    let reader_thread = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(None);
                    break;
                }
                Err(_) => {
                    let _ = tx.send(None);
                    break;
                }
                Ok(n) => {
                    if tx.send(Some(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut collected: Vec<u8> = Vec::new();
    drain(&rx, Duration::from_millis(500), Duration::from_secs(4), &mut collected, &mut raw);
    note(&format!("startup output: {}", hex(&collected)));

    writer
        .write_all(b"ping -t 127.0.0.1\r")
        .expect("write ping");
    writer.flush().expect("flush");
    note("WRITE: ping -t 127.0.0.1");
    collected.clear();
    drain(&rx, Duration::from_millis(700), Duration::from_secs(6), &mut collected, &mut raw);
    let before = collected.clone();
    note(&format!("ping output: {} bytes: {}", before.len(), hex(&before)));
    assert!(
        before.len() > 10,
        "ping -t should produce output, got {} bytes",
        before.len()
    );

    let t_ctrl = Instant::now();
    writer.write_all(b"\x03").expect("write ctrl-c");
    writer.flush().expect("flush");
    note("WRITE: \\x03 (Ctrl-C)");
    collected.clear();
    drain(&rx, Duration::from_millis(700), Duration::from_secs(8), &mut collected, &mut raw);
    let after = collected.clone();
    note(&format!(
        "after Ctrl-C: {} bytes within {:?}: {}",
        after.len(),
        t_ctrl.elapsed(),
        hex(&after)
    ));
    let txt = String::from_utf8_lossy(&after).to_string();
    note(&format!("after Ctrl-C text: {:?}", txt));
    let has_ctrl_marker = txt.contains('^') || txt.contains("Ctrl") || txt.contains('\x03');
    note(&format!("output contains ^C/Ctrl marker: {}", has_ctrl_marker));

    writer
        .write_all(b"echo AFTER-CTRL-C\r")
        .expect("write echo");
    writer.flush().expect("flush");
    collected.clear();
    drain(&rx, Duration::from_millis(500), Duration::from_secs(5), &mut collected, &mut raw);
    let tail = collected.clone();
    let tail_txt = String::from_utf8_lossy(&tail).to_string();
    note(&format!("after Ctrl-C echo output text: {:?}", tail_txt));
    assert!(
        tail_txt.contains("AFTER-CTRL-C"),
        "shell should still be interactive after Ctrl-C, got {:?}",
        tail_txt
    );
    note(&format!(
        "VERDICT ping interrupted = {}",
        !tail_txt.contains("TTL") && !tail_txt.contains("ping")
    ));

    writer.write_all(b"exit\r").expect("write exit");
    writer.flush().expect("flush");
    let _ = rx.recv_timeout(Duration::from_secs(10));
    let status = child.wait();
    note(&format!("child.wait() = {:?}", status));
    drop(master);
    // recorded platform behavior: ConPTY master reader may not return EOF
    // after child exit within the observation window; detach instead of join.
    drop(reader_thread);
    note("reader thread detached (no join; see W5 notes)");
}
