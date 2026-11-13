//! pty/windows：ConPTY 实现契约与平台行为测试。
//!
//! T3：cmd / powershell 启动置 UTF-8 代码页（spawn 参数做法），echo 你好 入网格且汉字占 2 格。
//! T4：Ctrl-C 按规格顺序的分项实测（powershell ping -t → cmd 另一长命令 → 连写两次 \x03
//! → flush 确认 → portable-pty 写入路径 → app 层键盘映射）。
//! T5：on_pty_write 回写（喂 ESC[6n，断言回写字节到达 PTY 写端）。
//!
//! 门控：ConPTY 与 Windows API，Linux 上编译跳过（与 pty_unix.rs 的 #![cfg(unix)] 对称）。
#![cfg(windows)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use pty::{NativePty, Pty, windows::join_thread_timeout};
use vt::{Action, CellWide, Key, KeyEncoder, Mods, VtTerminal};

fn spawn_collect(
    shell: &str,
) -> (NativePty, mpsc::Receiver<Vec<u8>>, std::thread::JoinHandle<()>) {
    let mut pty = NativePty::new();
    pty.spawn(shell, 80, 24).expect("spawn");
    let reader = pty.take_reader().expect("take_reader");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let handle = std::thread::spawn(move || {
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
    (pty, rx, handle)
}

fn drain_quiet(rx: &mpsc::Receiver<Vec<u8>>, quiet: Duration, cap: Duration) -> Vec<u8> {
    let t0 = Instant::now();
    let mut out = Vec::new();
    let mut last = Instant::now();
    loop {
        if t0.elapsed() > cap {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(d) => {
                out.extend_from_slice(&d);
                last = Instant::now();
            }
            Err(_) => {
                if last.elapsed() > quiet {
                    break;
                }
            }
        }
    }
    out
}


/// 等待输出中出现 needle（总超时 timeout）；返回（累计输出, 是否命中）。
fn wait_for(rx: &mpsc::Receiver<Vec<u8>>, needle: &[u8], timeout: Duration) -> (Vec<u8>, bool) {
    let t0 = Instant::now();
    let mut out: Vec<u8> = Vec::new();
    while t0.elapsed() < timeout {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(d) => {
                out.extend_from_slice(&d);
                if needle.is_empty() || out.windows(needle.len()).any(|w| w == needle) {
                    return (out, true);
                }
            }
            Err(_) => {}
        }
    }
    (out, false)
}

/// 测试进程级的 Ctrl-C 处理隔离。
///
/// pty 的 Windows spawn 会调用 SetConsoleCtrlHandler(NULL, FALSE) 清除“忽略
/// Ctrl-C”标志（继承标志修复），该调用作用于**本进程**；写入 \x03 后若 Ctrl-C
/// 事件广播到本控制台进程组，测试进程自己就会被终止（0xC000013A）。
/// 这里在 spawn 之后把标志重新置回，只影响测试进程，不改 spawn 语义。
/// 环境变量 TTY_PTY_NO_REIGNORE=1 用于对照组（保留原行为）。
fn reignore_ctrl_c_in_test() {
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: *mut core::ffi::c_void, add: i32) -> i32;
    }
    if std::env::var_os("TTY_PTY_NO_REIGNORE").is_some() {
        return;
    }
    // SAFETY: NULL handler + add=1 是 Win32 文档化用法（忽略 Ctrl-C）。
    unsafe {
        SetConsoleCtrlHandler(std::ptr::null_mut(), 1);
    }
}

fn assert_wide_you(collected: &[u8]) {
    let mut term = VtTerminal::new(80, 24).expect("VtTerminal::new");
    term.feed(collected);
    let snap = term.snapshot().expect("snapshot");
    let wide = snap
        .cells
        .iter()
        .flatten()
        .find(|c| c.chars == "你")
        .expect("网格应含 你");
    assert_eq!(wide.wide, CellWide::Wide, "你 应占 2 格");
}

/// T3a：cmd.exe（spawn 参数 /K chcp 65001）echo 你好 入网格。
#[test]
fn pty_cmd_echo_cn_resize_close() {
    let (mut pty, rx, handle) = spawn_collect("cmd.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all("echo 你好\r".as_bytes()).expect("write echo");
        w.flush().expect("flush");
    }
    let (out, got_cn) = wait_for(&rx, "你好".as_bytes(), Duration::from_secs(20));
    assert!(
        got_cn && out.windows(6).any(|w| w == "你好".as_bytes()),
        "PTY 输出应含 你好，实际 {:?}",
        String::from_utf8_lossy(&out)
    );
    assert_wide_you(&out);

    pty.resize(120, 40).expect("pty resize");
    let mut term = VtTerminal::new(80, 24).expect("VtTerminal::new");
    term.feed(&out);
    term.resize(120, 40).expect("vt resize");
    let snap = term.snapshot().expect("snapshot after resize");
    assert_eq!(snap.cols, 120);
    assert_eq!(snap.rows, 40);

    let t0 = Instant::now();
    pty.close().expect("close");
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "close 耗时 {}ms，超过 5s 契约上限",
        t0.elapsed().as_millis()
    );
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
    pty.close().expect("close 幂等");
}

/// T3b：powershell.exe（-NoLogo -NoExit -Command chcp 65001）echo 你好 入网格。
#[test]
fn pty_powershell_echo_cn() {
    let (mut pty, rx, handle) = spawn_collect("powershell.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all("echo 你好\r".as_bytes()).expect("write echo");
        w.flush().expect("flush");
    }
    // 实测：ConPTY 下 powershell 冷启动在标题与提示符之间存在 >700ms 静默间隙，
    // 静窗 700ms 会在此处误判结束（已验证事实，t2 轮曾快于该阈值通过）。
    // 不用静默阈值：并发跑整套测试时 powershell 冷启动会超过任何固定静窗，
    // 改为等目标字节出现（总超时 20 s）。
    let (out, got) = wait_for(&rx, "你好".as_bytes(), Duration::from_secs(20));
    assert!(
        got,
        "powershell PTY 输出应含 你好，实际 {:?}",
        String::from_utf8_lossy(&out)
    );
    assert_wide_you(&out);
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
}

/// T3c：空 shell 回落默认 cmd.exe。
#[test]
fn pty_default_shell_is_cmd() {
    let (mut pty, rx, handle) = spawn_collect("");
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"echo default-ok\r").expect("write");
        w.flush().expect("flush");
    }
    let (out, got) = wait_for(&rx, b"default-ok", Duration::from_secs(20));
    assert!(
        got && out.windows(10).any(|w| w == b"default-ok"),
        "默认 cmd.exe 应回显 default-ok，实际 {:?}",
        String::from_utf8_lossy(&out)
    );
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
}

/// T5：喂入 ESC[6n，on_pty_write 回写字节出现在 PTY 写端。
#[test]
fn pty_on_pty_write_dsr_response() {
    let (mut pty, _rx, handle) = spawn_collect("cmd.exe");
    let mut term = VtTerminal::new(80, 24).expect("VtTerminal::new");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    term.on_pty_write(move |_t, data| {
        let _ = tx.send(data.to_vec());
    })
    .expect("on_pty_write");
    term.feed(b"\x1b[6n");
    let resp = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("on_pty_write should emit DSR response");
    assert!(
        resp.starts_with(b"\x1b[") && resp.ends_with(b"R") && resp.contains(&b';'),
        "DSR 响应应为 ESC[r;cR，实际 {:?}",
        resp
    );
    // 回写字节接到 PTY 写端（与 app 接线同路径）。
    {
        let w = pty.writer().expect("writer");
        w.write_all(&resp).expect("pty write of DSR response");
        w.flush().expect("flush");
    }
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
}

/// T4 步骤 6：app 层键盘映射在 Windows 同样产出 \x03（KeyEncoder 与平台无关，
/// 同 vendored 库；本测试确认 Windows 上该路径字节一致）。
#[test]
fn key_encoder_ctrl_c_maps_to_0x03() {
    let term = VtTerminal::new(80, 24).expect("VtTerminal::new");
    let mut enc: KeyEncoder = term.key_encoder().expect("key_encoder");
    let bytes = enc
        .encode_key(Key::C, Mods::CTRL, Action::Press)
        .expect("encode Ctrl+C");
    assert_eq!(bytes, b"\x03", "Ctrl+C 应映射为 \\x03");
}

/// T4 步骤 1：powershell 下 ping -t → 首个 TTL → 单次 \x03 → 断言 TTL 停止。
///
/// 等待条件不用“静默 N ms”：powershell 冷启动约 1.2 s、ping 的 TTL 间隔约 1 s，
/// 都比旧的 800 ms 静默阈值长，旧写法会在拿到 TTL 之前就返回。
#[test]
fn ctrl_c_probe_powershell_ping() {
    let (mut pty, rx, handle) = spawn_collect("powershell.exe");
    reignore_ctrl_c_in_test();
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"ping.exe -t 127.0.0.1\r").expect("write ping");
        w.flush().expect("flush");
    }
    let (pre, got_ttl) = wait_for(&rx, b"TTL=", Duration::from_secs(15));
    assert!(
        got_ttl,
        "powershell ping -t 应在 15 s 内出现 TTL，实际 {:?}",
        String::from_utf8_lossy(&pre)
    );
    let w = pty.writer().expect("writer");
    w.write_all(b"\x03").expect("write ctrl-c");
    w.flush().expect("flush after ctrl-c");
    // 给中断一点时间落地：收集到出现提示符或 6 s 为止
    let (post, _) = wait_for(&rx, b"PS ", Duration::from_secs(6));
    // 断言中断生效：提示符之后不得再有 TTL 输出
    let text = String::from_utf8_lossy(&post).to_string();
    let after_prompt = match text.rfind("PS ") {
        Some(i) => &text[i..],
        None => text.as_str(),
    };
    println!(
        "[powershell ping] post-ctrl-c {} bytes, prompt_seen={} | tail: {:?}",
        post.len(),
        text.contains("PS "),
        after_prompt.chars().take(120).collect::<String>()
    );
    assert!(
        !after_prompt.contains("TTL="),
        "Ctrl-C 之后不应再有 TTL 输出，实际 {:?}",
        after_prompt
    );
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
}

/// T4 步骤 2：cmd 下换另一种长时间前台命令（sort 读 stdin）+ 单次 \x03。
#[test]
fn ctrl_c_probe_cmd_sort() {
    let (mut pty, rx, handle) = spawn_collect("cmd.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"sort\r").expect("write sort");
        w.flush().expect("flush");
    }
    let _pre = drain_quiet(&rx, Duration::from_millis(800), Duration::from_secs(4));
    let w = pty.writer().expect("writer");
    w.write_all(b"\x03").expect("write ctrl-c");
    w.flush().expect("flush after ctrl-c");
    let post = drain_quiet(&rx, Duration::from_millis(1000), Duration::from_secs(4));
    // sort 被中断后应回到提示符；再发 echo 验证外壳可交互。
    let w2 = pty.writer().expect("writer");
    w2.write_all(b"echo SORTDONE\r").expect("write echo");
    w2.flush().expect("flush echo");
    let tail = drain_quiet(&rx, Duration::from_millis(700), Duration::from_secs(4));
    let tail_text = String::from_utf8_lossy(&tail).to_string();
    println!(
        "[cmd sort] post-ctrl-c {} bytes: {:?} | echo marker visible: {}",
        post.len(),
        String::from_utf8_lossy(&post),
        tail_text.contains("SORTDONE")
    );
    assert!(
        tail_text.contains("SORTDONE"),
        "sort 被 Ctrl-C 中断后外壳应可执行 echo SORTDONE，实际 {:?}",
        tail_text
    );
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
}

/// T4 步骤 3+4：cmd ping -t 连写两次 \x03，且每次写后显式 flush 确认无缓冲延迟。
#[test]
fn ctrl_c_probe_cmd_ping_twice() {
    let (mut pty, rx, handle) = spawn_collect("cmd.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"ping -t 127.0.0.1\r").expect("write ping");
        w.flush().expect("flush");
    }
    let (pre, got_ttl) = wait_for(&rx, b"TTL=", Duration::from_secs(15));
    assert!(
        got_ttl && pre.windows(4).any(|w| w == b"TTL="),
        "cmd ping -t 应有 TTL 输出，实际 {:?}",
        String::from_utf8_lossy(&pre)
    );
    let w = pty.writer().expect("writer");
    w.write_all(b"\x03").expect("write ctrl-c #1");
    w.flush().expect("flush #1");
    w.write_all(b"\x03").expect("write ctrl-c #2");
    w.flush().expect("flush #2");
    let mid = drain_quiet(&rx, Duration::from_millis(1200), Duration::from_secs(5));
    println!(
        "[cmd ping twice] post-ctrl-c {} bytes: {:?} | TTL still: {}",
        mid.len(),
        String::from_utf8_lossy(&mid),
        mid.windows(4).any(|w| w == b"TTL=")
    );
    let w2 = pty.writer().expect("writer");
    w2.write_all(b"echo TWICEDONE\r").expect("write echo");
    w2.flush().expect("flush echo");
    let tail = drain_quiet(&rx, Duration::from_millis(700), Duration::from_secs(4));
    let tail_text = String::from_utf8_lossy(&tail).to_string();
    println!(
        "[cmd ping twice] echo marker visible: {} | tail: {:?}",
        tail_text.contains("TWICEDONE"),
        tail_text
    );
    // 未解决（已验证事实）：连写两次 \\x03 后 ping -t 仍继续，TWICEDONE 不出现。
    // 按阶段规格，此项以记录代替断言，不推断机制。
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("reader join within timeout");
}

/// T4 追加探针：数据落 target/t4-probes.log（防 stdout 丢失），只记录不断言。
fn probe_note(tag: &str, msg: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all("target");
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open("target/t4-probes.log")
    {
        let _ = writeln!(f, "[{}] {}", tag, msg);
    }
}

fn ctrl_c_probe(shell: &str, cmd: &str, ctrl: &[u8], tag: &str, post_cap: Duration) {
    let (mut pty, rx, handle) = spawn_collect(shell);
    if !cmd.is_empty() {
        let w = pty.writer().expect("writer");
        w.write_all(cmd.as_bytes()).expect("write cmd");
        w.flush().expect("flush cmd");
    }
    let pre = drain_quiet(&rx, Duration::from_millis(800), Duration::from_secs(6));
    {
        let w = pty.writer().expect("writer");
        w.write_all(ctrl).expect("write ctrl");
        w.flush().expect("flush ctrl");
    }
    let post = drain_quiet(&rx, Duration::from_millis(1500), post_cap);
    probe_note(
        tag,
        &format!(
            "pre={} post={} {:?}",
            pre.len(),
            post.len(),
            String::from_utf8_lossy(&post)
        ),
    );
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("join");
}

/// E1：cmd 空闲提示符下写 \x03（事件是否生成）。
#[test]
fn t4_e1_cmd_prompt_ctrl_c() {
    ctrl_c_probe("cmd.exe", "", b"\x03", "E1-cmd-idle", Duration::from_secs(5));
}

/// E2：cmd ping -t + \x03，10s 观察窗。
#[test]
fn t4_e2_cmd_ping_ctrl_c() {
    ctrl_c_probe(
        "cmd.exe",
        "ping -t 127.0.0.1\r",
        b"\x03",
        "E2-cmd-ping",
        Duration::from_secs(10),
    );
}

/// E3：cmd ping -n 5（有限次）+ \x03。
#[test]
fn t4_e3_cmd_ping_finite_ctrl_c() {
    ctrl_c_probe(
        "cmd.exe",
        "ping -n 5 127.0.0.1\r",
        b"\x03",
        "E3-cmd-ping-n5",
        Duration::from_secs(10),
    );
}

/// E4：cmd ping -t + \x03\r。
#[test]
fn t4_e4_cmd_ping_ctrl_c_cr() {
    ctrl_c_probe(
        "cmd.exe",
        "ping -t 127.0.0.1\r",
        b"\x03\r",
        "E4-cmd-ping-cr",
        Duration::from_secs(10),
    );
}

/// E5：powershell ping -t + \x03（重测，数据落文件）。
#[test]
fn t4_e5_powershell_ping_ctrl_c() {
    ctrl_c_probe(
        "powershell.exe",
        "ping.exe -t 127.0.0.1\r",
        b"\x03",
        "E5-ps-ping",
        Duration::from_secs(10),
    );
}

/// E6：cmd ping -t + \x1c（Ctrl+Break）。
#[test]
fn t4_e6_cmd_ping_ctrl_break() {
    ctrl_c_probe(
        "cmd.exe",
        "ping -t 127.0.0.1\r",
        b"\x1c",
        "E6-cmd-ping-break",
        Duration::from_secs(10),
    );
}

/// E7：cmd ping -t + \x03 间隔 1s 连写两次。
#[test]
fn t4_e7_cmd_ping_ctrl_c_spaced() {
    let (mut pty, rx, handle) = spawn_collect("cmd.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"ping -t 127.0.0.1\r").expect("write ping");
        w.flush().expect("flush");
    }
    let _pre = drain_quiet(&rx, Duration::from_millis(800), Duration::from_secs(6));
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"\x03").expect("ctrl #1");
        w.flush().expect("flush #1");
    }
    std::thread::sleep(Duration::from_secs(1));
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"\x03").expect("ctrl #2");
        w.flush().expect("flush #2");
    }
    let post = drain_quiet(&rx, Duration::from_millis(1500), Duration::from_secs(10));
    probe_note(
        "E7-cmd-ping-spaced",
        &format!("post={} {:?}", post.len(), String::from_utf8_lossy(&post)),
    );
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("join");
}

// E8：FFI 机制验证——AttachConsole 到 ping 进程所在控制台 +
// GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0)，验证 ping -t 是否可中断。
unsafe extern "system" {
    fn AttachConsole(dw_process_id: u32) -> i32;
    fn GenerateConsoleCtrlEvent(dw_ctrl_event: u32, dw_process_group_id: u32) -> i32;
    fn FreeConsole() -> i32;
}

#[test]
fn t4_e8_ffi_ctrl_c_to_ping() {
    let (mut pty, rx, handle) = spawn_collect("cmd.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"ping -t 127.0.0.1\r").expect("write ping");
        w.flush().expect("flush");
    }
    let pre = drain_quiet(&rx, Duration::from_millis(800), Duration::from_secs(6));
    assert!(pre.windows(4).any(|w| w == b"TTL="), "ping 应有 TTL 输出");
    let pid = pty.child_pid().expect("child pid");
    probe_note("E8-ffi", &format!("ping pid={}", pid));
    // AttachConsole 会失败（ConPTY 控制台拒绝外部附加），随后
    // GenerateConsoleCtrlEvent(0, 0) 就广播到**本测试进程的控制台组**，
    // 会把测试 harness 自己打死（0xC000013A）。先在本进程重新忽略 Ctrl-C。
    reignore_ctrl_c_in_test();
    let attach = unsafe { AttachConsole(pid) };
    let gen_ret = unsafe { GenerateConsoleCtrlEvent(0, 0) };
    let free = unsafe { FreeConsole() };
    probe_note("E8-ffi", &format!("AttachConsole={} Generate={} Free={}", attach, gen_ret, free));
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"echo Ffidone\r").expect("write echo");
        w.flush().expect("flush");
    }
    let post = drain_quiet(&rx, Duration::from_millis(1500), Duration::from_secs(8));
    let post_text = String::from_utf8_lossy(&post).to_string();
    probe_note("E8-ffi", &format!("post={} {:?}", post.len(), post_text));
    // 未解决（已验证事实）：AttachConsole=0（ConPTY 控制台拒绝外部附加），
    // GenerateConsoleCtrlEvent 虽返回 1 但 ping -t 未中断，Ffidone 不出现。
    // 按阶段规格，此项以记录代替断言，不推断机制。
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("join");
}

/// E9：cmd + timeout /T 99（非 stdin 型前台命令）+ \x03，判别是否 ping 特有。
#[test]
fn t4_e9_cmd_timeout_ctrl_c() {
    ctrl_c_probe(
        "cmd.exe",
        "timeout /T 99\r",
        b"\x03",
        "E9-cmd-timeout",
        Duration::from_secs(10),
    );
}

/// E11：powershell Test-Connection 连续 + \x03。
#[test]
fn t4_e11_powershell_testconnection_ctrl_c() {
    ctrl_c_probe(
        "powershell.exe",
        "Test-Connection -ComputerName 127.0.0.1 -Count 9999\r",
        b"\x03",
        "E11-ps-testconnection",
        Duration::from_secs(10),
    );
}

/// E12：sort 中断全过程（pre+post 全量落盘），复核 sort 分化结论。
#[test]
fn t4_e12_cmd_sort_full_data() {
    let (mut pty, rx, handle) = spawn_collect("cmd.exe");
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"sort\r").expect("write sort");
        w.flush().expect("flush");
    }
    let pre = drain_quiet(&rx, Duration::from_millis(800), Duration::from_secs(4));
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"\x03").expect("write ctrl-c");
        w.flush().expect("flush ctrl-c");
    }
    let post = drain_quiet(&rx, Duration::from_millis(1000), Duration::from_secs(4));
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"echo SORTDONE\r").expect("write echo");
        w.flush().expect("flush echo");
    }
    let tail = drain_quiet(&rx, Duration::from_millis(700), Duration::from_secs(4));
    probe_note(
        "E12-cmd-sort",
        &format!(
            "pre={} {:?} | post={} {:?} | tail={} {:?}",
            pre.len(),
            String::from_utf8_lossy(&pre),
            post.len(),
            String::from_utf8_lossy(&post),
            tail.len(),
            String::from_utf8_lossy(&tail)
        ),
    );
    let tail_text = String::from_utf8_lossy(&tail).to_string();
    assert!(tail_text.contains("SORTDONE"), "sort 应被中断且外壳可执行 echo");
    pty.close().expect("close");
    join_thread_timeout(handle, Duration::from_secs(5)).expect("join");
}

/// T6 探针：从 C:\dev\tty 为 cwd 起 spawn("cmd.exe")，复现 app 启动环境。
#[test]
fn t6_probe_spawn_from_devtty_cwd() {
    let saved = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(r"C:\dev\tty").expect("chdir to C:\\dev\\tty");
    let r = spawn_collect("cmd.exe");
    let (mut pty, rx, handle) = r;
    {
        let w = pty.writer().expect("writer");
        w.write_all(b"echo CWDPROBE\r").expect("write");
        w.flush().expect("flush");
    }
    let out = drain_quiet(&rx, Duration::from_millis(800), Duration::from_secs(8));
    probe_note(
        "T6-cwdprobe",
        &format!(
            "cwd={} out={} {:?}",
            std::env::current_dir().unwrap().display(),
            out.len(),
            String::from_utf8_lossy(&out)
        ),
    );
    let ok = out.windows(8).any(|w| w == b"CWDPROBE");
    let _ = pty.close();
    let _ = join_thread_timeout(handle, Duration::from_secs(5));
    std::env::set_current_dir(saved).expect("restore cwd");
    assert!(ok, "从 C:\\dev\\tty cwd spawn cmd.exe 应成功并回显");
}

/// T6 单元：resolve_program 对相对名与 POSIX 形态 PATH 均解析到绝对路径。
#[test]
fn t6_resolve_program_windows() {
    let r = pty::windows::WindowsPty::resolve_program("cmd.exe");
    assert!(
        r.len() > 7 && r.to_ascii_lowercase().ends_with("cmd.exe") && Path::new(&r).is_absolute(),
        "cmd.exe 应解析为绝对路径，实际 {}",
        r
    );
    let old = std::env::var("PATH").unwrap_or_default();
    // 模拟 app 由 bash 直启时的 POSIX 形态 PATH（已验证事实的触发环境）。
    unsafe { std::env::set_var("PATH", "/c/Windows/System32") };
    let r2 = pty::windows::WindowsPty::resolve_program("cmd.exe");
    unsafe { std::env::set_var("PATH", old) };
    assert!(
        Path::new(&r2).is_absolute()
            && r2.to_ascii_lowercase().ends_with("cmd.exe")
            && Path::new(&r2).is_file(),
        "POSIX 形态 PATH 下 cmd.exe 应解析为存在的绝对路径，实际 {}",
        r2
    );
}

use std::path::Path;
