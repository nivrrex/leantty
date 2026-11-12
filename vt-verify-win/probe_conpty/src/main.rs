// Minimal ConPTY host: creates the pseudo console with explicit flags so the
// portable-pty hardcoded mask can be compared against alternatives.
// usage: conpty <flags-hex> <cols> <rows> <program> [args...]
// env: PROBE_RESIZE=1  -> resize to (cols+40, rows+10) after 4 s
//      PROBE_REPLY=1   -> answer ESC[6n with ESC[1;1R
//      PROBE_SECS=n    -> observation window (default 10)
use std::ffi::{c_void, OsStr};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::sync::mpsc;
use std::time::{Duration, Instant};

type Handle = *mut c_void;
type Hpcon = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Coord {
    x: i16,
    y: i16,
}

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: Handle,
    h_std_output: Handle,
    h_std_error: Handle,
}

#[repr(C)]
struct StartupInfoExW {
    startup_info: StartupInfoW,
    lp_attribute_list: *mut c_void,
}

#[repr(C)]
struct ProcessInformation {
    h_process: Handle,
    h_thread: Handle,
    dw_process_id: u32,
    dw_thread_id: u32,
}

#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    lp_security_descriptor: *mut c_void,
    b_inherit_handle: i32,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreatePipe(read: *mut Handle, write: *mut Handle, sa: *mut SecurityAttributes, size: u32) -> i32;
    fn SetHandleInformation(h: Handle, mask: u32, flags: u32) -> i32;
    fn CloseHandle(h: Handle) -> i32;
    fn InitializeProcThreadAttributeList(list: *mut c_void, count: u32, flags: u32, size: *mut usize) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *mut c_void,
        size: usize,
        prev: *mut c_void,
        ret_size: *mut usize,
    ) -> i32;
    fn CreateProcessW(
        app: *const u16,
        cmd: *mut u16,
        pa: *mut c_void,
        ta: *mut c_void,
        inherit: i32,
        flags: u32,
        env: *mut c_void,
        dir: *const u16,
        si: *mut StartupInfoExW,
        pi: *mut ProcessInformation,
    ) -> i32;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn GetExitCodeProcess(h: Handle, code: *mut u32) -> i32;
    fn TerminateProcess(h: Handle, code: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn CreatePseudoConsole(size: Coord, input: Handle, output: Handle, flags: u32, hpcon: *mut Hpcon) -> i32;
    fn ResizePseudoConsole(hpcon: Hpcon, size: Coord) -> i32;
    fn ClosePseudoConsole(hpcon: Hpcon);
}

const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: conpty <flags-hex> <cols> <rows> <program> [args...]");
        return;
    }
    let flags = u32::from_str_radix(args[0].trim_start_matches("0x"), 16).unwrap_or(0);
    let cols: i16 = args[1].parse().unwrap_or(120);
    let rows: i16 = args[2].parse().unwrap_or(30);
    let program = args[3].clone();
    let child_args = &args[4..];
    let resize = std::env::var("PROBE_RESIZE").is_ok();
    let reply = std::env::var("PROBE_REPLY").is_ok();
    let secs: u64 = std::env::var("PROBE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let mut sa = SecurityAttributes { n_length: std::mem::size_of::<SecurityAttributes>() as u32, lp_security_descriptor: std::ptr::null_mut(), b_inherit_handle: 1 };
    let (mut in_read, mut in_write): (Handle, Handle) = (std::ptr::null_mut(), std::ptr::null_mut());
    let (mut out_read, mut out_write): (Handle, Handle) = (std::ptr::null_mut(), std::ptr::null_mut());
    unsafe {
        CreatePipe(&mut in_read, &mut in_write, &mut sa, 0);
        CreatePipe(&mut out_read, &mut out_write, &mut sa, 0);
        // keep both pipes inheritable: the child receives them through the
        // pseudoconsole attribute, and CreateProcess is called with inherit=1
        SetHandleInformation(in_write, 1, 1);
        SetHandleInformation(out_read, 1, 1);
    }

    let mut hpcon: Hpcon = std::ptr::null_mut();
    let hr = unsafe { CreatePseudoConsole(Coord { x: cols, y: rows }, in_read, out_write, flags, &mut hpcon) };
    eprintln!("CreatePseudoConsole hr=0x{:08x} flags=0x{:x} size={}x{}", hr, flags, cols, rows);
    if hr != 0 {
        eprintln!("RESULT=fail-ctor");
        return;
    }

    let mut size = {
        let mut n: usize = 0;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut n) };
        n
    };
    let list = unsafe { std::alloc::alloc(std::alloc::Layout::from_size_align(size, 16).unwrap()) } as *mut c_void;
    unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) };
    let ok = unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            (&mut hpcon) as *mut Hpcon as *mut c_void,
            std::mem::size_of::<Hpcon>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        eprintln!("UpdateProcThreadAttribute failed: {}", std::io::Error::last_os_error());
        eprintln!("RESULT=fail-attr");
        return;
    }

    eprintln!(
        "SIZES startupinfo={} startupinfoex={} coord={} pi={}",
        std::mem::size_of::<StartupInfoW>(),
        std::mem::size_of::<StartupInfoExW>(),
        std::mem::size_of::<Coord>(),
        std::mem::size_of::<ProcessInformation>()
    );
    let mut si: StartupInfoExW = unsafe { std::mem::zeroed() };
    si.startup_info.cb = std::mem::size_of::<StartupInfoExW>() as u32;
    si.lp_attribute_list = list;

    let mut cmdline: Vec<u16> = Vec::new();
    // program path may contain spaces: CreateProcessW parses argv[0] from the
    // command line when lpApplicationName is NULL
    cmdline.push('"' as u16);
    cmdline.extend(wide(&program));
    cmdline.push('"' as u16);
    for a in child_args {
        cmdline.push(' ' as u16);
        cmdline.extend(wide(a));
    }
    cmdline.push(0);

    let mut pi: ProcessInformation = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessW(
            wide(&program).as_ptr(),
            cmdline.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            1,
            EXTENDED_STARTUPINFO_PRESENT,
            std::ptr::null_mut(),
            std::ptr::null(),
            &mut si,
            &mut pi,
        )
    };
    if created == 0 {
        eprintln!("CreateProcessW failed: {}", std::io::Error::last_os_error());
        eprintln!("RESULT=fail-spawn");
        return;
    }
    unsafe {
        CloseHandle(pi.h_thread);
    }

    // reader thread on the output pipe
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let read_addr = out_read as usize;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            let mut n: u32 = 0;
            let ok = unsafe {
                ReadFile(read_addr as Handle, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut())
            };
            if ok == 0 || n == 0 {
                break;
            }
            if tx.send(buf[..n as usize].to_vec()).is_err() {
                break;
            }
        }
    });

    let write_addr = in_write as usize;
    let mut all: Vec<u8> = Vec::new();
    let mut replied = false;
    let t0 = Instant::now();
    let mut resized = false;
    while t0.elapsed() < Duration::from_secs(secs) {
        if resize && !resized && t0.elapsed() > Duration::from_secs(4) {
            let hr = unsafe { ResizePseudoConsole(hpcon, Coord { x: cols + 40, y: rows + 10 }) };
            eprintln!("ResizePseudoConsole hr=0x{:08x} -> {}x{}", hr, cols + 40, rows + 10);
            resized = true;
        }
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(chunk) => {
                all.extend_from_slice(&chunk);
                if reply && !replied && all.windows(4).any(|w| w == b"\x1b[6n") {
                    let mut n: u32 = 0;
                    let seq = b"\x1b[1;1R";
                    unsafe {
                        WriteFile(write_addr as Handle, seq.as_ptr(), seq.len() as u32, &mut n, std::ptr::null_mut());
                    }
                    eprintln!("REPLY ESC[1;1R after {} bytes", all.len());
                    replied = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        }
    }

    // terminate and harvest exit code
    let mut code: u32 = 0;
    let alive = unsafe { WaitForSingleObject(pi.h_process, 0) } != 0; // WAIT_TIMEOUT means alive
    if alive {
        unsafe { TerminateProcess(pi.h_process, 1) };
        unsafe { WaitForSingleObject(pi.h_process, 2000) };
    }
    unsafe { GetExitCodeProcess(pi.h_process, &mut code) };

    let text = String::from_utf8_lossy(&all).to_string();
    let has_total_width = text.contains("totalWidth");
    let has_6n = all.windows(4).any(|w| w == b"\x1b[6n");
    let has_failfast = text.contains("Process terminated") || text.contains("FailFast");
    eprintln!(
        "bytes={} exit=0x{:08x} alive_at_end={} esc6n={} totalWidth={} failfast={}",
        all.len(),
        code,
        alive,
        has_6n,
        has_total_width,
        has_failfast
    );
    eprintln!("first120_hex={}", all.iter().take(120).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""));
    let verdict = if has_total_width || has_failfast || (!alive && code == 0xC000_013A) {
        "CRASH"
    } else if !alive {
        "clean-exit"
    } else {
        "alive"
    };
    eprintln!("RESULT={}", verdict);
    unsafe {
        ClosePseudoConsole(hpcon);
        CloseHandle(pi.h_process);
        CloseHandle(in_write);
        CloseHandle(out_read);
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn ReadFile(h: Handle, buf: *mut u8, to_read: u32, read: *mut u32, ov: *mut c_void) -> i32;
    fn WriteFile(h: Handle, buf: *const u8, to_write: u32, written: *mut u32, ov: *mut c_void) -> i32;
}
