// Reads raw bytes from the console input handle (line input disabled) and
// reports how many arrived plus the head/tail bytes in hex.
// Emits ESC[?2004h first so the terminal enables bracketed paste.
use std::io::Write;

#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    lp_security_descriptor: *mut core::ffi::c_void,
    b_inherit_handle: i32,
}

extern "system" {
    fn GetStdHandle(which: u32) -> *mut core::ffi::c_void;
    fn SetConsoleMode(handle: *mut core::ffi::c_void, mode: u32) -> i32;
    fn ReadFile(
        handle: *mut core::ffi::c_void,
        buf: *mut u8,
        to_read: u32,
        read: *mut u32,
        overlapped: *mut core::ffi::c_void,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut core::ffi::c_void, ms: u32) -> u32;
}

const STD_INPUT: u32 = 0xFFFF_FFF6; // (DWORD)-10
const WAIT_TIMEOUT: u32 = 0x102;
const WAIT_OBJECT_0: u32 = 0;

fn hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push('-');
        }
        s.push_str(&format!("{:02X}", b));
    }
    s
}

fn main() {
    let hin = unsafe { GetStdHandle(STD_INPUT) };
    // 0 = raw: no line input, no echo, no processed input
    let ok = unsafe { SetConsoleMode(hin, 0) };
    println!("MODE={}", ok);
    // turn on bracketed paste so the terminal wraps pasted text
    print!("\x1b[?2004h");
    // optional: enable SGR mouse reporting when invoked with the "mouse" arg
    if std::env::args().any(|a| a == "mouse") {
        print!("\x1b[?1000h\x1b[?1006h");
    }
    if std::env::args().any(|a| a == "m1002") {
        print!("\x1b[?1002h\x1b[?1006h");
    }
    if std::env::args().any(|a| a == "m1003") {
        print!("\x1b[?1003h\x1b[?1006h");
    }
    let _ = std::io::stdout().flush();
    println!("READY");
    let _ = std::io::stdout().flush();

    let mut total = 0usize;
    let mut head: Vec<u8> = Vec::new();
    let mut tail: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let w = unsafe { WaitForSingleObject(hin, 15000) };
        if w != WAIT_OBJECT_0 {
            if w == WAIT_TIMEOUT {
                break;
            }
            break;
        }
        let mut n: u32 = 0;
        let r = unsafe { ReadFile(hin, buf.as_mut_ptr(), buf.len() as u32, &mut n, core::ptr::null_mut()) };
        if r == 0 || n == 0 {
            break;
        }
        let chunk = &buf[..n as usize];
        if head.len() < 16 {
            let take = (16 - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
        }
        tail.extend_from_slice(chunk);
        if tail.len() > 16 {
            let cut = tail.len() - 16;
            tail.drain(..cut);
        }
        total += n as usize;
        // incremental report: the reader can block on the next read, so the
        // e2e must be able to assert on the latest values at any time
        println!();
        println!("BYTES={}", total);
        println!("FIRSTHEX={}", hex(&head));
        println!("LASTHEX={}", hex(&tail));
        let _ = std::io::stdout().flush();
    }
    // restore line input mode before printing the report
    unsafe { SetConsoleMode(hin, 0x0001 | 0x0002 | 0x0004) };
    println!();
    println!("BYTES={}", total);
    println!("FIRSTHEX={}", hex(&head));
    println!("LASTHEX={}", hex(&tail));
}
