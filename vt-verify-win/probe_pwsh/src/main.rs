// Task-2 layer isolation: spawn a shell through portable-pty ConPTY at 120x30,
// capture raw output bytes (hex), detect ESC[6n startup query, optionally reply,
// detect pwsh FailFast markers, report exit status. No app window / no render.
// Usage: probe_pwsh <program> [args...]   env PROBE_REPLY=1 replies ESC[6n.
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: probe_pwsh <program> [args...] (env PROBE_REPLY=1 replies ESC[6n)");
        return;
    }
    let program = args.remove(0);
    let program_disp = program.clone();
    let reply = std::env::var("PROBE_REPLY").is_ok();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty 120x30");
    let mut cmd = CommandBuilder::new(program);
    for a in &args {
        cmd.arg(a);
    }
    let mut child = pair.slave.spawn_command(cmd).expect("spawn");
    drop(pair.slave);
    let mut writer = pair.master.take_writer().expect("writer");
    let mut reader = pair.master.try_clone_reader().expect("reader");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut c = [0u8; 1024];
        loop {
            match reader.read(&mut c) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(c[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut all: Vec<u8> = Vec::new();
    let mut esc6n_seen = false;
    let mut replied = false;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                all.extend_from_slice(&chunk);
                if all.windows(4).any(|w| w == b"\x1b[6n") {
                    esc6n_seen = true;
                    if reply && !replied {
                        writer.write_all(b"\x1b[1;1R").ok();
                        writer.flush().ok();
                        replied = true;
                        eprintln!("REPLY: sent ESC[1;1R after ESC[6n at {} bytes", all.len());
                    }
                }
            }
            Err(_) => continue,
        }
    }
    // Ctrl+C probe then final drain
    writer.write_all(b"\x03").ok();
    writer.flush().ok();
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(300)) {
        all.extend_from_slice(&chunk);
    }

    let exit = match child.try_wait() {
        Ok(Some(es)) => format!("exited {:?}", es),
        Ok(None) => {
            let _ = child.kill();
            "alive-at-10s-window (killed)".to_string()
        }
        Err(e) => format!("try_wait error: {}", e),
    };
    let text = String::from_utf8_lossy(&all);
    let failfast = text.contains("Process terminated")
        || text.contains("totalWidth")
        || text.contains("FailFast")
        || text.contains("Fail fast");
    eprintln!("program={} args={:?} reply={} replied={}", program_disp, args, reply, replied);
    eprintln!("total_bytes={} esc6n_seen={} failfast={}", all.len(), esc6n_seen, failfast);
    eprintln!("exit={}", exit);
    eprintln!("first96_hex={}", hexdump(&all[..all.len().min(96)]));
    for pat in ["totalWidth", "Process terminated", "FailFast", "\x1b[6n", "Windows PowerShell", "PowerShell 7"] {
        eprintln!("contains {:?} = {}", pat, text.contains(pat));
    }
}

fn hexdump(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ")
}
