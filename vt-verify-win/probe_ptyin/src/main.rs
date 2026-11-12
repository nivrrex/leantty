// Writes a byte sequence into a ConPTY master and reports what the child
// (raw-mode reader) actually received. Isolates the ConPTY input path from
// the app: does ESC[200~ ... ESC[201~ survive to the child?
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: ptyin <program> [args...]  env PROBE_SEQ=hex");
        return;
    }
    let program = args.remove(0);
    let seq_hex = std::env::var("PROBE_SEQ").unwrap_or_else(|_| "1b5b3230307e41421b5b3230317e".to_string());
    let seq: Vec<u8> = (0..seq_hex.len() / 2)
        .map(|i| u8::from_str_radix(&seq_hex[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    let sym = std::env::var("PROBE_SYM").unwrap_or_default().is_empty();

    let pair = native_pty_system()
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .expect("openpty");
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
    let t0 = Instant::now();
    // wait for READY
    while t0.elapsed() < Duration::from_secs(8) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                all.extend_from_slice(&chunk);
                if all.windows(5).any(|w| w == b"READY") {
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    std::thread::sleep(Duration::from_millis(400));
    if let Err(e) = writer.write_all(&seq).and_then(|_| writer.flush()) {
        eprintln!("write failed: {}", e);
    }
    if sym {
        eprintln!("wrote {} bytes", seq.len());
    }
    // collect the child's report (3 s of silence ends it)
    let t1 = Instant::now();
    while t1.elapsed() < Duration::from_secs(6) {
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(chunk) => {
                all.extend_from_slice(&chunk);
            }
            Err(_) => {}
        }
    }
    let _ = child.kill();
    let text = String::from_utf8_lossy(&all);
    println!("--- child output ---");
    println!("{}", text.replace('\r', ""));
    println!("--- end ---");
    println!("raw_hex={}", all.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""));
}
