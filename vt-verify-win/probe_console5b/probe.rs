// 5B t4a 探针：控制台相关 Win32 行为记录（结果写文件，不依赖 stdout）
use std::io::Write;
use std::ptr;
use std::time::Duration;

#[repr(C)]
struct RECT { l: i32, t: i32, r: i32, b: i32 }

unsafe extern "system" {
    fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
    fn AttachConsole(pid: u32) -> i32;
    fn FreeConsole() -> i32;
    fn GetCurrentProcessId() -> u32;
    fn EnumWindows(f: extern "system" fn(isize, isize) -> i32, l: isize) -> i32;
    fn GetWindowThreadProcessId(h: isize, pid: *mut u32) -> u32;
    fn GetClassNameW(h: isize, buf: *mut u16, n: i32) -> i32;
    fn IsWindowVisible(h: isize) -> i32;
    fn GetWindowRect(h: isize, r: *mut RECT) -> i32;
    fn GetLastError() -> u32;
}

static mut VISIBLE_CONSOLE: u32 = 0;
static mut SELF_PID: u32 = 0;

extern "system" fn on_window(h: isize, _l: isize) -> i32 {
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(h, &mut pid);
        if pid != SELF_PID {
            return 1;
        }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(h, buf.as_mut_ptr(), 64);
        let cls = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        if cls == "ConsoleWindowClass" && IsWindowVisible(h) != 0 {
            VISIBLE_CONSOLE += 1;
            let mut r = RECT { l: 0, t: 0, r: 0, b: 0 };
            GetWindowRect(h, &mut r);
            LOGLINE(&format!("    console_window hwnd={h} rect=({},{})-({},{})", r.l, r.t, r.r, r.b));
        }
        1
    }
}

fn count_console_windows() -> u32 {
    unsafe {
        VISIBLE_CONSOLE = 0;
        EnumWindows(on_window, 0);
        VISIBLE_CONSOLE
    }
}

static mut LOG: Option<std::fs::File> = None;

fn LOGLINE(s: &str) {
    unsafe {
        if let Some(f) = LOG.as_mut() {
            let _ = writeln!(f, "{s}");
            let _ = f.flush();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let log_path = args.get(1).cloned().unwrap_or_else(|| "probe.log".into());
    let mode = args.get(2).cloned().unwrap_or_else(|| "unknown".into());
    let f = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).unwrap();
    unsafe { LOG = Some(f) };
    unsafe { SELF_PID = GetCurrentProcessId() };
    LOGLINE(&format!("=== mode={mode} pid={} ===", unsafe { SELF_PID }));
    LOGLINE(&format!("pre: visible_console_windows={}", count_console_windows()));

    unsafe {
        let e0 = GetLastError();
        let n_null = GetConsoleProcessList(ptr::null_mut(), 0);
        let e1 = GetLastError();
        LOGLINE(&format!(
            "GetConsoleProcessList(null,0) -> {n_null} (GetLastError before={e0} after={e1})"
        ));
        let mut buf = [0u32; 8];
        let e2 = GetLastError();
        let n_buf = GetConsoleProcessList(buf.as_mut_ptr(), 8);
        let e3 = GetLastError();
        let pids: Vec<u32> = buf[..(n_buf.min(8) as usize)].to_vec();
        LOGLINE(&format!(
            "GetConsoleProcessList(buf,8) -> {n_buf} pids={pids:?} (GetLastError before={e2} after={e3})"
        ));
        let e4 = GetLastError();
        let free_rc = FreeConsole();
        let e5 = GetLastError();
        LOGLINE(&format!("FreeConsole() -> {free_rc} (GetLastError before={e4} after={e5})"));
    }
    std::thread::sleep(Duration::from_millis(1000));
    LOGLINE(&format!("post+1s: visible_console_windows={}", count_console_windows()));
    std::thread::sleep(Duration::from_millis(2000));
    LOGLINE(&format!("post+3s: visible_console_windows={}", count_console_windows()));
    unsafe {
        let e6 = GetLastError();
        let att = AttachConsole(SELF_PID);
        let e7 = GetLastError();
        LOGLINE(&format!("AttachConsole(self) after free -> {att} (GetLastError before={e6} after={e7})"));
        if att != 0 {
            let n_null = GetConsoleProcessList(ptr::null_mut(), 0);
            let mut buf = [0u32; 8];
            let n_buf = GetConsoleProcessList(buf.as_mut_ptr(), 8);
            let pids: Vec<u32> = buf[..(n_buf.min(8) as usize)].to_vec();
            LOGLINE(&format!("  reattached: null->{n_null} buf->{n_buf} pids={pids:?}"));
            FreeConsole();
        }
    }
    std::thread::sleep(Duration::from_millis(5000));
    LOGLINE(&format!("post+8s: visible_console_windows={}", count_console_windows()));
    LOGLINE("done");
}
