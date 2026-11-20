//! app crate：winit 事件循环接线。
//!
//! 接线顺序：
//! 1. 起 PTY（pty::Pty::spawn），读线程经 channel 把字节交回主线程；
//! 2. 主线程 channel 收字节 → vt::VtTerminal::feed；读线程每来一次数据经 Wake 唤醒事件循环；
//! 3. 键盘事件 → route_key 路由：event.text 优先直写 PTY；Ctrl/Alt 修饰组合与无 text 的
//!    命名键走 vt::KeyEncoder → PTY 写；
//! 4. 窗口 resize → vt::VtTerminal::resize 与 pty::Pty::resize；
//! 5. vt::VtTerminal::on_pty_write 回调 → channel → 主线程写 PTY
//!    （回调在 feed 调用栈内同步触发，闭包内不能直接借用 PTY writer，故经 channel 中转）；
//! 6. 子进程退出（pty::Pty::try_wait）→ 关闭窗口。
//!
//! 事件循环用 winit 0.30 的 ApplicationHandler 模式（run_app），窗口经
//! `Rc<Window>` 与 softbuffer Surface 共享句柄，避免闭包自引用借用冲突。
//!
//! 渲染尺寸取缓存（resumed/Resized 更新），窗口销毁后不再查询 X11 geometry。

/// 启动期/运行期性能与诊断日志的统一开关（阶段 4O t3）。
///
/// 默认**关闭**：release 正常运行不再向 stderr 打印 `[perf]` 行（此前每次启动 30+ 行）。
/// 设置环境变量 `LEANTTY_PERF_FRAMES`（已有变量，e2e 与帧日志共用）即打开。
/// 面向用户的警告/错误（配置告警、hinting 不支持、backend 回落等）不受此开关影响。
fn perf_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LEANTTY_PERF_FRAMES").is_some_and(|v| !v.is_empty()))
}

/// 诊断开关 `LEANTTY_FORCE_LIVENESS_POLL`（阶段 4V t1）：非 Windows 平台也走"存活检查
/// 唤醒"分支。仅用于在 Linux 上复现 Windows 的"子进程退出后窗口不关闭"问题，
/// 不改变任何生产行为（默认关闭）。
fn force_liveness_poll() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("LEANTTY_FORCE_LIVENESS_POLL")
            .map(|v| v.trim() == "1")
            .unwrap_or(false)
    })
}

/// 是否启用"存活检查唤醒"：Windows 恒开启（ConPTY 不 EOF，只能轮询）；
/// 其他平台仅在诊断开关打开时启用。
fn liveness_poll_wanted() -> bool {
    cfg!(target_os = "windows") || force_liveness_poll()
}

/// 受 `perf_on()` 门控的 eprintln（仅用于 `[perf]` 诊断行）。
macro_rules! eperf {
    ($($arg:tt)*) => {
        if perf_on() {
            eprintln!($($arg)*);
        }
    };
}

use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use config::{Action as ConfigAction, Mods as ConfigMods, Mouse as ConfigMouse};
use pty::{NativePty, Pty};
use render::{CpuRenderer, Renderer};
use std::collections::VecDeque;
use vt::ScrollViewport;
use vt::{Action, Cell, Mods, VtTerminal};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event::{MouseButton, MouseScrollDelta};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

/// 字体渲染配置与网格尺寸由字体度量算出（见 render::cell_metrics），无固定常量。
/// SHELL 环境变量未设置（或非 UTF-8）时的平台默认 shell。
///
/// Windows 回落 cmd.exe，其他平台回落 /bin/sh。
/// 内嵌的窗口图标（阶段 5A t3）。
///
/// 字节由 `scripts/gen-icons.sh` 从 `assets/icon/leantty.svg` 生成；解析失败只打印
/// 用户可见警告并返回 `None`（不退出、不影响启动），符合"图标不是关键路径"的定位。
fn window_icon() -> Option<winit::window::Icon> {
    const RGBA: &[u8] = include_bytes!("../assets/window-icon-128.rgba");
    match winit::window::Icon::from_rgba(RGBA.to_vec(), 128, 128) {
        Ok(icon) => Some(icon),
        Err(e) => {
            eprintln!("window icon: 图标数据无效（{e}），本次不设置窗口图标");
            None
        }
    }
}

// ===== 阶段 5A t4：Windows 控制台释放 =====

/// 是否应该调用 `FreeConsole`：仅当**当前进程是控制台的唯一使用者**时才释放
/// （阶段 5A t4，纯函数，便于单测）。
///
/// 由 `GetConsoleProcessList` 返回的进程数决定：1 表示只有自己（典型情形是从资源管理器
/// 或 .cmd 启动、且父进程已退出），此时释放控制台不会影响别人；大于 1 说明还有其它进程
/// 共享该控制台（例如从已打开的 cmd 窗口里启动），释放会让对方的窗口消失，故不动。
pub fn should_free_console(process_count: u32) -> bool {
    process_count == 1
}

#[cfg(windows)]
mod console {
    /// 释放本进程的控制台（失败一律忽略——这不是关键路径）。
    ///
    /// # Safety
    /// 两个调用都是 kernel32 的无参数/单指针 Win32 API：
    /// - `GetConsoleProcessList(&mut buf, len)` 的第一个参数是长 1 的 `u32` 数组（可空），
    ///   第二个是数组长度；传 `null_mut(), 0` 时返回控制台上的进程总数，不写内存。
    /// - `FreeConsole()` 无参数、无返回值语义（BOOL），只会失败于“本进程没有控制台”。
    ///
    /// 两者都不会保存我们传入的指针，也不会跨调用持有引用。
    pub fn free_if_sole_user() {
        unsafe extern "system" {
            fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
            fn FreeConsole() -> i32;
        }
        // 阶段 5B t4b（已验证事实）：本机 Win10 LTSC 上 `GetConsoleProcessList(null, 0)`
        // 返回 0 且 GetLastError=87（ERROR_INVALID_PARAMETER），因此旧实现永远不释放；
        // 传非空缓冲（容量 2）才能得到真实进程数（探针：独占=1、共享=2）。
        let mut buf = [0u32; 2];
        let count = unsafe { GetConsoleProcessList(buf.as_mut_ptr(), buf.len() as u32) };
        if !crate::should_free_console(count) {
            return;
        }
        // SAFETY: 无参数调用；返回值（BOOL）失败时唯一原因是本进程没有控制台，
        // 按"失败一律忽略"处理，不做重试也不报错。
        unsafe {
            FreeConsole();
        }
    }
}

#[cfg(windows)]
pub use console::free_if_sole_user;

pub fn default_shell() -> &'static str {
    if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }
}

/// 按键输入路由（纯函数输出）。
pub enum KeyRoute {
    /// event.text 优先：直接写 PTY 的文本字节。
    Text(String),
    /// Ctrl/Alt 修饰组合或无 text 的命名键：走 KeyEncoder 编码。
    Encoded { key: Option<vt::Key>, mods: Mods },
}

/// 按键输入路由（纯函数）：event.text 优先直写 PTY；Ctrl/Alt 修饰组合与无 text 的
/// 命名键走 KeyEncoder。
///
/// 根因修复（已验证事实）：字母键经 KeyEncoder 编码需要 event 的 unshifted codepoint，
/// app 侧未设置时输出为空字节（e2e 插桩实测 encoded 0B）；event.text 路径不需要 codepoint。
pub fn route_key(text: Option<&str>, key: Option<vt::Key>, mods: Mods) -> KeyRoute {
    if mods.intersects(Mods::CTRL | Mods::ALT) {
        return KeyRoute::Encoded { key, mods };
    }
    // 阶段 6B t2c：winit 给 Delete/Backspace 等命名键的 text 是控制字符本身
    //（例如 Delete = "\u{7f}"）。若走文本直写就会变成 0x7f，而规范要求 ESC[3~；
    // 因此"整串都是控制字符"的文本一律交给编码器，由它按当前模式决定字节。
    if let Some(t) = text
        && !t.is_empty()
        && !t.chars().all(|c| (c as u32) < 0x20 || c == '\u{7f}')
    {
        return KeyRoute::Text(t.to_string());
    }
    KeyRoute::Encoded { key, mods }
}

/// 进程启动时刻（perf 钩子：首帧耗时实测，任务 2B.1；release 常驻，仅打印一次）。
static START: OnceLock<Instant> = OnceLock::new();

/// 记录进程启动时刻（main 首行调用；重复调用以首次为准）。
pub fn start_time_set() {
    let _ = START.set(Instant::now());
}

/// 进程启动时刻（用于计算"进程启动 → main 首行"的偏移）。
///
/// Linux 下取 `/proc/self/stat` 的 starttime（系统启动后的时钟滴答数，按
/// sysconf(_SC_CLK_TCK) 换算）与系统 uptime 推算；取不到时返回 None，
/// 此时 [perf] main_ms 会打印为 0 并标注 unknown。
pub fn process_start_time() -> Option<Instant> {
    // 读取 /proc/uptime（秒，浮点）与 /proc/self/stat 第 22 字段（starttime, ticks）
    let uptime: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // comm 可能含空格/括号：从最后一个 ')' 之后开始数
    let tail = stat.rsplit_once(')')?.1;
    let ticks: f64 = tail.split_whitespace().nth(19)?.parse().ok()?;
    // CLK_TCK 在 Linux 上固定为 100（user_hz），与 sysconf(_SC_CLK_TCK) 一致。
    let hz = 100.0f64;
    let age_s = uptime - ticks / hz;
    if !age_s.is_finite() || age_s < 0.0 {
        return None;
    }
    Some(Instant::now() - std::time::Duration::from_secs_f64(age_s))
}

/// 启动时刻（供首帧耗时日志）。
pub fn start_time() -> Option<Instant> {
    START.get().copied()
}

/// 唤醒抽象：读线程每来一次数据须调用一次 wake（channel + proxy 隔离，可测）。
pub trait Wake: Send {
    fn wake(&self) -> Result<(), Error>;
}

/// winit EventLoopProxy 实现（Send + 'static，可移入读线程）。
pub struct ProxyWaker {
    proxy: winit::event_loop::EventLoopProxy<()>,
}

impl ProxyWaker {
    pub fn new(proxy: winit::event_loop::EventLoopProxy<()>) -> Self {
        Self { proxy }
    }
}

impl Wake for ProxyWaker {
    fn wake(&self) -> Result<(), Error> {
        // send_event 失败仅一种形态：事件循环已销毁（EventLoopClosed）。
        // 归入 Error::EventLoopClosed，由调用方按正常关闭处理。
        self.proxy
            .send_event(())
            .map_err(|_| Error::EventLoopClosed)
    }
}

/// 读线程主体（抽出为可测函数）：读 PTY → channel → wake。
/// wake 返回 EventLoopClosed（事件循环已销毁，app 正常关闭路径）时静默退出；
/// 其他错误经 report 上报后退出。
pub fn reader_body<R: std::io::Read>(
    reader: &mut R,
    tx: &mpsc::SyncSender<Vec<u8>>,
    waker: &dyn Wake,
    report: &mut dyn FnMut(&str),
) {
    reader_body_with_depth(reader, tx, waker, report, None)
}

/// 同 `reader_body`，但可选地把"已入队未取走"的条数记到共享计数器
/// （阶段 4C t3 的 `chan_depth_max` 观测；`mpsc::Receiver` 不暴露 len）。
pub fn reader_body_with_depth<R: std::io::Read>(
    reader: &mut R,
    tx: &mpsc::SyncSender<Vec<u8>>,
    waker: &dyn Wake,
    report: &mut dyn FnMut(&str),
    depth: Option<&std::sync::atomic::AtomicUsize>,
) {
    let mut buf = [0u8; 4096];
    // 诊断（`LEANTTY_PTY_TRACE=<path>`）：把 PTY 读到的前 4 KiB 以十六进制落盘，
    // 用于判断"首行多一个空行"是子进程写出来的还是我们渲染出来的。
    let mut trace: Option<(std::fs::File, usize)> = std::env::var_os("LEANTTY_PTY_TRACE")
        .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
        .map(|f| (f, 0usize));
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => {
                // reader 退出前唤醒主线程：否则事件循环无新事件可等，
                // about_to_wait 不触发→try_wait 不轮询→子进程退出检测丢失
                // （case11 偶发 5s 未退出根因；DECISIONS 登记）。
                let _ = waker.wake();
                break;
            }
            Ok(n) => {
                if let Some((f, written)) = trace.as_mut() {
                    use std::io::Write as _;
                    if *written < 4096 {
                        let mut hex = String::with_capacity(n * 3);
                        for b in &buf[..n] {
                            hex.push_str(&format!("{b:02x}"));
                        }
                        let _ = writeln!(f, "{hex}");
                        let _ = f.flush();
                        *written += n;
                    }
                }
                if let Some(d) = depth {
                    d.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                // 入队（阶段 3B 任务 2：有界通道 + 背压，无忙等、无死锁）。
                //
                // 死锁场景：通道满时读线程若先阻塞 send 再 wake，则主线程排空后回到
                // Wait 而无人唤醒（200k 行实测死锁）。因此满时先 wake 让主线程开始
                // 消费，再阻塞 send，成功后再 wake 一次确保最后一条被看到。
                let wake_or_exit = |w: &dyn Wake, r: &mut dyn FnMut(&str)| -> bool {
                    match w.wake() {
                        Ok(()) => true,
                        Err(e) => {
                            if !matches!(e, Error::EventLoopClosed) {
                                r(&format!("wake: {}", e));
                            }
                            false
                        }
                    }
                };
                match tx.try_send(buf[..n].to_vec()) {
                    Ok(()) => {
                        if !wake_or_exit(waker, report) {
                            break;
                        }
                    }
                    Err(mpsc::TrySendError::Full(d)) => {
                        if !wake_or_exit(waker, report) {
                            break;
                        }
                        // 阻塞等待空位（主线程已在消费）；不忙等、不睡眠。
                        if tx.send(d).is_err() {
                            break;
                        }
                        if !wake_or_exit(waker, report) {
                            break;
                        }
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
        }
    }
}

pub mod cli;
mod clipboard;
mod follow;
mod framepace;
mod ime;
mod paste;
mod scrollbar;
mod selection;
mod writer;

pub use clipboard::{ArboardClipboard, Clipboard, ClipboardKind, InjectClipboard};
pub use framepace::{PACE_EMA_ALPHA, PACE_K, PACE_MAX_MS, PACE_MIN_MS, next_ema, target_interval};
pub use ime::{normalize_ime_commit, preedit_display};
pub use paste::{
    DrainBudget, DrainReport, PASTE_CHUNK, drain_pending, drain_to_writer, normalize_paste,
    paste_chunks,
};
pub use scrollbar::{ScrollbarGeom, scrollbar_geometry, track_y_to_offset};
pub use selection::{
    CellPos, Selection, abs_to_viewport, auto_scroll_step, extract_rows, extract_text,
    extract_text_stream, selection_viewport_region, stream_row_range, viewport_to_abs, word_region,
};

/// 帧耗时诊断统计（LEANTTY_PERF_FRAMES=1；性能诊断日志，release 保留，默认关闭）。
#[derive(Default)]
struct PerfFrames {
    render_us: Vec<u128>,
    present_us: Vec<u128>,
    /// 因帧间隔未到期而被合并（丢弃）的唤醒次数。
    coalesced: u64,
    /// 最近一次采用的帧间隔（ms）。
    interval_ms: u64,
    /// 本统计窗口内喂入 vt 的字节数（阶段 4C t3）。
    feed_bytes: u128,
    /// 本窗口起点（用于算窗口时长 → feed 速率）。
    window_start: Option<std::time::Instant>,
    /// 本窗口内有界 channel 的积压峰值（阶段 4C t3）。
    chan_depth_max: usize,
    /// 本窗口内"两次事件循环迭代"的最大间隔（阶段 4G t2a，用于量化主线程停顿）。
    loop_gap_max_us: u128,
    /// 单次迭代内 feed 的总耗时峰值与对应字节数（阶段 4I t3 归因）。
    feed_ms_max: f64,
    feed_bytes_max: u64,
    /// 写线程队列深度峰值（阶段 4I t3 归因）。
    writer_queued_max: usize,
}

impl PerfFrames {
    const N: usize = 50;

    fn report(&mut self, mem: &str) {
        if self.render_us.len() < Self::N {
            return;
        }
        let stats = |v: &mut Vec<u128>| {
            v.sort_unstable();
            let med = v[v.len() / 2] / 1000;
            let p95 = v[v.len() * 95 / 100] / 1000;
            v.clear();
            (med, p95)
        };
        let (r_med, r_p95) = stats(&mut self.render_us);
        let (p_med, p_p95) = stats(&mut self.present_us);
        let fps = 1000u64.checked_div(self.interval_ms).unwrap_or(0);
        // feed 速率 = 本窗口喂入字节 / 窗口时长（阶段 4C t3；用于核对
        // "总字节数 ≈ 速率 × 时长" 的自洽性）。
        let elapsed_s = self
            .window_start
            .map(|t| t.elapsed().as_secs_f64())
            .filter(|s| *s > 0.0);
        let feed_rate = elapsed_s.map(|s| self.feed_bytes as f64 / s).unwrap_or(0.0);
        eperf!(
            "[perf] frames n={} render_ms(med={} p95={}) present_ms(med={} p95={}) coalesced={} interval_ms={} ~{}fps feed_bytes_per_s={:.0} chan_depth_max={} loop_gap_max_ms={} feed_ms_max={:.1} feed_bytes_max={} writer_queued_max={} {}",
            Self::N,
            r_med,
            r_p95,
            p_med,
            p_p95,
            self.coalesced,
            self.interval_ms,
            fps,
            feed_rate,
            self.chan_depth_max,
            self.loop_gap_max_us / 1000,
            self.feed_ms_max,
            self.feed_bytes_max,
            self.writer_queued_max,
            mem
        );
        self.coalesced = 0;
        self.loop_gap_max_us = 0;
        self.feed_ms_max = 0.0;
        self.feed_bytes_max = 0;
        self.writer_queued_max = 0;
        self.feed_bytes = 0;
        self.chan_depth_max = 0;
        self.window_start = Some(std::time::Instant::now());
    }
}

/// 渲染最小间隔（阶段 3B 任务 2：帧合并）。高频 PTY 输出时最多约 125 帧/秒，
/// 即约 8ms 一帧；交互事件（键盘/鼠标/IME/resize）走 urgent 低延迟路径不受此限。
const RENDER_MIN_INTERVAL: Duration = Duration::from_millis(8);

type SoftSurface = softbuffer::Surface<winit::event_loop::OwnedDisplayHandle, Rc<Window>>;

/// 应用程序配置。
pub struct App {
    /// 初始列数。
    pub cols: u16,
    /// 初始行数。
    pub rows: u16,
    /// `-e/--command` 覆盖（程序 + 参数）；`None` 表示走配置的 shell。
    exec_command: Option<Vec<String>>,
    /// shell 路径。
    pub shell: String,
    /// 字体设置（config 转换，含四组 style 与 fallback）。
    pub font_settings: render::FontSettings,
    /// 渲染选项（font 渲染选项）与颜色表（colors 段）。
    pub options: render::RenderOptions,
    pub palette: render::Palette,
    /// 内边距（逻辑像素，窗口每侧）。
    pub pad_x: u16,
    /// 内边距（逻辑像素，窗口每侧）。
    pub pad_y: u16,
    /// 原始配置（绑定执行/选区参数/滚动/IME 等后续任务按需读取）。
    pub config: config::Config,
}

impl App {
    /// 新建应用配置（由强类型 Config 驱动；shell 优先级 terminal.shell.program > SHELL > 平台默认）。
    pub fn new(config: config::Config) -> Self {
        Self::new_with_command(config, None)
    }

    /// 带 `-e/--command` 覆盖的构造（阶段 7A）：argv 首项为程序，其余为参数。
    pub fn new_with_command(config: config::Config, command: Option<Vec<String>>) -> Self {
        let shell = config
            .terminal
            .shell
            .as_ref()
            .map(|s| s.program.clone())
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| default_shell().to_string());
        Self {
            cols: config.window.dimensions.columns,
            rows: config.window.dimensions.lines,
            shell,
            exec_command: command,
            font_settings: render_font_settings(&config),
            options: render_options(&config),
            palette: render_palette(&config.colors),
            pad_x: config.window.padding.x,
            pad_y: config.window.padding.y,
            config,
        }
    }

    /// 运行事件循环（含上述六条接线）。
    pub fn run(self) -> Result<(), Error> {
        let t_loop = Instant::now();
        let event_loop =
            EventLoop::new().map_err(|e| Error::App(format!("EventLoop::new: {}", e)))?;
        // 阶段 4H t2：EventLoop::new（X11/socket 连接、平台后端初始化）此前无打点。
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] event_loop_ms={:.1} since_start={:.1}",
                t_loop.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let exec_command = self.exec_command.clone();
        let mut handler = AppHandler {
            perf_reset_at: std::env::var("LEANTTY_PERF_RESET_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|ms| Instant::now() + std::time::Duration::from_millis(ms)),
            config: Some(self),
            exec_command,
            selection: None,
            mouse_down: false,
            last_click: None,
            click_count: 0,
            clipboard: None,
            pending_paste: VecDeque::new(),
            last_mouse_pos: None,
            cols: 0,
            rows: 0,
            offset_from_top: 0,
            follow_bottom: true,
            scrollbar_drag: false,
            ime_preedit: String::new(),
            ime_preedit_cursor: None,
            auto_scroll: 0,
            #[cfg(debug_assertions)]
            test_preedit: None,
            #[cfg(debug_assertions)]
            test_preedit_clear_at: None,
            last_title: None,
            last_sel_region: None,
            last_render_offset: 0,
            #[cfg(debug_assertions)]
            force_full_redraw: std::env::var("LEANTTY_FORCE_FULL_REDRAW").is_ok(),
            drained_any: false,
            last_render: None,
            urgent_render: false,
            #[cfg(debug_assertions)]
            no_render: std::env::var("LEANTTY_NO_RENDER").is_ok(),
            #[cfg(debug_assertions)]
            no_throttle: std::env::var("LEANTTY_NO_THROTTLE").is_ok(),
            present_full: std::env::var("LEANTTY_PRESENT_FULL").is_ok(),
            render_ema_ms: None,
            frame_interval: Duration::from_millis(PACE_MIN_MS as u64),
            perf_frames: std::env::var("LEANTTY_PERF_FRAMES")
                .is_ok()
                .then(PerfFrames::default),
            #[cfg(debug_assertions)]
            present_stats: std::env::var("LEANTTY_PRESENT_STATS")
                .is_ok()
                .then(|| (std::env::var("LEANTTY_PRESENT_FULL").is_ok(), Vec::new())),
            vt: None,
            pty: None,
            renderer: None,
            window: None,
            surface: None,
            rx: None,
            pty_write_rx: None,
            reader_thread: None,
            last_size: None,
            cell_w: 8,
            cell_h: 16,
            pad_x: 0,
            pad_y: 0,
            scale_factor: 1.0,
            key_trace: std::env::var_os("LEANTTY_KEY_TRACE").and_then(|p| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .ok()
            }),
            focused_at: None,
            alt_released_at: None,
            focused: true,
            is_x11: false,
            event_proxy: Some(event_loop.create_proxy()),
            mods_state: ModifiersState::default(),
            first_frame_logged: false,
            first_pty_byte_logged: false,
            pre_first_submit_logged: false,
            writer: None,
            wake_counts: (0, 0, 0, 0),
            got_event: false,
            got_pty: false,
            last_liveness_check: None,
            last_sel_clamp_log: None,
            last_geom_drawn: None,
            frames_window: (0, 0, 0, 0),
            next_frame_origin: 0,
            last_loop_at: None,
            loop_report_at: None,
            chan_depth: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            os_redraw_pending: false,
            #[cfg(debug_assertions)]
            os_redraw_count: 0,
            #[cfg(debug_assertions)]
            test_paste_at: None,
            #[cfg(debug_assertions)]
            test_paste_text: None,
            #[cfg(debug_assertions)]
            test_paste_fired: None,
            fatal: None,
        };
        event_loop
            .run_app(&mut handler)
            .map_err(|e| Error::App(format!("event loop: {}", e)))?;
        // 退出前先丢弃接收端：大量输出（如 20 万行）时 channel 可能已积压，
        // 读线程会阻塞在 send；drop Receiver 使其 send 立即返回 Disconnected 并退出，
        // 否则 reader join 会等满 5s 超时（阶段 3C 任务 4 case28 实测）。
        let _ = handler.rx.take();
        // 致命错误优先返回：强制 GPU 后端不可用等情形必须以非零码退出。
        if let Some(e) = handler.fatal.take() {
            return Err(e);
        }
        // run_app 返回后回收：先 close（先释放主端再回收子进程，kill 子进程使 reader
        // 读端得 EOF 而退出），再带超时 join reader 线程（顺序倒置将空等 5s 超时）。
        // 顺序（阶段 4H t3 契约）：写线程先写空队列并释放写端（PTY 主端一侧），
        // 再 close（释放 master 并回收子进程），最后带超时 join 读线程。
        if let Some(mut w) = handler.writer.take() {
            w.close();
        }
        if let Some(mut pty) = handler.pty.take() {
            pty.close()?;
        }
        if let Some(thread) = handler.reader_thread.take() {
            pty::unix::join_thread_timeout(thread, Duration::from_secs(5)).map_err(Error::from)?;
        }
        Ok(())
    }
}

/// app crate 错误枚举。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 应用层错误。
    #[error("app error: {0}")]
    App(String),
    /// PTY 错误。
    #[error("pty error: {0}")]
    Pty(#[from] pty::Error),
    /// 事件循环已关闭（app 正常关闭路径，读线程静默退出，不上报）。
    #[error("event loop closed")]
    EventLoopClosed,
    /// 剪贴板错误（arboard/X11 所有权语义等）。
    #[error("clipboard error: {0}")]
    Clipboard(String),
    /// 终端错误。
    #[error("vt error: {0}")]
    Vt(#[from] vt::Error),
    /// 渲染错误。
    #[error("render error: {0}")]
    Render(#[from] render::Error),
    /// 请求的渲染后端在当前平台/构建不可用。
    #[error("renderer backend unsupported: {0}")]
    BackendUnsupported(String),
}

/// 渲染后端（阶段 4B t2）：由配置 `renderer.backend` 决定，**必须在首个
/// present 之前**创建，运行期不切换（避免增量渲染状态与 surface 语义冲突）。
///
/// 之所以用枚举而不是给 `Renderer` trait 加方法：trait 只保留 `render` 与
/// 默认空的 `set_preedit`（既有契约），CPU 专有方法（选区/滚动条/damage 等）
/// 由 app 层按后端分发。
pub enum RendererBackend {
    /// CPU 光栅化（softbuffer 提交）。
    Cpu(Box<CpuRenderer>),
    /// GPU（wgpu/Vulkan，仅 Linux；渲染到 winit 窗口 surface，不回读）。
    /// 用 Box 装箱：GpuRenderer 远大于 CpuRenderer，避免枚举整体膨胀。
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    Gpu(Box<render::gpu::GpuRenderer>),
}

impl RendererBackend {
    /// 后端名字（日志/诊断用）。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cpu(_) => "cpu",
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(_) => "gpu",
        }
    }

    /// 是否 GPU 后端（app 层据此决定叠加层/脏区分发）。
    pub fn is_gpu(&self) -> bool {
        match self {
            Self::Cpu(_) => false,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(_) => true,
        }
    }

    /// 当前字体的格子尺寸（物理像素）。
    pub fn cell_metrics(&self, scale: f64) -> (u32, u32) {
        match self {
            Self::Cpu(r) => r.cell_metrics(scale),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.cell_metrics(scale),
        }
    }

    /// 更新缩放因子（字形缓存失效 + 整屏重绘）。
    pub fn set_scale_factor(&mut self, scale: f64) {
        match self {
            Self::Cpu(r) => r.set_scale_factor(scale),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.set_scale_factor(scale),
        }
    }

    /// 选区高亮（网格坐标，含端点）。
    pub fn set_selection(&mut self, region: Option<(u16, u16, u16, u16)>) {
        match self {
            Self::Cpu(r) => r.set_selection(region),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.set_selection(region),
        }
    }

    /// 滚动条拇指几何。
    pub fn set_scrollbar(&mut self, geom: Option<(u32, u32)>) {
        match self {
            Self::Cpu(r) => r.set_scrollbar(geom),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.set_scrollbar(geom),
        }
    }

    /// IME 预编辑叠加层。
    pub fn set_preedit(&mut self, overlay: Option<render::PreeditOverlay>) {
        match self {
            Self::Cpu(r) => r.set_preedit(overlay),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.set_preedit(overlay),
        }
    }

    pub fn render(
        &mut self,
        frame: &vt::FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<(), render::Error> {
        match self {
            Self::Cpu(r) => r.render(frame, pixel_width, pixel_height),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.render_surface(frame, pixel_width, pixel_height),
        }
    }

    /// 本帧脏区（CPU 专有；GPU 走后端自己的整帧提交）。
    pub fn damage(&self) -> &[(u32, u32, u32, u32)] {
        match self {
            Self::Cpu(r) => r.damage(),
            // GPU 后端每帧整帧提交，没有"脏区"概念（surface 纹理内容不保留）。
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(_) => &[],
        }
    }

    /// 帧缓冲（softbuffer 提交与诊断用）。
    pub fn framebuffer(&self) -> &[u32] {
        match self {
            Self::Cpu(r) => r.framebuffer(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(_) => &[],
        }
    }

    /// 帧缓冲尺寸。
    pub fn framebuffer_size(&self) -> (u32, u32) {
        match self {
            Self::Cpu(r) => r.framebuffer_size(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.pixel_size(),
        }
    }

    /// 取 CPU 渲染器引用（诊断钩子/帧哈希用；GPU 后端返回 None）。
    pub fn as_cpu(&self) -> Option<&CpuRenderer> {
        match self {
            Self::Cpu(r) => Some(r),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(_) => None,
        }
    }

    /// 窗口尺寸变化（GPU 后端需 reconfigure surface）。
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        // 非 Linux 目标没有 GPU 分支，这两个参数不会被消费（Windows 上
        // release 构建会报 unused variable）。仅在此目标上显式忽略。
        #[cfg(not(target_os = "linux"))]
        let _ = (width, height);
        match self {
            Self::Cpu(_) => {}
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Self::Gpu(r) => r.resize_surface(width, height),
        }
    }
}

/// 按配置创建渲染器（阶段 4B t2：仅接通 CPU；GPU 分支在 t3/t4 接入）。
///
/// 依次尝试只发生在 `auto`；`gpu` 失败会直接返回错误（调用方非零退出），
/// 不做静默降级。Windows 目标一律 CPU，`gpu` 给一行警告。
pub fn create_renderer(
    cfg: &config::Config,
    font_settings: &render::FontSettings,
    options: render::RenderOptions,
    palette: render::Palette,
    window: Option<&winit::window::Window>,
    existing: Option<RendererBackend>,
) -> Result<RendererBackend, Error> {
    let requested = cfg.renderer.backend;
    let build_cpu =
        |scale: f64, existing: Option<RendererBackend>| -> Result<RendererBackend, Error> {
            // 复用窗口创建前已建好的 CPU 渲染器：debug 构建下 fontdb 扫描 + 四组
            // 主字体解析约 400-600ms，重建一次会明显推迟首帧与后台字体到位时间。
            if let Some(mut r) = existing {
                r.set_scale_factor(scale);
                return Ok(r);
            }
            let mut r = CpuRenderer::new(font_settings, options, palette)?;
            r.set_scale_factor(scale);
            Ok(RendererBackend::Cpu(Box::new(r)))
        };
    // 没有窗口时（仅用于窗口创建前估算格子尺寸）只能先用 CPU。
    let Some(window) = window else {
        return build_cpu(1.0, existing);
    };
    let scale = window.scale_factor();
    match requested {
        config::Backend::Cpu => build_cpu(scale, existing),
        config::Backend::Auto => {
            // 阶段 4P t2c：Windows 的 auto **仍直接走 CPU**——GPU 路径（Vulkan/DX12）尚未在
            // 真实 Windows GPU 上验证 present 行为，默认保持不变；Linux 的 auto 语义不变
            //（先试 GPU，失败回落 CPU）。
            #[cfg(target_os = "windows")]
            {
                eperf!("[perf] backend=auto → cpu（Windows 上 auto 不尝试 GPU，见 DECISIONS）");
                build_cpu(scale, existing)
            }
            #[cfg(target_os = "linux")]
            {
                let t = std::time::Instant::now();
                let reuse = existing.as_ref().and_then(|r| match r {
                    RendererBackend::Cpu(c) => Some(c.font_set()),
                    _ => None,
                });
                ALLOW_SOFTWARE_REQUESTED.with(|c| c.set(false)); // auto 永远忽略软件开关
                match build_gpu(font_settings, options, palette, window, scale, reuse) {
                    Ok((gpu, adapter)) => {
                        eperf!(
                            "[perf] gpu_init_ms={:.1} backend=gpu adapter='{}'",
                            t.elapsed().as_secs_f64() * 1000.0,
                            adapter
                        );
                        Ok(gpu)
                    }
                    Err(e) => {
                        eprintln!("[renderer] gpu 不可用，回落 cpu：{e}");
                        build_cpu(scale, existing)
                    }
                }
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                build_cpu(scale, existing)
            }
        }
        config::Backend::Gpu => {
            // 阶段 4P：Windows 与 Linux 一致——强制 gpu 且失败时返回错误（调用方非零退出），
            // 不再"回退 CPU 并警告"。
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            {
                let t = std::time::Instant::now();
                let reuse = existing.as_ref().and_then(|r| match r {
                    RendererBackend::Cpu(c) => Some(c.font_set()),
                    _ => None,
                });
                ALLOW_SOFTWARE_REQUESTED.with(|c| c.set(true)); // 显式 gpu 才可能允许
                let (gpu, adapter) =
                    build_gpu(font_settings, options, palette, window, scale, reuse)?;
                eperf!(
                    "[perf] gpu_init_ms={:.1} backend=gpu adapter='{}'",
                    t.elapsed().as_secs_f64() * 1000.0,
                    adapter
                );
                Ok(gpu)
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                Err(Error::App(
                    "renderer.backend=gpu 在当前平台不可用（仅 Linux/Windows 提供）".to_string(),
                ))
            }
        }
    }
}

/// 用 winit 窗口的 raw handle 创建 GPU surface 渲染器（仅 Linux）。
#[cfg(any(target_os = "linux", target_os = "windows"))]
/// `LEANTTY_GPU_ALLOW_SOFTWARE=1` 且配置显式 `backend="gpu"` 时才允许软件适配器。
fn cfg_gpu_forces_software() -> bool {
    ALLOW_SOFTWARE_REQUESTED.with(|c| c.get()) && render::gpu::allow_software_from_env()
}

thread_local! {
    /// 由 create_renderer 在调用 build_gpu 前设置：本次是否允许软件适配器
    /// （`backend="gpu"` → true；`backend="auto"` → false，永远忽略该环境变量）。
    static ALLOW_SOFTWARE_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn build_gpu(
    font_settings: &render::FontSettings,
    options: render::RenderOptions,
    palette: render::Palette,
    window: &winit::window::Window,
    scale: f64,
    existing_fonts: Option<std::sync::Arc<std::sync::Mutex<render::FontSet>>>,
) -> Result<(RendererBackend, String), Error> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    let wh = window
        .window_handle()
        .map_err(|e| Error::App(format!("window_handle: {e}")))?
        .as_raw();
    let dh = window
        .display_handle()
        .map_err(|e| Error::App(format!("display_handle: {e}")))?
        .as_raw();
    let size = window.inner_size();
    // 复用窗口前 CPU 渲染器已构建的字体链（stage 4E t3：省掉一次 fontdb + 字体解析）
    let reuse = existing_fonts;
    let mut gpu = render::gpu::GpuRenderer::new_surface_with_fonts(
        font_settings,
        options,
        palette,
        reuse,
        render::gpu::SurfaceInit {
            raw_window: wh,
            raw_display: dh,
            width: size.width.max(1),
            height: size.height.max(1),
        },
        // 阶段 4Q t4：软件适配器仅当 backend="gpu" 且 LEANTTY_GPU_ALLOW_SOFTWARE=1 时允许；
        // build_gpu 只被 Gpu/（Linux 的）Auto 调用，故此处再确认一次"强制 gpu"。
        cfg_gpu_forces_software(),
    )?;
    gpu.set_scale_factor(scale);
    let name = gpu.adapter_name().to_string();
    Ok((RendererBackend::Gpu(Box::new(gpu)), name))
}

struct AppHandler {
    config: Option<App>,
    /// 阶段 7A：`-e/--command` 覆盖（程序 + 参数），仅在本次启动使用。
    exec_command: Option<Vec<String>>,
    vt: Option<VtTerminal>,
    pty: Option<NativePty>,
    renderer: Option<RendererBackend>,
    window: Option<Rc<Window>>,
    surface: Option<SoftSurface>,
    rx: Option<mpsc::Receiver<Vec<u8>>>,

    pty_write_rx: Option<mpsc::Receiver<Vec<u8>>>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    /// 渲染尺寸缓存（resumed/Resized 更新；窗口销毁后不再查询 X11 geometry）。
    last_size: Option<(u32, u32)>,
    /// 当前格子物理尺寸（render::cell_metrics 按 scale_factor 计算）。
    cell_w: u32,
    cell_h: u32,
    /// 内边距（逻辑像素，每侧；窗口像素 = 行列*格子 + 2*padding*scale）。
    pad_x: u16,
    pad_y: u16,
    /// 当前缩放因子（winit scale_factor）。
    scale_factor: f64,
    /// 键盘诊断输出（`LEANTTY_KEY_TRACE=<path>` 时写入；用于定位 WM/输入法投递序列）。
    key_trace: Option<std::fs::File>,
    /// 后台字体是否已记录过"首次并入"时刻（逐个并入后用）。
    /// 最近一次拿到焦点的时刻。你的机器上 `Alt+Tab` 的 Tab press 会被 WM 抓走，
    /// 随后在 `Focused(true)` 之后 0–13 ms 内补投一次**裸 Tab press**（trace 实测），
    /// 那一击才是残留 Tab 的真正来源。
    focused_at: Option<Instant>,
    /// 最近一次"Alt 被释放"的时刻。用于识别 WM 组合键尾巴：`Alt+Tab` 在某些 WM/时序下
    /// Tab 的 press 会晚于 Alt 的 release 到达，此时 mods 里已没有 ALT。
    alt_released_at: Option<Instant>,
    /// 窗口当前是否持有焦点（阶段 9E）。`Alt+Tab` 切走的过程中会收到本窗口的
    /// `Focused(false)`，若随后仍把按键写进 PTY（切窗口时 Tab 的 release/press
    /// 次序在不同 WM 下不一致），终端里就会残留一个 Tab。
    focused: bool,
    /// 窗口是否为 X11：XIM 只接受 spot 点而非矩形，需把 IME 锚点下移一行
    /// （与 alacritty 的 `is_x11` 处理一致）。窗口创建后按 raw handle 判定。
    is_x11: bool,
    /// 首帧 present 耗时日志标志（perf 钩子，仅打印一次）。
    first_frame_logged: bool,
    /// 首个 PTY 字节到达日志标志（perf 钩子，仅打印一次）。
    first_pty_byte_logged: bool,
    /// PTY 写线程（阶段 4H t3）：拿到可独立持有的写端时启用；None = 沿用同步写
    /// （Windows ConPTY 的 `take_writer` 走 trait 默认实现返回 None）。
    writer: Option<writer::PtyWriter>,
    /// 本窗口（5 秒）内渲染的帧数与按来源分类（阶段 4Q t3 观测）：
    /// (总数, OS 重绘触发, 缩放/尺寸事件触发, 其它/自驱动)。
    frames_window: (u32, u32, u32, u32),
    /// 空闲唤醒来源计数（阶段 4S t2）：(loops, wake_timer, wake_event, wake_pty)
    wake_counts: (u32, u32, u32, u32),
    /// 本轮迭代是否由窗口事件/PTY 唤醒（用于区分"定时唤醒"）
    got_event: bool,
    got_pty: bool,
    /// 上次调用 try_wait 做存活检查的时刻（阶段 4T t3；仅 Windows 用于安排低频唤醒）。
    last_liveness_check: Option<Instant>,
    /// 上次打印"选区被 scrollback 截断"警告的时刻（阶段 4S t2 限速）。
    last_sel_clamp_log: Option<Instant>,
    /// 上次真正画到屏幕上的滚动条几何（阶段 4Q t3 空闲判定用）。
    last_geom_drawn: Option<(u32, u32)>,
    /// 待归属的"下一个渲染帧来源"：1=OS 重绘, 2=缩放/尺寸, 0=其它。
    next_frame_origin: u8,
    /// 上次事件循环迭代时刻（阶段 4G t2a 的 loop gap 观测）。
    last_loop_at: Option<std::time::Instant>,
    /// 下次独立打印 loop gap 的时刻（每 5 秒）。
    loop_report_at: Option<std::time::Instant>,
    /// 观测窗口重置时刻（阶段 4I t3，`LEANTTY_PERF_RESET_MS`，release 可用）。
    perf_reset_at: Option<std::time::Instant>,
    /// 首帧提交前一刻是否已打印（阶段 4C t3 启动打点）。
    pre_first_submit_logged: bool,
    /// 有界 channel 的"已入队未取走"计数（reader 加、主线程减），用于
    /// `chan_depth_max` 观测（阶段 4C t3）。
    chan_depth: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// OS 发起的重绘（X11 Expose / 遮挡后恢复等）待处理：必须整帧重绘 + 整窗提交，
    /// 不能按脏行增量（窗口内容可能已被系统丢弃）。应用自驱动的重绘不受影响。
    os_redraw_pending: bool,
    /// debug 专有：OS 触发重绘次数（e2e 行为级断言）。
    #[cfg(debug_assertions)]
    os_redraw_count: u32,
    /// debug 测试钩子：延迟触发一次同步粘贴（LEANTTY_TEST_PASTE / LEANTTY_TEST_PASTE_AT_MS）。
    /// 由事件循环内部调用 `paste_from_text`，不产生鼠标/键盘事件，因此不会顺带触发
    /// `RedrawRequested`，可以把"粘贴主动排帧"与其他重绘触发分离开来。
    #[cfg(debug_assertions)]
    test_paste_at: Option<std::time::Instant>,
    #[cfg(debug_assertions)]
    test_paste_text: Option<String>,
    /// 已触发、等待第一帧渲染以打印延迟（debug 钩子）。
    #[cfg(debug_assertions)]
    test_paste_fired: Option<std::time::Instant>,
    /// 致命错误（如强制 GPU 后端不可用）：置位后退出事件循环，run() 据此返回 Err
    /// 使进程以非零码退出（不得 panic、不得中途回退 CPU）。
    fatal: Option<Error>,
    /// 事件循环代理（run_app 前创建，读线程经其唤醒事件循环）。
    event_proxy: Option<winit::event_loop::EventLoopProxy<()>>,
    /// 后台字体加载完成通知（render 侧 channel；about_to_wait 轮询并入）。
    mods_state: ModifiersState,
    /// 当前选区（网格坐标；滚动偏移联动在任务 4 完成）。
    selection: Option<Selection>,
    /// 左键按下（拖选中）。
    mouse_down: bool,
    /// 上次点击（时刻+位置），双击/三击检测（<300ms 同格递增）。
    last_click: Option<(Instant, CellPos)>,
    click_count: u8,
    /// 剪贴板（主线程持有；arboard X11 所有权语义：set 后持有者存活至进程退出）。
    clipboard: Option<ArboardClipboard>,
    /// 大段粘贴分块队列（字节块；每轮按预算排空，剩余靠 request_redraw 继续）。
    pending_paste: VecDeque<Vec<u8>>,
    /// 最后鼠标位置（物理像素；MouseInput 事件无坐标，用 CursorMoved 缓存）。
    last_mouse_pos: Option<winit::dpi::PhysicalPosition<f64>>,
    /// 网格行列数（pixel_to_cell 上界校验；Resized 同步）。
    cols: u16,
    rows: u16,
    /// 滚动视口偏移（scrollback 顶=0，底=scrollback_rows；0=活动区/底部）。
    /// 滚轮/PageUp/拖拽统一维护，scroll_viewport(Row) 绝对定位。
    offset_from_top: usize,
    /// auto-follow：新输出时视口保持底部（用户上滚后暂停，回底恢复）。
    follow_bottom: bool,
    /// 滚动条拖拽中（左键命中右缘条带后进入，优先于选区拖拽）。
    scrollbar_drag: bool,
    /// IME 组合中间态（Preedit 串；Commit/Disabled 时清空）。
    ime_preedit: String,
    /// IME 组合内光标字节范围（Preedit 时随事件更新；清空时置 None）。
    ime_preedit_cursor: Option<(usize, usize)>,
    /// 拖选自动滚动方向（-1 上沿/0 停/+1 下沿；任务 2C.3 每帧推进）。
    auto_scroll: i8,
    /// 测试注入的预编辑串（仅 debug 构建；粘性，不被 winit Ime 事件清除）。
    #[cfg(debug_assertions)]
    test_preedit: Option<String>,
    /// 测试钩子：预编辑自动清除时刻（debug 专有，配合 LEANTTY_TEST_PREEDIT_CLEAR_MS）。
    #[cfg(debug_assertions)]
    test_preedit_clear_at: Option<std::time::Instant>,
    /// 上次应用的窗口标题（避免每帧重复 set_title）。
    last_title: Option<String>,
    /// 上次渲染时的选区区域（绝对坐标），用于选区叠加层脏判定。
    last_sel_region: Option<(u16, u16, u16, u16)>,
    /// 上次渲染时的视口偏移（变化即整屏重绘：视口滚动不做位移复用）。
    last_render_offset: usize,
    /// 强制整屏重绘（仅 debug：LEANTTY_FORCE_FULL_REDRAW=1，用于 e2e 增量/全量对拍）。
    #[cfg(debug_assertions)]
    force_full_redraw: bool,
    /// 本轮 about_to_wait 是否收到新 PTY 数据（供 follow/截断检查复用，减少快照）。
    drained_any: bool,
    /// 上次渲染时刻（帧合并节流用）。
    last_render: Option<Instant>,
    /// 交互事件触发的紧急渲染（键盘/鼠标/IME/resize）：跳过节流立即渲染。
    urgent_render: bool,
    /// 关闭渲染（仅 debug：LEANTTY_NO_RENDER=1，用于吞吐基线"只 feed 不渲染"）。
    #[cfg(debug_assertions)]
    no_render: bool,
    /// 关闭帧合并节流（仅 debug：LEANTTY_NO_THROTTLE=1），用于对照"节流前"的输入回显延迟。
    #[cfg(debug_assertions)]
    no_throttle: bool,
    /// 强制整缓冲 present（诊断开关 LEANTTY_PRESENT_FULL=1，release 与 debug 均可用，
    /// 默认关闭；用于对比脏区提交与全缓冲提交的 present 耗时，与 LEANTTY_PERF_FRAMES 同类）。
    present_full: bool,
    /// 渲染耗时 EMA（毫秒），用于自适应帧间隔；None 表示尚无样本。
    render_ema_ms: Option<f64>,
    /// 当前目标帧间隔（由 EMA 推导，最小 PACE_MIN_MS）。
    frame_interval: Duration,
    /// 帧耗时诊断采样（LEANTTY_PERF_FRAMES=1，release 与 debug 均可用，默认关闭）。
    /// 每 50 帧输出一行：渲染/present 中位与 p95、被合并丢弃的唤醒次数、当前帧间隔。
    perf_frames: Option<PerfFrames>,
    /// present 耗时采样（仅 debug：LEANTTY_PRESENT_STATS=1 每 100 帧打印一次）。
    #[cfg(debug_assertions)]
    present_stats: Option<(bool, Vec<u128>)>,
}

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let t_resumed = Instant::now();
        let config = self.config.take().expect("resumed 只能调用一次");
        // 阶段 9A：回退字体在**建窗前**就开始加载，与渲染器初始化/建窗/建 surface
        // 完全重叠（此前在窗口建好后才起线程，白等约 0.3s）；首帧仍不等字体。
        // 字体渲染器先行创建：格子尺寸由字体度量算出（任务2，替代固定 8x16）。
        // 此时窗口尚未创建，只能先建 CPU 渲染器用于估算；窗口创建后再按
        // renderer.backend 决定是否替换为 GPU 渲染器（必须在首个 present 之前）。
        let renderer = match create_renderer(
            &config.config,
            &config.font_settings,
            config.options,
            config.palette,
            None,
            None,
        ) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("create_renderer: {e}");
                return;
            }
        };
        // 初始格子按 scale=1.0 预估（winit 在 create_window 前不给真实 scale；
        // 创建后取真实 scale，差异时重算并 set_inner_size 修正）。
        let (init_cw, init_ch) = renderer.cell_metrics(1.0);
        let mut attrs = WindowAttributes::default();
        // 窗口标题初值：dynamic_title=false 固定为二进制名（任务 2C.4）；
        // true 时先给二进制名，OSC 0/2 出现后由 about_to_wait 更新。
        let bin_name = std::env::args()
            .next()
            .as_deref()
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string())
            })
            .unwrap_or_else(|| "leantty".to_string());
        attrs.title = bin_name.clone();
        // 阶段 5A t3：窗口图标（编译期内嵌 128x128 原始 RGBA，失败只警告不退出）。
        if let Some(icon) = window_icon() {
            attrs.window_icon = Some(icon);
        }
        // 阶段 5A t3：X11 的 WM_CLASS 与 Wayland 的 app_id 都固定为 "leantty"
        // （桌面环境据此匹配 .desktop 的 StartupWMClass / 图标）。
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // X11：WM_CLASS = (instance, class)；Wayland：app_id。两个扩展 trait 都叫
            // `with_name`，必须显式限定调用，不能同时 `use ... as _`。
            attrs = winit::platform::x11::WindowAttributesExtX11::with_name(
                attrs, "leantty", "leantty",
            );
            attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(
                attrs, "leantty", "leantty",
            );
        }
        // 初始位置（逻辑像素；Wayland 下合成器可能忽略——记录日志，不视为错误；
        // 越出屏幕范围交给窗口管理器处理，不自行钳制）。
        if let Some(pos) = config.config.window.position {
            attrs =
                attrs.with_position(winit::dpi::LogicalPosition::new(pos.x as f64, pos.y as f64));
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                eprintln!(
                    "window.position: Wayland 会话下合成器可能忽略初始位置（({}, {})）",
                    pos.x, pos.y
                );
            }
        }
        attrs.inner_size = Some(winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
            f64::from(config.cols) * f64::from(init_cw) + 2.0 * f64::from(config.pad_x),
            f64::from(config.rows) * f64::from(init_ch) + 2.0 * f64::from(config.pad_y),
        )));
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] pre_window_ms={:.1} since_start={:.1}",
                t_resumed.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        // 启动失败路径：记录原因并退出事件循环，不 panic（外部输入/系统调用须覆盖失败路径）。
        let t_window = Instant::now();
        let window = Rc::new(match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("create_window: {}", e);
                return;
            }
        });
        // 阶段 9D：判定窗口后端是否为 X11（决定 IME 锚点是否要下移一行）。
        // 用 raw handle 而不是猜环境变量：Win32/Xlib/Wayland 在 handle 类型上一目了然。
        {
            use raw_window_handle::HasWindowHandle;
            self.is_x11 = matches!(
                window.window_handle().map(|h| h.as_raw()),
                Ok(raw_window_handle::RawWindowHandle::Xlib(_))
            );
        }
        // window.position：winit 0.30 的 X11 后端在创建路径上不应用 attributes.position
        // （只在映射后经 set_outer_position 生效），因此创建后显式设置一次。
        // Wayland 下合成器可能忽略（已有提示日志）；越界交给窗口管理器。
        if let Some(pos) = config.config.window.position {
            window.set_outer_position(winit::dpi::LogicalPosition::new(pos.x as f64, pos.y as f64));
        }
        // 启动分段计时（阶段 3A 任务 2）：窗口创建与 surface 初始化。
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] window_ms={:.1} since_start={:.1}",
                t_window.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        // IME：窗口创建后无条件开启（任务 5）；Xvfb 无 IME 总线时事件不产生，
        // 开启调用本身无害（winit 内部按平台降级）。
        let t_ime = Instant::now();
        window.set_ime_allowed(true);
        // 阶段 4H t2：窗口创建完成到 renderer_ready 之间的逐段打点。
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] ime_ms={:.2} since_start={:.1}",
                t_ime.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let t_renderer = Instant::now();
        // 后端决定点：窗口已存在，auto/gpu 在此尝试创建 GPU 渲染器。
        // cpu 后端保持上面的 CPU 渲染器；GPU 失败时 auto 回落到它。
        let renderer = match create_renderer(
            &config.config,
            &config.font_settings,
            config.options,
            config.palette,
            Some(&window),
            Some(renderer),
        ) {
            Ok(r) => {
                if r.name() != "cpu" {
                    eperf!(
                        "[perf] backend={}（窗口创建后切换，首个 present 之前）",
                        r.name()
                    );
                }
                r
            }
            Err(e) => {
                // gpu 强制模式失败：记录 fatal 并退出事件循环（非零退出码）。
                eprintln!("create_renderer(gpu): {e}");
                self.fatal = Some(e);
                event_loop.exit();
                return;
            }
        };
        let gpu_backend = renderer.is_gpu();
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] renderer_build_ms={:.1} since_start={:.1} backend={}",
                t_renderer.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0,
                renderer.name()
            );
        }
        // 阶段 4C t3 启动打点：渲染器与其字形缓存初始化完成。
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] renderer_ready_ms={:.1} since_start={:.1}",
                t0.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let t_surface = Instant::now();
        // GPU 后端直接渲染到 winit 窗口 surface，**不创建** softbuffer context/surface
        // （同一窗口不允许同时挂两套 surface）。
        let mut surface = None;
        if !gpu_backend {
            let context = match softbuffer::Context::new(event_loop.owned_display_handle()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("softbuffer context: {}", e);
                    return;
                }
            };
            let mut s = match softbuffer::Surface::new(&context, window.clone()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("softbuffer surface: {}", e);
                    return;
                }
            };
            // 初始尺寸：softbuffer present 前必须 resize，否则 panic（Xvfb 复现实测）。
            let init_size = window.inner_size();
            self.last_size = Some((init_size.width, init_size.height));
            if let (Some(w), Some(h)) = (
                std::num::NonZeroU32::new(init_size.width),
                std::num::NonZeroU32::new(init_size.height),
            ) && let Err(e) = s.resize(w, h)
            {
                eprintln!("surface initial resize: {}", e);
                return;
            }
            surface = Some(s);
        } else {
            let init_size = window.inner_size();
            self.last_size = Some((init_size.width, init_size.height));
        }
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] surface_ms={:.1} since_start={:.1}",
                t_surface.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let t_vt = Instant::now();
        let mut vt = match VtTerminal::new_with_palette(
            config.cols,
            config.rows,
            &vt_palette(&config.config.colors),
        ) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("VtTerminal::new: {}", e);
                return;
            }
        };
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] vt_new_ms={:.2} since_start={:.1}",
                t_vt.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let t_spawn = Instant::now();
        let mut pty = NativePty::new();
        // 阶段 3U：用户配置的 terminal.shell.args 优先（None=未配置，Some([])=显式空数组）。
        // 阶段 7A：`-e/--command` 覆盖 program 与 args；args 一律作为 Some(向量)，
        // 使 Windows 侧 decide_shell_args 不注入 cmd.exe/powershell 的默认 chcp 参数。
        let (spawn_program, spawn_args) = match self.exec_command.as_ref() {
            Some(argv) => (
                argv.first().cloned().unwrap_or_default(),
                Some(argv[1..].to_vec()),
            ),
            None => {
                let args = config
                    .config
                    .terminal
                    .shell
                    .as_ref()
                    .and_then(|s| s.args.clone());
                (config.shell.clone(), args)
            }
        };
        if let Err(e) = pty.spawn_with_args(
            &spawn_program,
            spawn_args.as_deref(),
            config.cols,
            config.rows,
        ) {
            eprintln!("pty spawn: {}", e);
            return;
        }
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] pty_spawn_ms={:.1} since_start={:.1}",
                t_spawn.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        // 阶段 4H t3：前台程序不读 stdin 时同步写会阻塞主线程（release 实测 1MB
        // 粘贴停顿 12.9s）。取走可独立持有的写端，交给独立写线程；拿不到
        // （Windows 默认实现返回 None）则继续用同步写路径。
        self.writer = pty.take_writer().map(writer::PtyWriter::spawn);
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] writer_thread_ms={:.2} since_start={:.1} enabled={}",
                0.0,
                t0.elapsed().as_secs_f64() * 1000.0,
                self.writer.is_some()
            );
        }
        let mut reader = match pty.take_reader() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("take_reader: {}", e);
                return;
            }
        };
        // 唤醒器：读线程每来一次数据 wake 一次事件循环（channel + proxy 抽象）。
        let waker: Box<dyn Wake> = match self.event_proxy.take() {
            Some(proxy) => Box::new(ProxyWaker::new(proxy)),
            None => {
                eprintln!("create_proxy: no proxy available");
                return;
            }
        };
        // 任务 2B.1 / 阶段 9A：fallback 字体后台加载已在 `resumed` 开头（建窗前）发起，
        // 接收端见 `bg_rx`；线程安全：仅 `Vec<Arc<Font>>` 经 channel 传递，Database
        // 线程内自建，线程 detached（完成即退，无共享可变状态）。
        // 1. PTY 读线程 → channel → 主线程喂 vt
        // 有界通道（背压）：主线程忙时读线程阻塞在 send，进而 PTY 缓冲满、
        // 子进程写阻塞，而不是无限积压内存（DECISIONS 登记）。
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(256);
        let depth_for_reader = self.chan_depth.clone();
        let reader_thread = std::thread::spawn(move || {
            reader_body_with_depth(
                &mut reader,
                &tx,
                waker.as_ref(),
                &mut |m| eprintln!("{}", m),
                Some(depth_for_reader.as_ref()),
            );
        });
        self.reader_thread = Some(reader_thread);
        // 5. on_pty_write → channel → 主线程写 PTY
        let (pty_write_tx, pty_write_rx) = mpsc::channel::<Vec<u8>>();
        if let Err(e) = vt.on_pty_write(move |_term, data| {
            let _ = pty_write_tx.send(data.to_vec());
        }) {
            eprintln!("on_pty_write: {}", e);
            return;
        }
        // HiDPI：窗口创建后取真实 scale_factor，与预估不同则重算格子并修正窗口逻辑尺寸。
        let scale = window.scale_factor();
        let (cell_w, cell_h) = if (scale - 1.0).abs() > f64::EPSILON {
            let (cw, ch) = renderer.cell_metrics(scale);
            if window
                .request_inner_size(winit::dpi::LogicalSize::new(
                    f64::from(config.cols) * f64::from(cw) / scale + 2.0 * f64::from(config.pad_x),
                    f64::from(config.rows) * f64::from(ch) / scale + 2.0 * f64::from(config.pad_y),
                ))
                .is_none()
            {
                // winit 0.30：request_inner_size 返回 None 表示"请求已异步提交，
                // 结果稍后经 WindowEvent::Resized 返回"，不是被窗口管理器拒绝
                // （阶段 4B-fix1 任务 B 取证）。实际尺寸以 Resized 为准，
                // Resized 分支会按窗口真实像素反算 cols/rows 并 resize vt/pty。
                // 阶段 6B t4：这是信息性提示（不是失败），默认不输出；仅诊断开关下打印。
                eperf!("request_inner_size: 尺寸请求异步提交，实际尺寸以 Resized 事件为准");
            }
            (cw, ch)
        } else {
            (init_cw, init_ch)
        };
        self.cell_w = cell_w;
        self.cell_h = cell_h;
        self.scale_factor = scale;
        self.window = Some(window);
        self.surface = surface;
        self.vt = Some(vt);
        self.pty = Some(pty);
        self.renderer = Some(renderer);
        self.rx = Some(rx);
        self.pty_write_rx = Some(pty_write_rx);
        self.cols = config.cols;
        self.rows = config.rows;
        // 运行时绑定查表/选区参数访问（resumed take 后回填；AppHandler.config
        // 字段语义从"待消费配置"扩展为"运行时配置"，阶段 2B 任务 2.5 起用）。
        self.config = Some(config);
        // 测试专用开关（仅 debug 构建；release 中不存在）：LEANTTY_TEST_PREEDIT
        // 注入一个预编辑串，供 e2e 断言叠加层渲染而不经过真实输入法。
        #[cfg(debug_assertions)]
        if let Ok(t) = std::env::var("LEANTTY_TEST_PREEDIT")
            && !t.is_empty()
        {
            self.test_preedit = Some(t);
            // 配套测试钩子（debug 专有）：设置 LEANTTY_TEST_PREEDIT_CLEAR_MS 后，
            // 从注入时刻起经过该毫秒数即清除预编辑，用于验证"清除后不留残影"
            // （阶段 4B-fix1 任务 C 的 e2e 场景）。
            if let Ok(ms) = std::env::var("LEANTTY_TEST_PREEDIT_CLEAR_MS")
                && let Ok(ms) = ms.parse::<u64>()
            {
                self.test_preedit_clear_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                eperf!("[perf] test_preedit_clear_scheduled ms={ms}");
            }
        }
        // 测试钩子（debug 专有）：延迟触发一次同步粘贴，用于验证"粘贴后主动排帧"。
        #[cfg(debug_assertions)]
        if let Ok(t) = std::env::var("LEANTTY_TEST_PASTE")
            && !t.is_empty()
        {
            let at_ms = std::env::var("LEANTTY_TEST_PASTE_AT_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(2000);
            // `@<path>` 形式从文件读取（环境变量单值上限 MAX_ARG_STRLEN≈128KB，
            // 无法传 1MB 量级的粘贴素材；仅 debug 钩子，release 不含）。
            let t = match t.strip_prefix('@') {
                Some(path) => std::fs::read_to_string(path).unwrap_or_default(),
                None => t,
            };
            self.test_paste_text = Some(t);
            self.test_paste_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(at_ms));
        }
        self.clipboard = match ArboardClipboard::new() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("clipboard init: {}（剪贴板不可用，复制/粘贴降级）", e);
                None
            }
        };
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        self.got_pty = true;
        // 唤醒：读线程来数据后经 proxy 发来；about_to_wait 将消费 channel。
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        self.got_event = true;
        match event {
            // winit 0.30 把 X11 的 Expose（以及遮挡恢复、窗口首次映射）都映射为
            // RedrawRequested；本 app 的自驱动重绘走 about_to_wait 的节流逻辑，
            // 因此本分支只可能是"系统要求重绘"。必须整帧重绘 + 整窗提交，
            // 否则静止画面在遮挡/恢复后会留下残影（阶段 4B-fix1 任务 C）。
            WindowEvent::RedrawRequested => {
                // debug 钩子：记录 OS 触发的重绘次数（e2e 断言"整帧重绘路径被走到"，
                // 因为 Xvfb/openbox 有 backing store 时截图可能恒等，截图不足以证伪）。
                #[cfg(debug_assertions)]
                {
                    self.os_redraw_count += 1;
                    if self.os_redraw_count <= 3 {
                        eperf!("[perf] os_redraw #{}", self.os_redraw_count);
                    }
                }
                self.os_redraw_pending = true;
                self.urgent_render = true;
                if self.next_frame_origin == 0 {
                    self.next_frame_origin = 1; // OS 触发
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                self.next_frame_origin = 2; // 缩放因子变化也归类为缩放触发
                // HiDPI：重算格子物理尺寸 → InnerSizeWriter 保持 dimensions 行列数
                // （逻辑尺寸 = 行列*逻辑格子 + 2*padding）；随后 Resized 走统一路径
                // 重排网格并 resize vt/pty。字形位图缓存整体失效（set_scale_factor）。
                let (Some(renderer), Some(app_cfg)) =
                    (self.renderer.as_mut(), self.config.as_ref())
                else {
                    return;
                };
                renderer.set_scale_factor(scale_factor);
                let (cw, ch) = renderer.cell_metrics(scale_factor);
                self.cell_w = cw;
                self.cell_h = ch;
                self.scale_factor = scale_factor;
                if let Err(e) = inner_size_writer.request_inner_size(winit::dpi::PhysicalSize::new(
                    u32::from(app_cfg.cols) * cw
                        + 2 * u32::from(app_cfg.pad_x) * scale_factor as u32,
                    u32::from(app_cfg.rows) * ch
                        + 2 * u32::from(app_cfg.pad_y) * scale_factor as u32,
                )) {
                    eprintln!("request_inner_size: {}", e);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.urgent_render = true;
                self.on_mouse_input(state, button);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.urgent_render = true;
                self.on_cursor_moved(position);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.urgent_render = true;
                self.on_mouse_scroll(delta);
            }
            WindowEvent::Ime(ime) => {
                self.urgent_render = true;
                self.on_ime_event(ime);
            }
            WindowEvent::ModifiersChanged(m) => {
                let prev = self.mods_state;
                self.mods_state = m.state();
                if prev.alt_key() && !self.mods_state.alt_key() {
                    self.alt_released_at = Some(Instant::now());
                }
                self.trace_key(format!(
                    "mods shift={} ctrl={} alt={} super={}",
                    self.mods_state.shift_key(),
                    self.mods_state.control_key(),
                    self.mods_state.alt_key(),
                    self.mods_state.super_key()
                ));
            }
            // 阶段 9E：失焦时清空修饰键状态。`Alt+Tab` 切走后 Alt 的 release 事件会送到
            // 新获得焦点的窗口，我们这边永远收不到 → `mods_state` 卡在 ALT，之后每敲一个
            // 字符都会被编码成 `ESC` 前缀（表现为"切换回来打字出现怪字符"）。
            WindowEvent::Focused(focused) => {
                self.trace_key(format!("focus {focused}"));
                self.focused = focused;
                if focused {
                    self.focused_at = Some(Instant::now());
                }
                if !focused {
                    self.mods_state = ModifiersState::default();
                }
                self.urgent_render = true;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.trace_key(format!(
                    "key {:?} {:?} mods(shift={} ctrl={} alt={} super={}) text={:?}",
                    event.state,
                    event.logical_key,
                    self.mods_state.shift_key(),
                    self.mods_state.control_key(),
                    self.mods_state.alt_key(),
                    self.mods_state.super_key(),
                    event.text.as_deref().unwrap_or("")
                ));
                // 阶段 9E：切窗口过度期间的按键归属窗口管理器，不写进 PTY。
                if !self.focused {
                    return;
                }
                self.urgent_render = true;
                if event.state == ElementState::Pressed {
                    // 阶段 9E：窗口管理器保留的组合（Super 组合、Alt+Tab）不写进 PTY。
                    // 绑定查表之前先挡掉：这类组合属于桌面快捷键，即使将来有人把它们
                    // 写进 keyboard.bindings 也不该在我们这里生效。
                    let mapped_key = map_logical_key(&event.logical_key);
                    if wm_reserved_key(&self.mods_state, mapped_key)
                        || tab_is_alt_tab_tail(
                            mapped_key,
                            self.mods_state.alt_key(),
                            self.alt_released_at.map(|t| t.elapsed()),
                        )
                        || tab_is_focus_replay(
                            mapped_key,
                            self.mods_state.alt_key(),
                            self.mods_state.control_key(),
                            self.focused_at.map(|t| t.elapsed()),
                        )
                    {
                        return;
                    }
                    // 绑定执行优先（任务 2.5）：keyboard.bindings 查表（空则内置默认
                    // Ctrl+Shift+C/V）；命中即执行动作，不再走 route_key。
                    let kmods = config_mods(&self.mods_state);
                    let key_str = key_string(&event.logical_key);
                    let bound = key_str.as_deref().and_then(|k| {
                        self.config.as_ref().and_then(|c| {
                            if c.config.keyboard.bindings.is_empty() {
                                find_key_binding(&default_key_bindings(), k, &kmods)
                            } else {
                                find_key_binding(&c.config.keyboard.bindings, k, &kmods)
                            }
                        })
                    });
                    if let Some(action) = bound {
                        self.run_action(action);
                    } else {
                        let mods = vt_mods_from(&self.mods_state);
                        let mapped = map_logical_key(&event.logical_key);
                        let route = route_key(event.text.as_deref(), mapped, mods);
                        match route {
                            KeyRoute::Text(s) => {
                                self.follow_viewport_before_input(follow::InputEvent::Printable);
                                if let Some(pty) = self.pty.as_mut() {
                                    write_pty(pty, self.writer.as_ref(), s.as_bytes());
                                }
                            }
                            KeyRoute::Encoded { key, mods } => {
                                if let (Some(key), Some(vt)) = (key, self.vt.as_mut()) {
                                    match vt.key_encoder() {
                                        Ok(mut enc) => {
                                            // 阶段 6A：kitty 模式下必须提供 unshifted codepoint，
                                            // 否则组合键被上游编码器静默丢弃（Ctrl+D/P/C 全丢）。
                                            let unshifted = unshifted_codepoint(&event);
                                            // 阶段 9F：把按键文本一并交给编码器。ghostty 的
                                            // legacy 分支在 `Ctrl+<char>` 上要么用 `event.utf8`、
                                            // 要么用逻辑键自带码点；`Ctrl+Shift+字母/符号` 又进不了
                                            // `ctrlSeq`（unset_mods 不是 ctrl_only）→ 此前一个字节都不写
                                            // （实测 `Ctrl+Space`/`Ctrl+Shift+A`/`Ctrl+Shift+[` 全丢）。
                                            let text = event.text.as_deref();
                                            // 先按"不带 utf8"编码：保持 modifyOtherKeys 等既有模式的行为
                                            // （带 utf8 会让 mok2 从 `0x01` 变成 `ESC[27;5;97~`）。
                                            // 只有当这种编码**一个字节都不产出**时，才带 utf8 重试，
                                            // 用于修 `Ctrl+Shift+字母/符号`（上游只认 utf8 的那条路径）。
                                            let mut enc_result = enc.encode_key_full(
                                                key,
                                                mods,
                                                Action::Press,
                                                unshifted,
                                                None,
                                            );
                                            if matches!(&enc_result, Ok(b) if b.is_empty()) {
                                                enc_result = enc.encode_key_full(
                                                    key, mods, Action::Press, unshifted, text,
                                                );
                                            }
                                            match enc_result {
                                                Ok(bytes) => {
                                                    // 阶段 4I t2：Ctrl-C 取消进行中的粘贴。
                                                    // 写线程只能丢弃"已在队列里"的块（并补发括号
                                                    // 粘贴结束标记），但 app 侧的 pending_paste
                                                    // 会在下一帧继续回灌，粘贴实际没停（实测：
                                                    // 首次 Ctrl-C 后仍持续写入，第二次 Ctrl-C 又丢
                                                    // 1MiB）。中断语义要求粘贴整体作废，故此处清空。
                                                    // 阶段 6B t2j：legacy 模式下上游编码器对
                                                    // ctrl+[ 不产出字节（ctrl+space 已由编码器给出
                                                    // 0x00、ctrl+] \ / 分别给出 1d/1c/1f）。按裁决
                                                    // 仅对此一键按 xterm 规则回写 ESC，不新增通用表。
                                                    let bytes = if bytes.is_empty()
                                                        && mods.intersects(Mods::CTRL)
                                                        && !mods.intersects(Mods::ALT)
                                                        && key == vt::Key::BracketLeft
                                                    {
                                                        vec![0x1b]
                                                    } else {
                                                        bytes
                                                    };
                                                    // 阶段 9F：应用没启用 Kitty 键盘协议时，上游编码器
                                                    // 对 `Ctrl+Shift+<字符>` 仍会产出 `CSI u`（实测
                                                    // `Ctrl+Shift+A` → `ESC[97;6u`），传统应用会把这一串当
                                                    // 普通字符回显。此时按 xterm 口径回写控制字节
                                                    // （`Ctrl+Shift+A`→0x01、`Ctrl+Shift+[`→0x1b），与 alacritty 一致。
                                                    let bytes = if is_csi_u(&bytes)
                                                        && !self
                                                            .vt
                                                            .as_ref()
                                                            .is_some_and(|v| v.kitty_keyboard_enabled())
                                                    {
                                                        let ch = unshifted.or_else(|| {
                                                            event
                                                                .text
                                                                .as_deref()
                                                                .and_then(|t| t.chars().next())
                                                        });
                                                        match ch.and_then(ctrl_byte) {
                                                            Some(b) => vec![b],
                                                            None => bytes,
                                                        }
                                                    } else {
                                                        bytes
                                                    };
                                                    if bytes.contains(&0x03) {
                                                        self.pending_paste.clear();
                                                    }
                                                    self.follow_viewport_before_input(
                                                        follow::InputEvent::EncodedKey,
                                                    );
                                                    if let Some(pty) = self.pty.as_mut() {
                                                        write_pty(
                                                            pty,
                                                            self.writer.as_ref(),
                                                            &bytes,
                                                        );
                                                    }
                                                }
                                                Err(e) => eprintln!("encode: {}", e),
                                            }
                                        }
                                        Err(e) => eprintln!("key_encoder: {}", e),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::Resized(physical_size) => {
                self.apply_resize(physical_size.width, physical_size.height);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 阶段 4S t2：唤醒来源计数（loops / 定时 / 事件 / PTY）
        self.wake_counts.0 = self.wake_counts.0.saturating_add(1);
        if self.got_event {
            self.wake_counts.2 = self.wake_counts.2.saturating_add(1);
        } else if self.got_pty {
            self.wake_counts.3 = self.wake_counts.3.saturating_add(1);
        } else {
            self.wake_counts.1 = self.wake_counts.1.saturating_add(1);
        }
        self.got_event = false;
        self.got_pty = false;
        // 阶段 4S t2：**默认进入无限等待**。此前不重置控制流，上一轮设置的
        // WaitUntil（其时刻往往已过去）会被保留，winit 立刻返回 → about_to_wait
        // 再跑一轮 → 又渲染/再次返回，形成"每几毫秒一次"的空转（实测 cpu 空闲
        // 249 loops/s、wake_timer=100%、约 13% 单核）。下面各分支只在确有
        // 待办工作时再升级为 WaitUntil。
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        // 轮询真实窗口像素尺寸：WM 可能在映射时裁窗且不投递 Resized（实测 720p 屏幕上
        // 请求 1008px 被裁到 696px，导致渲染/滚动条按错误高度计算）。
        if let Some(w) = self.window.as_ref() {
            let sz = w.inner_size();
            if sz.width > 0
                && sz.height > 0
                && self.last_size != Some((sz.width, sz.height))
            {
                self.apply_resize(sz.width, sz.height);
            }
        }
        // 阶段 4I t3：观测窗口重置（LEANTTY_PERF_RESET_MS，默认关闭）。用于把启动期
        // 空闲/字体加载排除在"粘贴窗口"的停顿统计之外；release 同样可用。
        if let Some(t) = self.perf_reset_at
            && Instant::now() >= t
        {
            self.perf_reset_at = None;
            if let Some(pf) = self.perf_frames.as_mut() {
                pf.loop_gap_max_us = 0;
                pf.feed_ms_max = 0.0;
                pf.feed_bytes_max = 0;
                pf.writer_queued_max = 0;
                pf.chan_depth_max = 0;
                pf.coalesced = 0;
                pf.feed_bytes = 0;
                pf.window_start = Some(Instant::now());
            }
            self.last_loop_at = Some(Instant::now());
            self.loop_report_at = None;
            eperf!("[perf] window_reset");
        }
        // 阶段 4G t2a 观测：两次事件循环迭代的最大间隔（主线程停顿的直接度量）。
        // 仅在 LEANTTY_PERF_FRAMES 开启时统计（默认关闭，代价一次 Option 判空）。
        if self.perf_frames.is_some() {
            let now = std::time::Instant::now();
            if let Some(prev) = self.last_loop_at {
                let gap = now.duration_since(prev);
                if let Some(pf) = self.perf_frames.as_mut()
                    && gap.as_micros() > pf.loop_gap_max_us
                {
                    pf.loop_gap_max_us = gap.as_micros();
                }
            }
            self.last_loop_at = Some(now);
            // 每 5 秒独立输出一次（不依赖 50 帧窗口；画面静止时也能观测到停顿）。
            if self.loop_report_at.is_none_or(|t| now >= t) {
                let ms = self
                    .perf_frames
                    .as_ref()
                    .map(|p| p.loop_gap_max_us / 1000)
                    .unwrap_or(0);
                // 同时带上迭代级归因字段：慢渲染/少帧场景下 frames 行可能不出现，
                // 只有这条 5 秒线才能给出"停顿期间 feed 花了多少"。
                let (fms, fb, wq) = self
                    .perf_frames
                    .as_ref()
                    .map(|p| (p.feed_ms_max, p.feed_bytes_max, p.writer_queued_max))
                    .unwrap_or((0.0, 0, 0));
                let (fw, fos, frs, fot) = self.frames_window;
                let acq = match self.renderer.as_ref() {
                    Some(RendererBackend::Gpu(g)) => {
                        let a = g.acquire_stats();
                        format!(
                            " success={} suboptimal={} outdated={} lost={} other={}",
                            a.success, a.suboptimal, a.outdated, a.lost, a.other
                        )
                    }
                    _ => String::new(),
                };
                let (loops, wt, we, wp) = self.wake_counts;
                eperf!(
                    "[perf] loop_gap_ms={ms} feed_ms_max={fms:.1} feed_bytes_max={fb} writer_queued_max={wq} frames_window={fw} frames_os={fos} frames_resize={frs} frames_other={fot} loops={loops} wake_timer={wt} wake_event={we} wake_pty={wp}{acq}"
                );
                self.frames_window = (0, 0, 0, 0);
                self.wake_counts = (0, 0, 0, 0);
                if let Some(pf) = self.perf_frames.as_mut() {
                    pf.loop_gap_max_us = 0;
                }
                self.loop_report_at = Some(now + std::time::Duration::from_secs(5));
            }
        }
        // 测试钩子（debug 专有）：预编辑到期清除。必须放在渲染节流之前——静止画面
        // 时 will_render 可能为 false，若把清除逻辑放在渲染路径里就永远不会执行。
        #[cfg(debug_assertions)]
        if let Some(t) = self.test_paste_at {
            if std::time::Instant::now() >= t {
                self.test_paste_at = None;
                if let Some(text) = self.test_paste_text.take() {
                    eperf!("[perf] test_paste_fired");
                    self.paste_from_text(&text);
                    self.test_paste_fired = Some(std::time::Instant::now());
                    // 停顿统计从"粘贴已入队"这一刻重新起算，避免把启动期的空闲
                    // 间隔（事件循环等定时唤醒）算成本次粘贴的停顿。
                    if let Some(pf) = self.perf_frames.as_mut() {
                        pf.loop_gap_max_us = 0;
                    }
                    self.last_loop_at = Some(std::time::Instant::now());
                    self.loop_report_at = None;
                }
            } else {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(t));
            }
        }
        #[cfg(debug_assertions)]
        if let Some(t) = self.test_preedit_clear_at {
            if std::time::Instant::now() >= t {
                self.test_preedit = None;
                self.test_preedit_clear_at = None;
                eperf!("[perf] test_preedit_cleared");
                self.urgent_render = true;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            } else {
                // 静止画面时渲染路径不会再设 WaitUntil（due 分支直接返回），
                // 事件循环会永久 Wait，导致到期清除永远不执行（实测）。这里显式
                // 安排一次唤醒；渲染路径若设置了更早的时刻会覆盖本值（无害）。
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(t));
            }
        }
        // 大段粘贴排空（任务 2D.2 修复）：每轮按预算写出有限字节，剩余数据
        // 通过 request_redraw 在下一帧继续驱动。
        // 原缺陷：每轮只写一块且写完不自驱动，事件循环进入 Wait 后不再运行，
        // 静止场景下粘贴停在第 1 块（Linux 实测 65536 字节后停住）。
        if !self.pending_paste.is_empty() {
            let mut report = crate::paste::DrainReport::default();
            let mut pty_gone = false;
            match self.pty.as_mut() {
                Some(_pty) if self.writer.is_some() => {
                    // 写线程路径：入队即返回，队列满则本轮到此为止（下一帧继续）。
                    report = drain_to_writer(
                        &mut self.pending_paste,
                        self.writer.as_ref().expect("writer 已判定存在"),
                        crate::paste::DrainBudget::default(),
                    );
                }
                Some(pty) => match pty.writer() {
                    Ok(w) => {
                        report = drain_pending(
                            &mut self.pending_paste,
                            w,
                            crate::paste::DrainBudget::default(),
                        );
                        // flush 失败（如对端暂不可写）保留剩余，下一轮重试。
                        if let Err(e) = w.flush()
                            && e.kind() == std::io::ErrorKind::WouldBlock
                        {
                            report.would_block = true;
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "paste drain: pty writer 不可用（{}），丢弃剩余 {} 块",
                            e,
                            self.pending_paste.len()
                        );
                        pty_gone = true;
                    }
                },
                None => pty_gone = true,
            }
            if let Some(w) = self.writer.as_ref()
                && let Some(pf) = self.perf_frames.as_mut()
            {
                let q = w.queued_bytes();
                if q > pf.writer_queued_max {
                    pf.writer_queued_max = q;
                }
            }
            if pty_gone {
                // 窗口/PTY 已关闭：未发完的数据丢弃，不挂起事件循环。
                self.pending_paste.clear();
            } else if report.needs_more(self.pending_paste.is_empty())
                && let Some(w) = self.window.as_ref()
            {
                // 主动请求下一帧，避免事件循环空闲停摆（不忙等：每帧只写预算量）。
                // 同时置 urgent：排空期间内容在变，下一帧应尽快到来（4B-fix3）。
                self.urgent_render = true;
                w.request_redraw();
            }
        }
        // 2. channel 收字节 → 喂 vt（auto-follow：开启时新输出后回底）
        if let Some(rx) = self.rx.as_ref()
            && let Some(vt) = self.vt.as_mut()
        {
            let mut n_drained = 0usize;
            // 阶段 4I t3：单次迭代内 feed 的总耗时与字节数（归因"主线程停在哪"）。
            let t_feed = Instant::now();
            let mut iter_feed_bytes = 0u64;
            while let Ok(bytes) = rx.try_recv() {
                if !self.first_pty_byte_logged {
                    self.first_pty_byte_logged = true;
                    if let Some(t0) = crate::start_time() {
                        eperf!(
                            "[perf] first_pty_byte_ms={} since_start={}",
                            t0.elapsed().as_millis(),
                            t0.elapsed().as_millis()
                        );
                    }
                }
                #[cfg_attr(not(debug_assertions), allow(unused_variables))]
                let n = bytes.len();
                vt.feed(&bytes);
                n_drained += 1;
                // 阶段 4C t3 观测：累计喂入字节（release 亦可用，仅一次 Option 判空）
                self.chan_depth
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                iter_feed_bytes += n as u64;
                if let Some(pf) = self.perf_frames.as_mut() {
                    pf.feed_bytes += n as u128;
                    if pf.window_start.is_none() {
                        pf.window_start = Some(std::time::Instant::now());
                    }
                }
            }
            // 单次迭代 feed 峰值（只有真的喂过数据才记录）
            if iter_feed_bytes > 0 {
                let ms = t_feed.elapsed().as_secs_f64() * 1000.0;
                if let Some(pf) = self.perf_frames.as_mut() {
                    if ms > pf.feed_ms_max {
                        pf.feed_ms_max = ms;
                    }
                    if iter_feed_bytes > pf.feed_bytes_max {
                        pf.feed_bytes_max = iter_feed_bytes;
                    }
                }
            }
            // channel 积压峰值（有界通道：reader 侧入队 +1、这里取走 -1）
            let depth = self.chan_depth.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(pf) = self.perf_frames.as_mut()
                && depth > pf.chan_depth_max
            {
                pf.chan_depth_max = depth;
            }
            // 供后续步骤复用：本轮是否收到新数据（避免每帧多次全量 snapshot）。
            self.drained_any = n_drained > 0;
        }
        // auto-follow：开启时新输出后回底（仅在有新数据时，避免每帧全量 snapshot）。
        if self.drained_any && self.follow_bottom {
            if let Some(vt) = self.vt.as_mut() {
                vt.scroll_viewport(ScrollViewport::Bottom);
            }
            self.offset_from_top = self.sb_rows();
        }
        // scrollback 截断（超 history）致选区行消失：clamp 到可见历史边界并记日志。
        if let Some(sel) = self.selection {
            let (_, _, _, max_abs) = sel.region();
            let sb = self.sb_rows();
            let visible_max = sb + self.rows as usize;
            if max_abs as usize > visible_max {
                // 阶段 4S t2：**限速**。拖选 + 持续输出时该分支每帧成立，实测 6 分钟
                // soak 里打印 17.5 万行（490 行/秒），stderr 写入本身把主线程拖出
                // 1708ms 的停顿（loop_gap 观测）。语义不变，只把同一警告限到 1 秒一次。
                let now = Instant::now();
                let should_log = self
                    .last_sel_clamp_log
                    .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1));
                if should_log {
                    self.last_sel_clamp_log = Some(now);
                    eprintln!(
                        "[selection] scrollback 截断：选区末行 {} 超出可见历史边界 {}，clamp（同一警告每秒最多一条）",
                        max_abs, visible_max
                    );
                }
                self.selection = Some(Selection {
                    anchor: sel.anchor,
                    focus: CellPos {
                        col: sel.focus.col,
                        row: visible_max.min(max_abs as usize) as u16,
                    },
                });
            }
        }
        // 5. vt 响应字节 → 写 PTY
        if let Some(pty_write_rx) = self.pty_write_rx.as_ref()
            && let Some(pty) = self.pty.as_mut()
        {
            while let Ok(bytes) = pty_write_rx.try_recv() {
                write_pty(pty, self.writer.as_ref(), &bytes);
            }
        }
        // 6. 子进程退出 → 关闭窗口
        if let Some(pty) = self.pty.as_mut() {
            self.last_liveness_check = Some(Instant::now());
            match pty.try_wait() {
                Ok(Some(_status)) => event_loop.exit(),
                Ok(None) => {}
                Err(e) => {
                    eprintln!("try_wait: {}", e);
                    event_loop.exit();
                }
            }
        }
        // 帧合并（阶段 3B 任务 2）：PTY 输出已在上方一次性排空并全部 feed，
        // 这里把渲染频率限制到约 8ms 一帧；交互事件（键盘/鼠标/IME/resize）置
        // urgent_render 走低延迟路径。未到期时用 WaitUntil 安排下一次唤醒，
        // 避免画面停更。
        #[cfg(debug_assertions)]
        if self.no_render {
            self.finish_wake(event_loop, None);
            return;
        }
        #[cfg(debug_assertions)]
        let throttle_disabled = self.no_throttle;
        #[cfg(not(debug_assertions))]
        let throttle_disabled = false;
        let now = Instant::now();
        // 自适应帧间隔（阶段 3C 任务 4）：输出路径用 frame_interval（K×EMA，clamp 8–100ms）；
        // 输入等交互帧不受自适应间隔拖延，但仍受 RENDER_MIN_INTERVAL（8ms）下限约束，
        // 避免输出洪峰时输入帧背靠背执行。
        let wait_needed = match self.last_render {
            Some(t) => {
                let elapsed = now.duration_since(t);
                if self.urgent_render {
                    elapsed < RENDER_MIN_INTERVAL
                } else {
                    elapsed < self.frame_interval
                }
            }
            None => false,
        };
        let due = throttle_disabled || !wait_needed;
        if !due {
            if let Some(pf) = self.perf_frames.as_mut() {
                pf.coalesced += 1;
            }
            if let Some(t) = self.last_render {
                // 尾沿保证：约定在"到期时刻"唤醒一次，输出停止后最后一帧一定会被渲染。
                let target = if self.urgent_render {
                    t + RENDER_MIN_INTERVAL
                } else {
                    t + self.frame_interval
                };
                self.finish_wake(event_loop, Some(target));
            } else {
                self.finish_wake(event_loop, None);
            }
            return;
        }
        let prev_render = self.last_render;
        self.last_render = Some(now);
        // 本帧是否有 "紧急" 触发（输入/粘贴/OS 重绘等）：release 也需要（空闲判定用）
        let urgent_before = self.urgent_render;
        #[cfg(debug_assertions)]
        let was_urgent = self.urgent_render;
        self.urgent_render = false;
        // 阶段 4T t2c：**每帧维护工作前移**。以下两段（OSC 标题跟随、拖选自动滚动推进）
        // 原先位于渲染块之后，而"GPU 空闲跳过"与"CPU 空 damage 提前返回"都会 return，
        // 把它们一并跳过——实测 GPU 后端上拖选自动滚动完全不推进、OSC 0 标题也不更新。
        // 前移到渲染之前后，无论本帧是否真正绘制，维护逻辑都会执行。
        // 窗口标题（任务 2C.4）：dynamic_title=true 时跟随 OSC 0/2；
        // false 时保持二进制名（resumed 已设，后续不再更新）。
        let dynamic_title = self
            .config
            .as_ref()
            .map(|c| c.config.window.dynamic_title)
            .unwrap_or(false);
        if dynamic_title
            && let (Some(vt), Some(window)) = (self.vt.as_ref(), self.window.as_ref())
            && let Ok(Some(title)) = vt.title()
            && self.last_title.as_deref() != Some(title.as_str())
        {
            window.set_title(&title);
            self.last_title = Some(title);
        }
        // 拖选自动滚动：每帧按方向推进 offset 并扩展 focus 到新视口边界行
        // （任务 2C.3；释放后 auto_scroll=0 自然停止）。
        if self.mouse_down && self.auto_scroll != 0 {
            let sb = self.sb_rows();
            let (new_offset, focus_abs) =
                auto_scroll_step(self.offset_from_top, self.auto_scroll, sb, self.rows);
            self.offset_from_top = new_offset;
            self.follow_bottom = new_offset >= sb;
            if let Some(sel) = self.selection.as_mut() {
                sel.set_focus(CellPos {
                    col: sel.focus.col,
                    row: focus_abs as u16,
                });
            }
            if let Some(vt) = self.vt.as_mut() {
                vt.scroll_viewport(ScrollViewport::Row(new_offset));
            }
            // 事件循环无事件时会阻塞在 epoll（about_to_wait 不再触发），
            // 主动请求重绘驱动下一帧，使自动滚动持续到释放。
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }

        let t_render_start = Instant::now();
        // 阶段 4K t2：内存观测在借用 vt/renderer/surface 之前取一次。
        let mem = self.mem_summary();
        // 渲染一帧（尺寸取缓存，窗口销毁后不查询 X11 geometry）
        let sb_render = self.sb_rows();
        if let (Some(vt), Some(renderer)) = (self.vt.as_ref(), self.renderer.as_mut()) {
            // GPU 后端直接渲染到窗口 surface（无 softbuffer），CPU 后端必须已有 surface。
            let is_gpu = renderer.is_gpu();
            let mut surface_guard = if is_gpu { None } else { self.surface.as_mut() };
            if !is_gpu && surface_guard.is_none() {
                self.finish_wake(event_loop, None);
                return;
            }
            match vt.snapshot_incremental() {
                Ok(mut snap) => {
                    // 叠加层脏合并（阶段 3A 任务 3）：libghostty 只上报行内容脏，
                    // 选区（反色）与视口滚动需 app 侧补齐。
                    // 视口滚动：整屏重绘（不做位移复用，记录为后续优化点）。
                    if self.offset_from_top != self.last_render_offset {
                        snap.full_redraw = true;
                        self.last_render_offset = self.offset_from_top;
                    }
                    // OS 发起的重绘（Expose/遮挡恢复）：整帧重绘（full_redraw 会让
                    // 所有行进 damage，CPU 侧等价整窗提交；GPU 本就整帧 present）。
                    if self.os_redraw_pending {
                        self.os_redraw_pending = false;
                        snap.full_redraw = true;
                    }
                    // 选区变化：旧区域与新区域的所有行标脏。
                    // 阶段 8A S2：流式（阅读顺序）选区 + 零长度不绘制（单击不留痕迹）。
                    let vis_region = self.selection.and_then(|s| {
                        if s.anchor == s.focus {
                            return None;
                        }
                        let top = self.offset_from_top as u16;
                        let bottom = top.saturating_add(self.rows.saturating_sub(1));
                        let clamp = |p: CellPos| -> Option<(u16, u16)> {
                            if p.row < top {
                                Some((top, 0))
                            } else if p.row > bottom {
                                Some((bottom, self.cols.saturating_sub(1)))
                            } else {
                                Some((p.row, p.col))
                            }
                        };
                        let (ar, ac) = clamp(s.anchor)?;
                        let (fr, fc) = clamp(s.focus)?;
                        Some((ar.saturating_sub(top), ac, fr.saturating_sub(top), fc))
                    });
                    if vis_region != self.last_sel_region {
                        if let Some((ar, _, fr, _)) = self.last_sel_region {
                            snap.mark_rows_dirty(ar.min(fr), ar.max(fr));
                        }
                        if let Some((ar, _, fr, _)) = vis_region {
                            snap.mark_rows_dirty(ar.min(fr), ar.max(fr));
                        }
                        self.last_sel_region = vis_region;
                    }
                    // 滚动条几何（提前算：既用于空闲判定，也用于随后的 set_scrollbar）
                    let geom_now: Option<(u32, u32)> = {
                        let sb = sb_render;
                        if sb == 0 {
                            None
                        } else {
                            scrollbar_geometry(
                                sb as u64,
                                u64::from(self.rows),
                                self.offset_from_top,
                                self.last_size.map(|(_, h)| h).unwrap_or(16),
                            )
                            .map(|g| (g.thumb_y, g.thumb_h))
                        }
                    };
                    // 阶段 4Q t3：**空闲帧不渲染**。
                    // 帧节流每帧后都会排一次 WaitUntil（尾沿保证），静止画面也会被定时
                    // 唤醒；CPU 后端此时"渲染但不提交"（damage 为空即返回），GPU 后端却会
                    // 真正整帧 present → 空闲持续占用 GPU（Windows 实测 17 秒 1050 帧、
                    // 每帧约 5ms、feed_bytes=0）。这里在渲染前判定：只有"有输入/有脏行/
                    // 整屏重绘/OS 重绘/选区或滚动条变化"才画。
                    if is_gpu
                        && !urgent_before
                        && !snap.full_redraw
                        && snap.is_clean()
                        && geom_now == self.last_geom_drawn
                    {
                        self.update_ime_cursor_area();
                        self.finish_wake(event_loop, None);
                        return;
                    }
                    // debug-only 强制全量（e2e 对拍用；release 不含该分支）
                    #[cfg(debug_assertions)]
                    if self.force_full_redraw {
                        snap.full_redraw = true;
                    }
                    let (pw, ph) = self.last_size.unwrap_or((8, 16));
                    renderer.set_selection(vis_region);
                    // 滚动条几何（scrollback=0 时不绘制）——已在空闲判定前算好
                    let geom = geom_now;
                    renderer.set_scrollbar(geom);
                    self.last_geom_drawn = geom;
                    // IME 预编辑叠加层（不写入网格；空串=清除）。
                    // debug 测试注入（LEANTTY_TEST_PREEDIT）优先于真实 Ime 事件。
                    #[cfg(debug_assertions)]
                    let test_text = self.test_preedit.clone();
                    #[cfg(not(debug_assertions))]
                    let test_text: Option<String> = None;
                    let overlay = if let Some(t) = test_text {
                        Some(render::PreeditOverlay::new(t, None))
                    } else if self.ime_preedit.is_empty() {
                        None
                    } else {
                        Some(render::PreeditOverlay::new(
                            self.ime_preedit.clone(),
                            self.ime_preedit_cursor,
                        ))
                    };
                    renderer.set_preedit(overlay);
                    #[cfg(debug_assertions)]
                    dump_snapshot_if_requested(&snap, renderer.as_cpu());
                    // 阶段 4C t3 启动打点：首帧提交前一刻（仅首次）
                    if !self.pre_first_submit_logged
                        && let Some(t0) = crate::start_time()
                    {
                        self.pre_first_submit_logged = true;
                        eperf!(
                            "[perf] pre_first_submit_ms={} since_start={}",
                            t0.elapsed().as_millis(),
                            t0.elapsed().as_millis()
                        );
                    }
                    // 阶段 4Q t3：只统计**真正绘制**的帧（空闲跳过不计入），并记来源。
                    self.frames_window.0 = self.frames_window.0.saturating_add(1);
                    match self.next_frame_origin {
                        1 => self.frames_window.1 = self.frames_window.1.saturating_add(1),
                        2 => self.frames_window.2 = self.frames_window.2.saturating_add(1),
                        _ => self.frames_window.3 = self.frames_window.3.saturating_add(1),
                    }
                    self.next_frame_origin = 0;
                    if let Err(e) = renderer.render(&snap, pw, ph) {
                        eprintln!("render: {}", e);
                    }
                    let render_ms = t_render_start.elapsed().as_secs_f64() * 1000.0;
                    self.render_ema_ms = Some(next_ema(self.render_ema_ms, render_ms));
                    self.frame_interval = target_interval(self.render_ema_ms.unwrap_or(render_ms));
                    if let Some(pf) = self.perf_frames.as_mut() {
                        pf.render_us.push((render_ms * 1000.0) as u128);
                    }
                    // GPU 后端：render() 已把这一帧直接 present 到窗口 surface，
                    // 没有脏区拷贝步骤（surface 纹理内容不保留，每帧整帧重绘）。
                    if is_gpu {
                        if let Some(pf) = self.perf_frames.as_mut() {
                            // GPU present 已在 render_ms 内，这里只维护帧计数与间隔。
                            pf.present_us.push(0);
                            pf.interval_ms = prev_render
                                .map(|t| now.duration_since(t).as_millis() as u64)
                                .unwrap_or(0);
                            if pf.present_us.len() >= PerfFrames::N {
                                pf.report(&mem);
                            }
                        }
                        if !self.first_frame_logged {
                            self.first_frame_logged = true;
                            self.trace_geom("first_frame");
                            if let Some(t0) = crate::start_time() {
                                eperf!("[perf] first_frame_ms={}", t0.elapsed().as_millis());
                            }
                        }
                        self.update_ime_cursor_area();
                        self.finish_wake(event_loop, None);
                        return;
                    }
                    // 本帧脏区（阶段 3A 任务 3）：只拷贝并提交脏区；
                    // 无脏区则跳过 present（静止画面零提交）。
                    let damage: Vec<(u32, u32, u32, u32)> = renderer.damage().to_vec();
                    let fb = renderer.framebuffer();
                    let fb_w = pw;
                    let Some(surface) = surface_guard.as_mut() else {
                        self.finish_wake(event_loop, None);
                        return;
                    };
                    // LEANTTY_PRESENT_FULL=1 时强制整缓冲提交（诊断用，release 亦可用）。
                    let force_full_present = self.present_full;
                    if !force_full_present && damage.is_empty() {
                        // 完全静态帧：不重绘、不提交（仍可更新 perf 日志）。
                        if !self.first_frame_logged {
                            self.first_frame_logged = true;
                            if let Some(t0) = crate::start_time() {
                                eperf!("[perf] first_frame_ms={}", t0.elapsed().as_millis());
                            }
                        }
                        self.update_ime_cursor_area();
                        self.finish_wake(event_loop, None);
                        return;
                    }
                    match surface.buffer_mut() {
                        Ok(mut buffer) => {
                            let t_present = Instant::now();
                            // 拷贝：damage 覆盖整屏时整缓冲拷贝，否则按矩形逐行拷贝。
                            let full_cover = force_full_present
                                || damage.len() == 1 && damage[0] == (0, 0, pw, ph);
                            if full_cover {
                                for (i, px) in fb.iter().enumerate() {
                                    if i < buffer.len() {
                                        buffer[i] = *px;
                                    }
                                }
                            } else {
                                for &(x, y, w, h) in &damage {
                                    for row in y..(y + h).min(ph) {
                                        let src_start = (row * fb_w + x) as usize;
                                        let src_end =
                                            (row * fb_w + x + w).min((row + 1) * fb_w) as usize;
                                        let dst_start = (row * pw + x) as usize;
                                        if src_end <= src_start || dst_start >= buffer.len() {
                                            continue;
                                        }
                                        let n = (src_end - src_start).min(buffer.len() - dst_start);
                                        buffer[dst_start..dst_start + n]
                                            .copy_from_slice(&fb[src_start..src_start + n]);
                                    }
                                }
                            }
                            // 提交：可用 present_with_damage 时只提交脏矩形。
                            let present_res = if force_full_present || full_cover {
                                buffer.present()
                            } else {
                                let rects: Vec<softbuffer::Rect> = damage
                                    .iter()
                                    .filter_map(|&(x, y, w, h)| {
                                        Some(softbuffer::Rect {
                                            x,
                                            y,
                                            width: std::num::NonZeroU32::new(w)?,
                                            height: std::num::NonZeroU32::new(h)?,
                                        })
                                    })
                                    .collect();
                                // present_with_damage 在不支持 damage 的后端等价于 present。
                                buffer.present_with_damage(&rects)
                            };
                            if let Some(pf) = self.perf_frames.as_mut() {
                                pf.present_us.push(t_present.elapsed().as_micros());
                                pf.interval_ms = prev_render
                                    .map(|t| now.duration_since(t).as_millis() as u64)
                                    .unwrap_or(0);
                                // 每 50 帧输出一行帧耗时诊断（release 保留）。
                                if pf.present_us.len() >= PerfFrames::N {
                                    pf.report(&mem);
                                }
                            }
                            #[cfg(debug_assertions)]
                            if let Some((full_flag, samples)) = self.present_stats.as_mut() {
                                samples.push(t_present.elapsed().as_micros());
                                if samples.len() >= 20 {
                                    let mut v = samples.clone();
                                    v.sort_unstable();
                                    eperf!(
                                        "[perf] present_us n=20 full={} median={} p95={}",
                                        full_flag,
                                        v[v.len() / 2],
                                        v[v.len() * 95 / 100]
                                    );
                                    samples.clear();
                                }
                            }
                            if let Err(e) = present_res {
                                eprintln!("surface present: {}", e);
                            }
                            #[cfg(debug_assertions)]
                            if let Some(cpu) = renderer.as_cpu() {
                                dump_frame_hash_if_requested(cpu);
                            }
                            if !self.first_frame_logged {
                                self.first_frame_logged = true;
                                if let Some(t0) = crate::start_time() {
                                    eperf!("[perf] first_frame_ms={}", t0.elapsed().as_millis());
                                }
                            }
                            // 测试钩子：粘贴后的首帧延迟与是否走 urgent 路径
                            #[cfg(debug_assertions)]
                            if let Some(fired) = self.test_paste_fired.take() {
                                eperf!(
                                    "[perf] test_paste_first_frame latency_ms={} urgent={}",
                                    fired.elapsed().as_millis(),
                                    was_urgent
                                );
                            }
                        }
                        Err(e) => eprintln!("buffer_mut: {}", e),
                    }
                }
                Err(e) => eprintln!("snapshot: {}", e),
            }
        }
        // IME 光标区跟随快照光标；该调用内部会取快照，故仅在本次确实要渲染时执行
        // （高频输出下每帧多一次全量快照会显著拖慢 channel 排空）。
        let will_render = match self.last_render {
            Some(t) => {
                self.urgent_render || Instant::now().duration_since(t) >= RENDER_MIN_INTERVAL
            }
            None => true,
        };
        if will_render {
            self.update_ime_cursor_area();
        }
        // 阶段 4V t2：**唯一出口**。存活检查、渲染节流、debug 钩子定时的到期时刻在这里
        // 合并取最早者，再决定控制流；所有提前 return 都经 `finish_wake` 走同一逻辑。
        // 迁此之前，存活唤醒登记只在函数末尾，任一提前 return（GPU 空闲跳过、CPU 无脏区
        // 跳过 present 等）都会跳过它 → 子进程退出后不再有唤醒（Windows 3S 现象）。
        self.finish_wake(event_loop, None);
    }
}

impl AppHandler {
    /// 应用窗口像素尺寸变化：重算网格、resize vt/pty、更新 surface 与缓存尺寸。
    ///
    /// 除 `WindowEvent::Resized` 外，`about_to_wait` 还会**轮询** `window.inner_size()`
    /// （窗口管理器可能在映射时悄悄裁窗而不发事件——实测 1280x720 屏幕上 24 行请求
    /// 1008px 被裁到 696px，若不轮询，渲染与滚动条仍按 1008px 计算，滚动条画到窗口外）。
    fn apply_resize(&mut self, width: u32, height: u32) {
        self.next_frame_origin = 2;
        self.urgent_render = true;
        let pad_x = u32::from(self.pad_x) * self.scale_factor as u32;
        let pad_y = u32::from(self.pad_y) * self.scale_factor as u32;
        let Some((cols, rows)) = resize_target(width, height, self.cell_w, self.cell_h, pad_x, pad_y)
        else {
            #[cfg(debug_assertions)]
            eperf!("[perf] resize_ignored {width}x{height}（小于一个字符格）");
            return;
        };
        let unchanged = (cols, rows) == (self.cols, self.rows);
        if !unchanged {
            if let Some(vt) = self.vt.as_mut()
                && let Err(e) = vt.resize(cols, rows)
            {
                eprintln!("vt resize: {}", e);
            }
            if let Some(pty) = self.pty.as_mut()
                && let Err(e) = pty.resize(cols, rows)
            {
                eprintln!("pty resize: {}", e);
            }
            self.cols = cols;
            self.rows = rows;
        }
        self.last_size = Some((width, height));
        if let Some(r) = self.renderer.as_mut()
            && r.is_gpu()
        {
            r.resize_surface(width, height);
        }
        if let Some(surface) = self.surface.as_mut()
            && let (Some(w), Some(h)) = (
                std::num::NonZeroU32::new(width),
                std::num::NonZeroU32::new(height),
            )
            && let Err(e) = surface.resize(w, h)
        {
            eprintln!("surface resize: {}", e);
        }
    }

    /// 存活检查的下一次到期时刻（阶段 4V t2，无副作用）。
    ///
    /// Windows 恒启用（ConPTY 输出管道不会因 shell 退出而 EOF，只能轮询 `try_wait`）；
    /// 其他平台仅在诊断开关 `LEANTTY_FORCE_LIVENESS_POLL=1` 打开时启用，默认 `None`，
    /// 即保持纯事件驱动、空闲零唤醒。
    fn liveness_deadline(&self, now: Instant) -> Option<Instant> {
        if !liveness_poll_wanted() || self.pty.is_none() {
            return None;
        }
        next_liveness_wake(now, true, self.last_liveness_check, Duration::from_secs(1))
    }

    /// 本帧仍需保持的定时唤醒（debug 钩子；release 恒为 `None`）。
    fn pending_timer_at(&self) -> Option<Instant> {
        #[cfg(debug_assertions)]
        {
            [self.test_paste_at, self.test_preedit_clear_at]
                .into_iter()
                .flatten()
                .min()
        }
        #[cfg(not(debug_assertions))]
        None
    }

    /// 唤醒决策的唯一出口（阶段 4V t2）：合并"存活检查"与"其它定时待办"，取最早者。
    ///
    /// `timer_at` 为本帧已确定的其它定时到期时刻（渲染节流目标、debug 钩子）；存活检查
    /// 到期时刻在此处按当前时间重新计算，避免用到陈旧的过期值。
    fn finish_wake(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        timer_at: Option<Instant>,
    ) {
        let now = Instant::now();
        let liveness_at = self.liveness_deadline(now);
        // debug 钩子的定时待办在这里再合并一次：提前 return 的路径传 `timer_at = None`，
        // 若不在内部补上，会把上面钩子块设置的 WaitUntil 覆盖成无限 Wait（4V 实测：
        // case30/32/39 的钩子定时全丢，粘贴/预编辑钩子不再触发）。
        let hook_at = self.pending_timer_at();
        match next_wake(now, liveness_at, timer_at, hook_at) {
            Some(t) => {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(t));
            }
            None => {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            }
        }
    }

    /// 鼠标是否命中滚动条条带（右缘 4px；条带判定优先于选区坐标转换）。
    fn hit_scrollbar(&self, position: Option<winit::dpi::PhysicalPosition<f64>>) -> bool {
        let Some(pos) = position else {
            return false;
        };
        let Some((w, _h)) = self.last_size else {
            return false;
        };
        pos.x >= w.saturating_sub(render::SCROLLBAR_WIDTH) as f64 && w > render::SCROLLBAR_WIDTH
    }

    /// 物理像素坐标 → 网格坐标（含 padding；cell_w/h 已含 scale，越界返 None）。
    fn pixel_to_cell(&self, position: winit::dpi::PhysicalPosition<f64>) -> Option<CellPos> {
        let x = position.x - f64::from(self.pad_x) * self.scale_factor;
        let y = position.y - f64::from(self.pad_y) * self.scale_factor;
        let col = (x / f64::from(self.cell_w.max(1))) as i64;
        let row = (y / f64::from(self.cell_h.max(1))) as i64;
        if col < 0 || row < 0 || col >= i64::from(self.cols) || row >= i64::from(self.rows) {
            return None;
        }
        Some(CellPos {
            col: col as u16,
            row: row as u16,
        })
    }

    /// 鼠标按钮事件：上报优先（非 Shift 且 vt 开跟踪→编码写 PTY）；
    /// 否则走选区（左键）与绑定（中/右键）。Shift 绕过上报。
    fn on_mouse_input(&mut self, state: ElementState, button: MouseButton) {
        let shift = self.mods_state.shift_key();
        if !shift
            && self
                .vt
                .as_ref()
                .is_some_and(|v| v.is_mouse_tracking().unwrap_or(false))
        {
            // 位置先算（避免与下方 vt/pty 可变借用冲突）。
            let pos = self
                .last_mouse_pos
                .and_then(|p| self.pixel_to_cell(p))
                .map(|c| vt::Position {
                    x: f32::from(c.col),
                    y: f32::from(c.row),
                })
                .unwrap_or(vt::Position { x: 0.0, y: 0.0 });
            let mods = vt_mods_from(&self.mods_state);
            let action = match state {
                ElementState::Pressed => vt::MouseAction::Press,
                ElementState::Released => vt::MouseAction::Release,
            };
            let btn = match button {
                MouseButton::Left => Some(vt::Button::Left),
                MouseButton::Right => Some(vt::Button::Right),
                MouseButton::Middle => Some(vt::Button::Middle),
                _ => None,
            };
            if let Some(vt) = self.vt.as_mut()
                && let Ok(mut enc) = vt.mouse_encoder()
                && let Some(pty) = self.pty.as_mut()
            {
                match enc.encode_button(action, btn, mods, pos) {
                    Ok(bytes) => write_pty(pty, self.writer.as_ref(), &bytes),
                    Err(e) => eprintln!("mouse encode: {}", e),
                }
            }
            return;
        }
        if state != ElementState::Pressed {
            if button == MouseButton::Left {
                self.scrollbar_drag = false;
                self.mouse_down = false;
                self.auto_scroll = 0;
                if self.selection.is_some() {
                    self.commit_selection();
                }
            }
            return;
        }
        match button {
            MouseButton::Left => {
                // 滚动条条带命中（右缘 4px）→ 拖拽模式，优先于选区。
                if self.hit_scrollbar(self.last_mouse_pos) {
                    self.scrollbar_drag = true;
                    if let Some(y) = self.last_mouse_pos.map(|p| p.y) {
                        let track_h = self.last_size.map(|(_, h)| h).unwrap_or(0);
                        let sb = self.sb_rows();
                        self.offset_from_top =
                            track_y_to_offset(y as u32, track_h, sb as u64).min(sb);
                        self.follow_bottom = self.offset_from_top >= sb;
                        if let Some(vt) = self.vt.as_mut() {
                            vt.scroll_viewport(ScrollViewport::Row(self.offset_from_top));
                        }
                    }
                    return;
                }
                if shift {
                    // Shift+点击：扩展选区 focus（anchor 不动）；行号转绝对坐标。
                    let pos = self.last_mouse_pos.and_then(|p| self.pixel_to_cell(p));
                    if let (Some(sel), Some(pos)) = (self.selection.as_mut(), pos) {
                        sel.set_focus(CellPos {
                            col: pos.col,
                            row: viewport_to_abs(pos.row, self.offset_from_top) as u16,
                        });
                    }
                } else {
                    self.start_selection();
                }
            }
            MouseButton::Middle => self.run_mouse_binding(ConfigMouse::Middle),
            MouseButton::Right => self.run_mouse_binding(ConfigMouse::Right),
            _ => {}
        }
    }

    /// 鼠标移动：更新缓存坐标；拖选中扩展 focus；跟踪模式下 Motion 上报。
    fn on_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        self.last_mouse_pos = Some(position);
        // 拖选自动滚动方向（任务 2C.3）：指针到窗口上/下沿 12px 内时每帧推进。
        if self.mouse_down && !self.scrollbar_drag {
            let h = self.last_size.map(|(_, h)| h).unwrap_or(0);
            self.auto_scroll = if position.y < 12.0 {
                -1
            } else if h > 24 && position.y > f64::from(h - 12) {
                1
            } else {
                0
            };
        }
        // 滚动条拖拽中：offset 实时更新（优先于选区扩展）。
        if self.scrollbar_drag {
            let track_h = self.last_size.map(|(_, h)| h).unwrap_or(0);
            let sb = self.sb_rows();
            self.offset_from_top = track_y_to_offset(position.y as u32, track_h, sb as u64).min(sb);
            self.follow_bottom = self.offset_from_top >= sb;
            if let Some(vt) = self.vt.as_mut() {
                vt.scroll_viewport(ScrollViewport::Row(self.offset_from_top));
            }
            return;
        }
        let pos = self.pixel_to_cell(position);
        if self.mouse_down
            && let Some(sel) = self.selection.as_mut()
            && let Some(pos) = pos
        {
            // 拖拽扩展：行号转绝对坐标（滚动时选区跟随内容）。
            sel.set_focus(CellPos {
                col: pos.col,
                row: viewport_to_abs(pos.row, self.offset_from_top) as u16,
            });
        }
        if !self.mods_state.shift_key()
            && self.mouse_down
            && self
                .vt
                .as_ref()
                .is_some_and(|v| v.is_mouse_tracking().unwrap_or(false))
        {
            let pos = self
                .pixel_to_cell(position)
                .map(|c| vt::Position {
                    x: f32::from(c.col),
                    y: f32::from(c.row),
                })
                .unwrap_or(vt::Position { x: 0.0, y: 0.0 });
            let mods = vt_mods_from(&self.mods_state);
            if let Some(vt) = self.vt.as_mut()
                && let Ok(mut enc) = vt.mouse_encoder()
                && let Some(pty) = self.pty.as_mut()
                && let Ok(bytes) = enc.encode_button(vt::MouseAction::Motion, None, mods, pos)
            {
                write_pty(pty, self.writer.as_ref(), &bytes);
            }
        }
    }

    /// IME 事件（任务 5）：Commit 归一化写 PTY；Preedit 存字段（渲染层显示
    /// 组合下划线本阶段未实现，DECISIONS 登记限制）；Disabled 清空并更新光标区。
    fn on_ime_event(&mut self, event: winit::event::Ime) {
        use winit::event::Ime;
        let (changed, text, cursor) = ime_preedit_update(&event);
        if changed {
            self.ime_preedit = text;
            self.ime_preedit_cursor = cursor;
        }
        if let Ime::Commit(s) = &event {
            // Commit 文本归一化（\n→\r）后写 PTY；Preedit 不写 PTY。
            let norm = normalize_ime_commit(s);
            if !norm.is_empty() {
                self.follow_viewport_before_input(follow::InputEvent::ImeCommit);
                if let Some(pty) = self.pty.as_mut() {
                    write_pty(pty, self.writer.as_ref(), norm.as_bytes());
                }
            }
        }
        self.update_ime_cursor_area();
    }

    /// 几何诊断（`LEANTTY_GEOM=1`）：打印窗口像素、scale、格子尺寸、vt 行列与视口偏移。
    /// 用于定位"首行空一行 / 同配置比 alacritty 大"这类只在特定机器上出现的差异。
    fn trace_geom(&self, why: &str) {
        if std::env::var_os("LEANTTY_GEOM").is_none() {
            return;
        }
        let (vw, vh) = self.last_size.unwrap_or((0, 0));
        let snap = self.vt.as_ref().and_then(|v| v.snapshot().ok());
        let (vcols, vrows) = snap.as_ref().map(|s| (s.cols, s.rows)).unwrap_or((0, 0));
        let (cur_x, cur_y) = snap
            .as_ref()
            .map(|s| (s.cursor.x, s.cursor.y))
            .unwrap_or((0, 0));
        let row0: String = snap
            .as_ref()
            .and_then(|s| s.cells.first())
            .map(|r| r.iter().map(|c| c.chars.as_str()).collect::<String>().trim_end().to_string())
            .unwrap_or_default();
        let ncells = snap.as_ref().map(|s| s.cells.len()).unwrap_or(0);
        let sb = snap.as_ref().map(|s| s.scrollback_rows).unwrap_or(0);
        eprintln!(
            "[geom] {why} cursor=({cur_x},{cur_y}) cells_len={ncells} sb_rows={sb} rows_field={vrows} row0_len={} row0={:?}",
            row0.chars().count(),
            row0.chars().take(24).collect::<String>()
        );
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "-".into());
        eprintln!(
            "[geom] {why} TERM={} SHELL={} COLUMNS={} LINES={} GDK_SCALE={} cwd={}",
            env("TERM"),
            env("SHELL"),
            env("COLUMNS"),
            env("LINES"),
            env("GDK_SCALE"),
            std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "-".into())
        );
        eprintln!(
            "[geom] {why} window={vw}x{vh} scale={:.3} cell={}x{} cfg_cols={} cfg_rows={} vt={}x{} offset={} pad={}x{}",
            self.scale_factor,
            self.cell_w,
            self.cell_h,
            self.cols,
            self.rows,
            vcols,
            vrows,
            self.offset_from_top,
            self.pad_x,
            self.pad_y
        );
    }

    /// 键盘诊断：`LEANTTY_KEY_TRACE` 设置时把事件追加到文件（每行立即 flush）。
    fn trace_key(&mut self, line: String) {
        use std::io::Write as _;
        let Some(f) = self.key_trace.as_mut() else {
            return;
        };
        let ms = crate::start_time()
            .map(|t| t.elapsed().as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "{ms} {line}");
        let _ = f.flush();
    }

    /// 更新 IME 光标区（快照光标所在格像素矩形；窗口未建时跳过）。
    ///
    /// 与 alacritty 对齐的三处细节（阶段 9D）：
    /// 1. 传**物理像素**（`PhysicalPosition/PhysicalSize`）；此前传的是"已乘过 scale 的值 +
    ///    Logical" → 在 HiDPI 下会被 winit 再乘一次 scale，锚点整体偏移。
    /// 2. X11 的 XIM 只接受一个 spot 点、不支持矩形，故把 y 下移**一行**（锚在光标格底边），
    ///    否则候选窗直接盖住正在输入的那一行。
    /// 3. 宽度取**两格**：部分合成器/IM 面板在可用宽度太窄时会把候选窗甩到窗口右下角。
    fn update_ime_cursor_area(&self) {
        let (Some(window), Some(vt)) = (self.window.as_ref(), self.vt.as_ref()) else {
            return;
        };
        let Ok(snap) = vt.snapshot() else {
            return;
        };
        // Preedit 期间 IME 候选窗跟随组合末尾（任务 2C.3），否则跟随终端光标。
        let (cy_cell, cx_cell) = ime_cursor_cell(
            &self.ime_preedit,
            snap.cursor.x,
            snap.cursor.y,
            snap.cols,
            snap.rows,
        );
        let (x, y, w, h) = ime_area_rect(
            ImeGeometry {
                pad_x: self.pad_x,
                pad_y: self.pad_y,
                scale: self.scale_factor,
                cell_w: self.cell_w,
                cell_h: self.cell_h,
                is_x11: self.is_x11,
            },
            cx_cell,
            cy_cell,
        );
        window.set_ime_cursor_area(
            winit::dpi::PhysicalPosition::new(x, y),
            winit::dpi::PhysicalSize::new(w, h),
        );
    }

    /// 滚轮（任务 3/4）：鼠标跟踪模式下上报 Four/Five（非 Shift 优先）；备用屏且
    /// alt_screen_scroll 时发方向键；否则 scroll_viewport Delta（×multiplier）。
    fn on_mouse_scroll(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_x, y) => y as isize,
            MouseScrollDelta::PixelDelta(pos) => (pos.y / f64::from(self.cell_h.max(1))) as isize,
        };
        if lines == 0 {
            return;
        }
        // 滚轮上报优先级：非 Shift 且跟踪开启→Button Four（上）/Five（下）编码写 PTY。
        if !self.mods_state.shift_key()
            && self
                .vt
                .as_ref()
                .is_some_and(|v| v.is_mouse_tracking().unwrap_or(false))
        {
            let btn = if lines > 0 {
                vt::Button::Four
            } else {
                vt::Button::Five
            };
            let n = lines.unsigned_abs();
            for _ in 0..n {
                let pos = self
                    .last_mouse_pos
                    .and_then(|p| self.pixel_to_cell(p))
                    .map(|c| vt::Position {
                        x: f32::from(c.col),
                        y: f32::from(c.row),
                    })
                    .unwrap_or(vt::Position { x: 0.0, y: 0.0 });
                let mods = vt_mods_from(&self.mods_state);
                if let (Some(vt), Some(pty)) = (self.vt.as_mut(), self.pty.as_mut())
                    && let Ok(mut enc) = vt.mouse_encoder()
                    && let Ok(bytes) =
                        enc.encode_button(vt::MouseAction::Press, Some(btn), mods, pos)
                {
                    write_pty(pty, self.writer.as_ref(), &bytes);
                }
                if let (Some(vt), Some(pty)) = (self.vt.as_mut(), self.pty.as_mut())
                    && let Ok(mut enc) = vt.mouse_encoder()
                    && let Ok(bytes) =
                        enc.encode_button(vt::MouseAction::Release, Some(btn), mods, pos)
                {
                    write_pty(pty, self.writer.as_ref(), &bytes);
                }
            }
            return;
        }
        // 备用屏+alt_screen_scroll：方向键（Up=上滚/Down=下滚）。
        let alternate = self
            .vt
            .as_ref()
            .map(|v| {
                v.snapshot()
                    .map(|snap| snap.alternate_screen)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if alternate
            && self
                .config
                .as_ref()
                .is_some_and(|c| c.config.scrolling.alt_screen_scroll)
        {
            let mult = self
                .config
                .as_ref()
                .map(|c| c.config.scrolling.multiplier)
                .unwrap_or(3.0)
                .max(1.0) as isize;
            let key = if lines > 0 {
                vt::Key::ArrowUp
            } else {
                vt::Key::ArrowDown
            };
            let n = lines.unsigned_abs() as isize * mult;
            for _ in 0..n {
                if let (Some(vt), Some(pty)) = (self.vt.as_mut(), self.pty.as_mut())
                    && let Ok(mut enc) = vt.key_encoder()
                    && let Ok(bytes) = enc.encode_key(key, Mods::empty(), Action::Press)
                    && let Ok(bytes2) = enc.encode_key(key, Mods::empty(), Action::Release)
                {
                    write_pty(pty, self.writer.as_ref(), &bytes);
                    write_pty(pty, self.writer.as_ref(), &bytes2);
                }
            }
            return;
        }
        // 主屏（或 alt_screen_scroll=false）：offset 状态机+scroll_viewport Row 绝对定位
        // （实测：vt 上滚后 feed 新输出不自动回底，DECISIONS 登记；offset 由 app 维护）。
        let mult = self
            .config
            .as_ref()
            .map(|c| c.config.scrolling.multiplier)
            .unwrap_or(3.0)
            .max(1.0) as isize;
        let delta = lines.unsigned_abs() as isize * mult;
        let sb = self
            .vt
            .as_ref()
            .and_then(|v| v.snapshot().ok())
            .map(|snap| snap.scrollback_rows as isize)
            .unwrap_or(0);
        if lines > 0 {
            // 上滚：向 scrollback 顶（offset 减小）
            self.offset_from_top = (self.offset_from_top as isize - delta).max(0) as usize;
        } else {
            // 下滚：向活动区（offset 增大）
            self.offset_from_top = (self.offset_from_top as isize + delta).min(sb) as usize;
        }
        self.follow_bottom = self.offset_from_top as isize >= sb;
        if let Some(vt) = self.vt.as_mut() {
            vt.scroll_viewport(ScrollViewport::Row(self.offset_from_top));
        }
    }

    /// 左键按下新选区：双击选词（<300ms 同格）/三击选行/单格。
    fn start_selection(&mut self) {
        let Some(pos) = self.last_mouse_pos.and_then(|p| self.pixel_to_cell(p)) else {
            return;
        };
        // 选区坐标=内容绝对行（视口行+offset；滚动后选区附着内容不动）。
        let abs_pos = CellPos {
            col: pos.col,
            row: viewport_to_abs(pos.row, self.offset_from_top) as u16,
        };
        let now = Instant::now();
        let count = match self.last_click {
            Some((t, p)) if t.elapsed() < Duration::from_millis(300) && p == pos => {
                self.click_count.saturating_add(1)
            }
            _ => 1,
        };
        self.click_count = count;
        self.last_click = Some((now, abs_pos));
        self.mouse_down = true;
        let region = match count {
            2 => self
                .word_region_at(pos.row, pos.col)
                .map(|(sc, ec)| Selection {
                    anchor: CellPos {
                        col: sc,
                        row: abs_pos.row,
                    },
                    focus: CellPos {
                        col: ec,
                        row: abs_pos.row,
                    },
                }),
            3 => Some(Selection {
                anchor: CellPos {
                    col: 0,
                    row: abs_pos.row,
                },
                focus: CellPos {
                    col: self.cols.saturating_sub(1),
                    row: abs_pos.row,
                },
            }),
            _ => Some(Selection::new(abs_pos)),
        };
        // 单击（count==1）不留可见标记，并清除既有选区；拖动时 focus 变化才出现。
        match region {
            Some(r) if count == 1 => {
                self.selection = Some(r);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            Some(r) => self.selection = Some(r),
            None => self.selection = None,
        }
    }

    /// 双击选词：以 semantic_escape_chars 为边界计算词区域（网格坐标）。
    fn word_region_at(&self, viewport_row: u16, focus_col: u16) -> Option<(u16, u16)> {
        let snap = self.vt.as_ref()?.snapshot().ok()?;
        // 绝对行=viewport_row+offset（调用方传视口行）；读视口内行文本。
        let row = viewport_row;
        let line = Self::row_text(&snap, row);
        let escape = self
            .config
            .as_ref()
            .map(|c| c.config.selection.semantic_escape_chars.clone())
            .unwrap_or_default();
        let (sc, ec) = word_region(&line, focus_col as usize, &escape);
        Some((sc as u16, ec as u16))
    }

    /// 快照行文本（Cell.chars 拼接；Spacer 尾格跳过——与 extract_text 同源）。
    fn row_text(snap: &vt::FrameSnapshot, row: u16) -> String {
        let mut line = String::new();
        if let Some(cells) = snap.cells.get(row as usize) {
            for cell in cells {
                if matches!(
                    cell.wide,
                    vt::CellWide::SpacerTail | vt::CellWide::SpacerHead
                ) {
                    continue;
                }
                line.push_str(&cell.chars);
            }
        }
        line
    }

    /// 选区定型/复制：提取文本写 PRIMARY；save_to_clipboard 时兼写 CLIPBOARD。
    fn commit_selection(&mut self) {
        let Some(sel) = self.selection else {
            return;
        };
        let Some(vt) = self.vt.as_ref() else {
            return;
        };
        let Ok(snap) = vt.snapshot() else {
            return;
        };
        // 阶段 8A S2：按流式（阅读顺序）逐行取列范围；零长度选区不产生文本。
        let (anchor, focus) = (sel.anchor, sel.focus);
        if anchor == focus {
            return;
        }
        let offset = self.offset_from_top;
        let viewport_rows = self.rows as usize;
        let cols = self.cols;
        let top_abs = anchor.row.min(focus.row);
        let bot_abs = anchor.row.max(focus.row);
        let in_viewport =
            (top_abs as usize) >= offset && (bot_abs as usize) < offset + viewport_rows;
        let text = if in_viewport {
            let v0 = (top_abs as usize - offset).min(u16::MAX as usize) as u16;
            let v1 = (bot_abs as usize - offset).min(u16::MAX as usize) as u16;
            let rows: Vec<(&[Cell], bool)> = (v0..=v1)
                .filter_map(|r| {
                    snap.cells.get(r as usize).map(|cs| {
                        (
                            cs.as_slice(),
                            snap.wrapped.get(r as usize).copied().unwrap_or(false),
                        )
                    })
                })
                .collect();
            let spans: Vec<(u16, u16)> = (top_abs..=bot_abs)
                .filter_map(|row| stream_row_range(anchor, focus, row, cols))
                .collect();
            extract_text_stream(&rows, &spans, cols)
        } else {
            match vt.rows_text(top_abs as usize, bot_abs as usize, cols) {
                Ok(history) => {
                    let rows_ref: Vec<(&[Cell], bool)> =
                        history.iter().map(|(cs, w)| (cs.as_slice(), *w)).collect();
                    let spans: Vec<(u16, u16)> = (top_abs..=bot_abs)
                        .filter_map(|row| stream_row_range(anchor, focus, row, cols))
                        .collect();
                    extract_text_stream(&rows_ref, &spans, cols)
                }
                Err(e) => {
                    eprintln!("rows_text: {}", e);
                    String::new()
                }
            }
        };
        if text.is_empty() {
            return;
        }
        let Some(clip) = self.clipboard.as_mut() else {
            return;
        };
        if let Err(e) = clip.set(ClipboardKind::Primary, &text) {
            eprintln!("copy primary: {}", e);
        }
        let save = self
            .config
            .as_ref()
            .is_some_and(|c| c.config.selection.save_to_clipboard);
        if save && let Err(e) = clip.set(ClipboardKind::Clipboard, &text) {
            eprintln!("copy clipboard: {}", e);
        }
    }

    /// 鼠标绑定执行：配置查表（为空则内置默认 Middle=Copy/Right=Paste）。
    fn run_mouse_binding(&mut self, button: ConfigMouse) {
        let mods = config_mods(&self.mods_state);
        let action = self.config.as_ref().and_then(|c| {
            if c.config.mouse.bindings.is_empty() {
                find_binding(&default_mouse_bindings(), button, &mods)
            } else {
                find_binding(&c.config.mouse.bindings, button, &mods)
            }
        });
        // [插术] case13 取证
        // config 分支走向取证插桩已撤（case13 根因=AppHandler.config take 后未回填）。
        if let Some(a) = action {
            self.run_action(a);
        }
    }

    /// 绑定动作执行（复制/粘贴/滚动；None 无操作）。
    fn run_action(&mut self, action: ConfigAction) {
        match action {
            ConfigAction::Copy => self.commit_selection(),
            ConfigAction::Paste => self.do_paste(ClipboardKind::Clipboard),
            ConfigAction::PasteSelection => self.do_paste(ClipboardKind::Primary),
            ConfigAction::ScrollPageUp => {
                let sb = self.sb_rows();
                self.offset_from_top = self
                    .offset_from_top
                    .saturating_sub(self.rows as usize)
                    .min(sb);
                self.follow_bottom = false;
                if let Some(vt) = self.vt.as_mut() {
                    vt.scroll_viewport(ScrollViewport::Row(self.offset_from_top));
                }
            }
            ConfigAction::ScrollPageDown => {
                let sb = self.sb_rows();
                self.offset_from_top = (self.offset_from_top + self.rows as usize).min(sb);
                self.follow_bottom = self.offset_from_top >= sb;
                if let Some(vt) = self.vt.as_mut() {
                    vt.scroll_viewport(ScrollViewport::Row(self.offset_from_top));
                }
            }
            ConfigAction::ScrollToTop => {
                self.offset_from_top = 0;
                self.follow_bottom = false;
                if let Some(vt) = self.vt.as_mut() {
                    vt.scroll_viewport(ScrollViewport::Row(0));
                }
            }
            ConfigAction::ScrollToBottom => {
                let sb = self.sb_rows();
                self.offset_from_top = sb;
                self.follow_bottom = true;
                if let Some(vt) = self.vt.as_mut() {
                    vt.scroll_viewport(ScrollViewport::Bottom);
                }
            }
            ConfigAction::None => {}
        }
    }

    /// 内存相关观测摘要（阶段 4K t2）：随 5 秒帧日志输出，用于把 RSS 增长归到具体结构。
    ///
    /// 全部为只读取数、无分配以外副作用；release 可用。
    fn mem_summary(&self) -> String {
        let sb_rows = self
            .vt
            .as_ref()
            .map(|v| v.scrollback_rows().unwrap_or(0))
            .unwrap_or(0);
        let (glyphs, fb_bytes) = match self.renderer.as_ref() {
            Some(RendererBackend::Cpu(c)) => c.cache_stats(),
            // GPU 后端不回读帧缓冲：字形缓存复用同一份 Font 链，缓存规模不可直接取。
            // （Gpu 变体仅 Linux 存在，此处按平台门控，保证 Windows 目标可编译。）
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Some(RendererBackend::Gpu(_)) => (0, 0),
            None => (0, 0),
        };
        let pending = self.pending_paste.len();
        let pending_bytes: usize = self.pending_paste.iter().map(|c| c.len()).sum();
        let (wq_cap, wq_len, wq_bytes) = match self.writer.as_ref() {
            Some(w) => (w.capacity(), w.queued_len(), w.queued_bytes()),
            None => (0, 0, 0),
        };
        format!(
            "sb_rows={sb_rows} glyphs={glyphs} fb_bytes={fb_bytes} pending={pending}/{pending_bytes}B wq_cap={wq_cap} wq_len={wq_len} wq_bytes={wq_bytes}"
        )
    }

    /// 输入发往 PTY 前的**唯一入口**：按纯决策把视口拉回底部（阶段 4K t1b）。
    ///
    /// 修复前没有任何"输入→回底"逻辑，回底只由"新输出到达且 follow_bottom"触发；
    /// 滚轮上滚会把 follow_bottom 置 false，于是键入只写 PTY、视口停在历史位置。
    /// 已在底部时本函数无副作用（不 request_redraw），避免每键一帧。
    fn follow_viewport_before_input(&mut self, ev: follow::InputEvent) {
        let alt = self
            .vt
            .as_ref()
            .map(|v| v.is_alternate_screen().unwrap_or(false))
            .unwrap_or(false);
        let mouse = self
            .vt
            .as_ref()
            .map(|v| v.is_mouse_tracking().unwrap_or(false))
            .unwrap_or(false);
        if !follow::should_follow(ev, alt, mouse) {
            return;
        }
        if self.follow_bottom {
            return;
        }
        self.follow_bottom = true;
        self.offset_from_top = self.sb_rows();
        if let Some(vt) = self.vt.as_mut() {
            vt.scroll_viewport(ScrollViewport::Bottom);
        }
        self.urgent_render = true;
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn sb_rows(&self) -> usize {
        self.vt
            .as_ref()
            .and_then(|v| v.snapshot().ok())
            .map(|snap| snap.scrollback_rows)
            .unwrap_or(0)
    }

    /// 粘贴执行：读剪贴板→规范化（?2004 包裹）→写 PTY；≥1MB 分块入队逐帧写。
    fn do_paste(&mut self, kind: ClipboardKind) {
        let text = match self.clipboard.as_mut() {
            Some(c) => match c.get(kind) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("paste get: {}", e);
                    return;
                }
            },
            None => return,
        };
        if text.is_empty() {
            return;
        }
        self.paste_from_text(&text);
    }

    /// 完整粘贴路径：按当前括号粘贴模式规范化后写入 PTY 并主动排一帧。
    ///
    /// `do_paste`（剪贴板）与 debug 测试钩子 `LEANTTY_TEST_PASTE` 都走这里，
    /// 保证门禁验证的是生产路径（含 `ESC[200~ … ESC[201~` 包裹）。
    fn paste_from_text(&mut self, text: &str) {
        let bracketed = self
            .vt
            .as_ref()
            .map(|v| v.is_bracketed_paste().unwrap_or(false))
            .unwrap_or(false);
        // 顺序很重要（阶段 4L t1）：先**包裹**（normalize_paste 加 ESC[200~/ESC[201~），
        // 再按积压上限做**整条接受/拒绝**判定，最后才入队。绝不截断——截断会切掉结束
        // 标记（readline 卡在粘贴状态）并可能切开 UTF-8 字符。
        let norm = normalize_paste(text, bracketed);
        if !self.accept_paste_or_reject(norm.len()) {
            // 被整条拒绝：没有输入发往 PTY，视口不改变（规格）。
            return;
        }
        self.paste_accepted(&norm);
    }

    /// 粘贴的积压判定（唯一入口）：整条接受返回 true；超限整条拒绝并记日志。
    ///
    /// 全有或全无——不做部分接受，从而保证"要么整段（含括号粘贴结束标记）入队，
    /// 要么一个字节都不写"。这条判定被 <1MB 与 ≥1MB 两条路径共用。
    fn accept_paste_or_reject(&mut self, bytes_len: usize) -> bool {
        let backlog: usize = self.pending_paste.iter().map(|c| c.len()).sum();
        if crate::paste::accepts_whole(bytes_len, backlog, crate::paste::PASTE_BACKLOG_CAP) {
            return true;
        }
        // 单条粘贴本身超过上限与"积压已满"分开标出，便于排查
        let oversized = bytes_len > crate::paste::PASTE_BACKLOG_CAP;
        eperf!(
            "[perf] paste_rejected bytes={bytes_len} backlog={backlog} cap={} oversized={oversized}",
            crate::paste::PASTE_BACKLOG_CAP
        );
        false
    }

    /// 已通过积压判定的粘贴：**唯一入口**。
    ///
    /// 阶段 4M t2：回底判定收敛到这里，而不是散落在各调用点——此前 <1MB 路径
    /// （`paste_text_accepted`）漏了 `follow_viewport_before_input(InputEvent::Paste)`，
    /// 只有 ≥1MB 的分块路径调了，表现为"上滚后小粘贴不回底"（Windows 3J 实测）。
    /// 被整条拒绝的粘贴不会走到这里（拒绝 = 没有输入发往 PTY = 不动视口）。
    fn paste_accepted(&mut self, norm: &str) {
        // 回底判定的唯一来源（接受 + 非空 + 非备用屏 + 非鼠标上报模式才回底）。
        let alt = self
            .vt
            .as_ref()
            .map(|v| v.is_alternate_screen().unwrap_or(false))
            .unwrap_or(false);
        let mouse = self
            .vt
            .as_ref()
            .map(|v| v.is_mouse_tracking().unwrap_or(false))
            .unwrap_or(false);
        if follow::paste_returns_to_bottom(norm.len(), true, alt, mouse) {
            self.follow_viewport_before_input(follow::InputEvent::Paste);
        }
        if norm.is_empty() {
            return;
        }
        if norm.len() >= 1024 * 1024 {
            for chunk in paste_chunks(norm) {
                self.pending_paste.push_back(chunk.into_bytes());
            }
            self.urgent_render = true;
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
            return;
        }
        self.paste_text_accepted(norm);
    }

    /// 小粘贴（<1MB）：有界入队（剩余转 pending_paste）。
    ///
    /// 阶段 4B-fix3：这条路径不产生应用侧状态变化，若不置 urgent，画面刷新只能等
    /// PTY 回显字节到达后由帧间隔驱动；回显缺失/延迟时（shell 关闭 echo、readline
    /// 尚未处理）会出现"内容已送入 shell 但画面不刷新，直到回车或再次键入"。
    fn paste_text_accepted(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut rest = None;
        match self.writer.as_ref() {
            Some(w) => {
                let n = w.try_enqueue_paste(bytes);
                if n < bytes.len() {
                    rest = Some(bytes[n..].to_vec());
                }
            }
            None => {
                // 无写线程（Windows ConPTY 默认实现）：沿用同步写，行为不变。
                if let Some(pty) = self.pty.as_mut() {
                    write_pty(pty, None, bytes);
                }
            }
        }
        if let Some(tail) = rest {
            self.pending_paste.push_back(tail);
        }
        self.urgent_render = true;
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

/// 合并各项"到期时刻"，得出下一次唤醒（阶段 4V t2，纯函数、无 I/O）。
///
/// 输入为三类候选：
/// - `liveness`：存活检查的下一次轮询时刻（Windows 恒有；其他平台仅诊断开关打开时）；
/// - `render`：渲染节流的下一次到期时刻；
/// - `paste`：粘贴排空等其它定时待办。
///
/// 返回 `None` 表示无需唤醒（事件循环进入无限 `Wait`，保持纯事件驱动）；否则返回其中
/// **最早**的时刻；已经过去的时刻归一化为 `now`（立即唤醒一次，避免忙等重试）。
pub fn next_wake(
    now: std::time::Instant,
    liveness: Option<std::time::Instant>,
    render: Option<std::time::Instant>,
    paste: Option<std::time::Instant>,
) -> Option<std::time::Instant> {
    [liveness, render, paste]
        .into_iter()
        .flatten()
        .min()
        .map(|t| if t <= now { now } else { t })
}

/// Windows 子进程存活检查的下一次唤醒时刻（阶段 4T t3，纯函数）。
///
/// 背景：子进程退出检测是 `about_to_wait` 里轮询 `Pty::try_wait`（事件驱动）。Linux 上
/// 读端在子进程退出时返回 EOF/EIO，读线程会 `wake()` 唤醒主线程，因此轮询会被触发；
/// 而 Windows 的 ConPTY **输出管道不会因子进程退出而 EOF**，没有唤醒源，轮询就不会发生
/// → 窗口不会关闭。故 Windows 下按固定间隔安排低频存活检查。
///
/// 返回 `None` 表示无需唤醒（子进程已退出，退出路径已接管）。子进程存活期间最多每
/// `interval` 唤醒一次；距上次检查已超过 `interval` 时立即唤醒一次。
pub fn next_liveness_wake(
    now: std::time::Instant,
    child_alive: bool,
    last_check: Option<std::time::Instant>,
    interval: std::time::Duration,
) -> Option<std::time::Instant> {
    if !child_alive {
        return None;
    }
    match last_check {
        None => Some(now),
        Some(t) => {
            let due = t + interval;
            Some(if due <= now { now } else { due })
        }
    }
}

/// 由窗口物理像素尺寸反算网格行列（纯函数，便于单测）。
///
/// 返回 `None` 表示该尺寸**无法容纳任何完整字符格**，调用方必须保持原网格不变：
/// 不 resize vt/pty、不重配 surface、不更新渲染尺寸。判断依据是实测的格子像素
/// 尺寸（`cell_w × cell_h`）——窗口宽或高小于一格时，`w / cell_w == 0`（或
/// `h / cell_h == 0`），此时任何 `max(1)` 钳位都会把网格缩成 1×1 并把内容裁掉
/// 且不可恢复（Windows 最小化实测发 1×1、有时 0×0）。
///
/// 旧实现用 `.max(1)` 兜底，正是 P1 数据丢失的直接原因。
pub fn resize_target(
    physical_width: u32,
    physical_height: u32,
    cell_w: u32,
    cell_h: u32,
    pad_x: u32,
    pad_y: u32,
) -> Option<(u16, u16)> {
    let cw = cell_w.max(1);
    let ch = cell_h.max(1);
    let w = physical_width.saturating_sub(2 * pad_x);
    let h = physical_height.saturating_sub(2 * pad_y);
    let cols = w / cw;
    let rows = h / ch;
    if cols == 0 || rows == 0 {
        return None;
    }
    Some((
        cols.min(u16::MAX as u32) as u16,
        rows.min(u16::MAX as u32) as u16,
    ))
}

/// IME 事件对预编辑状态的更新（纯函数，便于单测）：
/// 返回 (是否改动, 新文本, 新光标范围)。Enabled 不改动（仅重新定位候选窗）。
pub fn ime_preedit_update(event: &winit::event::Ime) -> (bool, String, Option<(usize, usize)>) {
    use winit::event::Ime;
    match event {
        Ime::Enabled => (false, String::new(), None),
        Ime::Preedit(s, cursor) => (true, s.clone(), *cursor),
        Ime::Commit(_) => (true, String::new(), None),
        Ime::Disabled => (true, String::new(), None),
    }
}

/// IME 候选窗定位格：Preedit 非空时返回组合末尾格（layout_end_position），
/// 否则返回终端光标格（任务 2C.3；纯函数便于单测）。
/// IME 锚点所需的几何参数（物理像素与逻辑 padding 混用，集中在一处换算）。
#[derive(Debug, Clone, Copy)]
pub struct ImeGeometry {
    pub pad_x: u16,
    pub pad_y: u16,
    pub scale: f64,
    pub cell_w: u32,
    pub cell_h: u32,
    pub is_x11: bool,
}

/// 计算 IME 锚点矩形（物理像素，窗口内坐标）。返回 `(x, y, w, h)`。
///
/// - `pad_*` 是配置里的逻辑值，乘一次 `scale` 得到物理 padding（只乘一次）。
/// - X11 的 XIM 只接受一个 spot 点、不支持矩形：`y` 下移一行（锚在光标格底边），
///   否则候选窗会盖住正在输入的行。
/// - 宽度取两格：给 IM 面板留出避开窗口右缘的余量（alacritty 同样做法）。
pub fn ime_area_rect(g: ImeGeometry, cx: u16, cy: u16) -> (f64, f64, f64, f64) {
    let x = f64::from(g.pad_x) * g.scale + f64::from(cx) * f64::from(g.cell_w);
    let line_offset = u16::from(g.is_x11);
    let y = f64::from(g.pad_y) * g.scale
        + f64::from(cy.saturating_add(line_offset)) * f64::from(g.cell_h);
    let w = f64::from(g.cell_w) * 2.0;
    let h = f64::from(g.cell_h);
    (x, y, w, h)
}

pub fn ime_cursor_cell(
    preedit: &str,
    cursor_x: u16,
    cursor_y: u16,
    cols: u16,
    rows: u16,
) -> (u16, u16) {
    if preedit.is_empty() {
        return (cursor_y, cursor_x);
    }
    match render::layout_preedit(preedit, cursor_x, cursor_y, cols, rows) {
        Some(l) => render::layout_end_position(&l, cols),
        None => (cursor_y, cursor_x),
    }
}

/// winit 修饰键状态 → vt::Mods（KeyEncoder/encode_button 共用）。
/// 该按键组合是否属于**窗口管理器保留**、不应写进 PTY 的一类（纯函数，可单测）。
///
/// 现象（阶段 9E，用户反馈）：`Alt+Tab` 切换窗口后，终端里留下一个 Tab。原因是 WM
/// 抓键失败（或抓键前先送到客户端）时我们照样把它当"Alt+Tab"编码成 `ESC` + `\t`
/// 写进 PTY，shell 收到后就是一次补全/Tab。同理，`Super` 组合（Super+Space、
/// Super+Tab 等桌面快捷键）在当前实现里被 `vt_mods_from` 直接丢弃修饰位 → 退化成
/// **裸键**写进 PTY（按 Super+Space 会打出空格）。
///
/// 处置：`Super` 按下时不转发任何键；`Alt+Tab`（含 Shift）不转发。其余 Alt 组合
/// 保持 xterm 行为（`Alt+a` = `ESC a`），不受影响。
/// `Alt+Tab` 的"尾巴"判定：Tab 到达时 ALT 已经释放（某些 WM/时序下先发 Alt release）。
///
/// 只吞"Alt 刚松开 ≤200 ms 内的裸 Tab"，其余裸 Tab 照常转发——人不会松开 Alt 后
/// 200 ms 内什么都不按直接敲 Tab，而 WM 切换窗口的按键尾巴正好落在这个窗口里。
/// `Alt+Tab` 之后 WM 在拿到焦点时**补投的裸 Tab**（trace 实测：`focus true` 之后
/// 0–13 ms 出现 `key Pressed Named(Tab)`，mods 全空，前面没有对应的 Tab press）。
///
/// 只认"裸 Tab、且距拿到焦点 ≤100 ms"：人手不可能在窗口刚获得焦点的 100 ms 内就敲 Tab，
/// 而无修饰 Tab 本身仍照常转发（粘贴后按 Tab、切换窗口后从容按 Tab 都不受影响）。
pub fn tab_is_focus_replay(
    key: Option<vt::Key>,
    alt_down: bool,
    ctrl_down: bool,
    since_focus: Option<std::time::Duration>,
) -> bool {
    if alt_down || ctrl_down || key != Some(vt::Key::Tab) {
        return false;
    }
    since_focus.is_some_and(|d| d <= std::time::Duration::from_millis(100))
}

pub fn tab_is_alt_tab_tail(
    key: Option<vt::Key>,
    alt_currently_down: bool,
    since_alt_release: Option<std::time::Duration>,
) -> bool {
    if alt_currently_down || key != Some(vt::Key::Tab) {
        return false;
    }
    since_alt_release.is_some_and(|d| d <= std::time::Duration::from_millis(200))
}

/// 是否为 `CSI u`（Kitty 键盘协议）序列：`ESC [` … `u`。
pub fn is_csi_u(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.starts_with(b"\x1b[") && bytes.ends_with(b"u")
}

/// `Ctrl+<字符>` 的 xterm 控制字节（`Ctrl+A`→0x01 … `Ctrl+Z`→0x1a，符号按 xterm 表）。
///
/// 仅用于"传统模式（应用未启用 Kitty 键盘协议）"下修正上游编码器的 `CSI u` 输出。
pub fn ctrl_byte(ch: char) -> Option<u8> {
    let c = ch.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => (c as u8) - b'a' + 1,
        '@' | ' ' | '`' => 0x00,
        '[' | '{' => 0x1b,
        '\\' | '|' => 0x1c,
        ']' | '}' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        _ => return None,
    })
}

pub fn wm_reserved_key(m: &ModifiersState, key: Option<vt::Key>) -> bool {
    if m.super_key() {
        return true;
    }
    m.alt_key() && matches!(key, Some(vt::Key::Tab))
}

fn vt_mods_from(m: &ModifiersState) -> Mods {
    let mut mods = Mods::empty();
    if m.shift_key() {
        mods |= Mods::SHIFT;
    }
    if m.control_key() {
        mods |= Mods::CTRL;
    }
    if m.alt_key() {
        mods |= Mods::ALT;
    }
    mods
}

/// 鼠标绑定查表（同按钮后者覆盖前者——规格任务 2.5；纯函数可单测）。
fn find_binding(
    bindings: &[config::Binding],
    button: ConfigMouse,
    mods: &ConfigMods,
) -> Option<ConfigAction> {
    let mut found = None;
    for b in bindings {
        if b.mouse == Some(button) && b.mods == *mods {
            found = Some(b.action);
        }
    }
    found
}

/// 键盘绑定查表（同 key+mods 后者覆盖前者；纯函数可单测）。
fn find_key_binding(
    bindings: &[config::Binding],
    key: &str,
    mods: &ConfigMods,
) -> Option<ConfigAction> {
    let mut found = None;
    for b in bindings {
        if b.key.as_deref().is_some_and(|k| key_matches(k, key)) && b.mods == *mods {
            found = Some(b.action);
        }
    }
    found
}

/// 绑定键与按下的逻辑键是否匹配。
///
/// 单字符键按大小写无关比较：带 Shift 的组合键（如默认的 Ctrl+Shift+C）在 winit 里
/// 上报为 `Character("C")`，而配置/内置绑定写的是 `"c"`；Shift 本身已由 `mods` 表达，
/// 因此大小写不应再区分（阶段 4K：此前 Ctrl+Shift+C 不命中绑定，被当作编码键送进
/// PTY，既没复制也会把视口带回底部）。
fn key_matches(bound: &str, pressed: &str) -> bool {
    bound == pressed
        || (bound.len() == 1 && pressed.len() == 1 && bound.eq_ignore_ascii_case(pressed))
}

/// winit 修饰键状态 → config::Mods（绑定查表用；hyper 无对应键恒 false）。
fn config_mods(m: &ModifiersState) -> ConfigMods {
    ConfigMods {
        control: m.control_key(),
        shift: m.shift_key(),
        alt: m.alt_key(),
        super_: m.super_key(),
        hyper: false,
        meta: false, // winit 0.30 无 meta 键位（super 涵盖），恒 false。
    }
}

/// winit 逻辑键 → 配置 key 字符串（Named 用 Debug 名如 "Insert"；Character 用字符）。
fn key_string(key: &WinitKey) -> Option<String> {
    match key {
        WinitKey::Named(n) => Some(format!("{n:?}")),
        WinitKey::Character(s) => Some(s.to_string()),
        _ => None,
    }
}

/// 内置默认鼠标绑定（配置 mouse.bindings 为空时生效；非空以配置为准不叠加）。
fn default_mouse_bindings() -> Vec<config::Binding> {
    let none = ConfigMods {
        control: false,
        shift: false,
        alt: false,
        super_: false,
        hyper: false,
        meta: false,
    };
    vec![
        config::Binding {
            mouse: Some(ConfigMouse::Middle),
            key: None,
            mods: none,
            action: ConfigAction::Copy,
        },
        config::Binding {
            mouse: Some(ConfigMouse::Right),
            key: None,
            mods: none,
            action: ConfigAction::Paste,
        },
    ]
}

/// 内置默认键盘绑定（配置 keyboard.bindings 为空时生效：Ctrl+Shift+C/V）。
fn default_key_bindings() -> Vec<config::Binding> {
    let cm = ConfigMods {
        control: true,
        shift: true,
        alt: false,
        super_: false,
        hyper: false,
        meta: false,
    };
    vec![
        config::Binding {
            mouse: None,
            // 键盘滚动回看：只 Shift（不能复用 cm，它含 control=true）
            key: Some("PageUp".to_string()),
            mods: ConfigMods {
                control: false,
                shift: true,
                alt: false,
                super_: false,
                hyper: false,
                meta: false,
            },
            action: ConfigAction::ScrollPageUp,
        },
        config::Binding {
            mouse: None,
            key: Some("PageDown".to_string()),
            mods: ConfigMods {
                control: false,
                shift: true,
                alt: false,
                super_: false,
                hyper: false,
                meta: false,
            },
            action: ConfigAction::ScrollPageDown,
        },
        config::Binding {
            mouse: None,
            key: Some("c".to_string()),
            mods: cm,
            action: ConfigAction::Copy,
        },
        config::Binding {
            mouse: None,
            key: Some("v".to_string()),
            mods: cm,
            action: ConfigAction::Paste,
        },
    ]
}

/// 写字节到 PTY：有写线程时入队（主线程不阻塞），否则同步写并 flush。
///
/// 阶段 4H t3：前台程序不读 stdin 时同步写会阻塞主线程（实测 12.9s），
/// 因此 Unix 上写端已交给 `writer::PtyWriter`；Windows（`take_writer` 默认
/// 返回 None）保持原同步路径，行为不变。
fn write_pty(pty: &mut NativePty, writer: Option<&writer::PtyWriter>, bytes: &[u8]) {
    if let Some(w) = writer {
        w.enqueue_typed(bytes);
        return;
    }
    match pty.writer() {
        Ok(w) => {
            if let Err(e) = w.write_all(bytes).and_then(|()| w.flush()) {
                eprintln!("pty write: {}", e);
            }
        }
        Err(e) => eprintln!("pty writer: {}", e),
    }
}

/// 诊断钩子（仅 debug，默认关闭）：LEANTTY_FRAME_HASH=<路径> 时，每帧渲染后追加一行
/// `frame=<n> all=<hash> rows=<h0,h1,...>`，用于逐帧对拍"复用 vs 强制全量"，
/// 定位首个出现像素差异的帧与差异行范围（阶段 3C 任务 6）。
#[cfg(debug_assertions)]
fn dump_frame_hash_if_requested(renderer: &render::CpuRenderer) {
    let Ok(path) = std::env::var("LEANTTY_FRAME_HASH") else {
        return;
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let fb = renderer.framebuffer();
    let (w, h) = renderer.framebuffer_size();
    if w == 0 || h == 0 {
        return;
    }
    // 行哈希：把该行像素折叠成 FNV-1a 64
    let mut rows: Vec<String> = Vec::with_capacity(h as usize);
    let mut all: u64 = 0xcbf2_9ce4_8422_2325;
    for y in 0..h as usize {
        let mut hr: u64 = 0xcbf2_9ce4_8422_2325;
        for x in 0..w as usize {
            let px = fb[y * w as usize + x];
            hr = (hr ^ u64::from(px)).wrapping_mul(0x0000_0100_0000_01b3);
            all = (all ^ u64::from(px)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        rows.push(format!("{hr:016x}"));
    }
    let line = format!(
        "frame={n} all={all:016x} rows={}
",
        rows.join(",")
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 测试钩子（默认关闭）：LEANTTY_DUMP_SNAPSHOT 设置时，每帧把 FrameSnapshot 文本
/// （行文本 + 宽格统计）写入该路径，供 e2e 断言而不依赖 OCR。
/// 测试钩子（默认关闭，仅 debug 构建；任务 4 门控）：LEANTTY_DUMP_SNAPSHOT 设置时，
/// 每帧把 FrameSnapshot 文本（行文本 + 宽格统计 + 每格字体索引）写入该路径，
/// 供 e2e 断言而不依赖 OCR。release 构建中本函数与钩子字符串均不存在。
#[cfg(debug_assertions)]
fn dump_snapshot_if_requested(snap: &vt::FrameSnapshot, renderer: Option<&render::CpuRenderer>) {
    let Ok(path) = std::env::var("LEANTTY_DUMP_SNAPSHOT") else {
        return;
    };
    let mut text = String::new();
    let mut wide = 0usize;
    let mut spacer = 0usize;
    for row in &snap.cells {
        let mut line = String::new();
        for cell in row {
            line.push_str(&cell.chars);
            match cell.wide {
                vt::CellWide::Wide => wide += 1,
                vt::CellWide::SpacerTail => spacer += 1,
                _ => {}
            }
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    // 每格实际使用的组内字体索引（回退链 e2e 断言用；任务 4 将本钩子门控为仅 debug）。
    if let Some(r) = renderer {
        for line in r.font_indices(snap) {
            text.push_str("# font: ");
            text.push_str(
                &line
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            text.push('\n');
        }
    }
    let full = format!(
        "# cols={} rows={} wide={} spacer={} alternate={}\n{}",
        snap.cols, snap.rows, wide, spacer, snap.alternate_screen, text
    );
    if let Err(e) = std::fs::write(&path, full) {
        eprintln!("dump snapshot: {}", e);
    }
}

const LETTERS: [vt::Key; 26] = [
    vt::Key::A,
    vt::Key::B,
    vt::Key::C,
    vt::Key::D,
    vt::Key::E,
    vt::Key::F,
    vt::Key::G,
    vt::Key::H,
    vt::Key::I,
    vt::Key::J,
    vt::Key::K,
    vt::Key::L,
    vt::Key::M,
    vt::Key::N,
    vt::Key::O,
    vt::Key::P,
    vt::Key::Q,
    vt::Key::R,
    vt::Key::S,
    vt::Key::T,
    vt::Key::U,
    vt::Key::V,
    vt::Key::W,
    vt::Key::X,
    vt::Key::Y,
    vt::Key::Z,
];

const DIGITS: [vt::Key; 10] = [
    vt::Key::Digit0,
    vt::Key::Digit1,
    vt::Key::Digit2,
    vt::Key::Digit3,
    vt::Key::Digit4,
    vt::Key::Digit5,
    vt::Key::Digit6,
    vt::Key::Digit7,
    vt::Key::Digit8,
    vt::Key::Digit9,
];

/// winit 逻辑键 → vt::Key（阶段1 子集：命名键 + ASCII 字母数字）。
/// 取"未按 Shift 的码点"（阶段 6A 修复，纯函数）。
///
/// Kitty 键盘协议要求非功能键带上这个码点；缺省为 0 时上游编码器找不到 kitty 表项，
/// 会**静默不输出**（见 `vendor/ghostty-src/src/input/key_encode.zig` 的 `kitty()`）。
/// winit 的 `logical_key` 在 Shift 按下时给的是大写字母，因此这里统一折成小写，
/// 与 kitty 规范里"unshifted code point"的语义一致。
pub fn unshifted_codepoint(event: &winit::event::KeyEvent) -> Option<char> {
    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement as _;
    // 优先用 winit 提供的"去掉所有修饰键后的键"（X11 与 Windows 后端都实现了，
    // 见 winit-0.30.13 `src/platform/modifier_supplement.rs:6/26`）：
    // 它按当前键盘布局给出未加修饰的字符，符号键也能拿到。
    let base = event.key_without_modifiers();
    if let WinitKey::Character(s) = &base
        && s.as_ref().chars().count() == 1
        && let Some(c) = s.as_ref().chars().next()
    {
        return Some(c);
    }
    // 回退：用逻辑键的单字符结果（多字符、命名键不适用），ASCII 大写折小写。
    match &event.logical_key {
        WinitKey::Character(s) if s.as_ref().chars().count() == 1 => {
            let c = s.as_ref().chars().next()?;
            Some(if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                c
            })
        }
        _ => None,
    }
}

fn map_logical_key(key: &WinitKey) -> Option<vt::Key> {
    match key {
        WinitKey::Named(NamedKey::Enter) => Some(vt::Key::Enter),
        // 阶段 9F：`Named(Space)` 此前没有映射 → `key` 为 None，编码分支直接跳过，
        // 导致 `Ctrl+Space` 在无输入法环境里一个字节都不发（应为 0x00）。
        WinitKey::Named(NamedKey::Space) => Some(vt::Key::Space),
        WinitKey::Named(NamedKey::Tab) => Some(vt::Key::Tab),
        WinitKey::Named(NamedKey::Backspace) => Some(vt::Key::Backspace),
        WinitKey::Named(NamedKey::Escape) => Some(vt::Key::Escape),
        WinitKey::Named(NamedKey::ArrowUp) => Some(vt::Key::ArrowUp),
        WinitKey::Named(NamedKey::ArrowDown) => Some(vt::Key::ArrowDown),
        WinitKey::Named(NamedKey::ArrowLeft) => Some(vt::Key::ArrowLeft),
        WinitKey::Named(NamedKey::ArrowRight) => Some(vt::Key::ArrowRight),
        WinitKey::Named(NamedKey::Home) => Some(vt::Key::Home),
        WinitKey::Named(NamedKey::End) => Some(vt::Key::End),
        // 阶段 6B t2b：功能键此前未映射，导致 F1–F12 / Insert / PageUp / PageDown 静默丢失。
        WinitKey::Named(NamedKey::Insert) => Some(vt::Key::Insert),
        // 阶段 6B 补充（用户裁定方案 a）：无修饰 PageUp/PageDown 转发给应用，
        // 键盘滚动回看由默认的 Shift+PageUp/PageDown 绑定承担（见 default_key_bindings）。
        WinitKey::Named(NamedKey::PageUp) => Some(vt::Key::PageUp),
        WinitKey::Named(NamedKey::PageDown) => Some(vt::Key::PageDown),
        WinitKey::Named(NamedKey::Delete) => Some(vt::Key::Delete),
        WinitKey::Named(NamedKey::F1) => Some(vt::Key::F1),
        WinitKey::Named(NamedKey::F2) => Some(vt::Key::F2),
        WinitKey::Named(NamedKey::F3) => Some(vt::Key::F3),
        WinitKey::Named(NamedKey::F4) => Some(vt::Key::F4),
        WinitKey::Named(NamedKey::F5) => Some(vt::Key::F5),
        WinitKey::Named(NamedKey::F6) => Some(vt::Key::F6),
        WinitKey::Named(NamedKey::F7) => Some(vt::Key::F7),
        WinitKey::Named(NamedKey::F8) => Some(vt::Key::F8),
        WinitKey::Named(NamedKey::F9) => Some(vt::Key::F9),
        WinitKey::Named(NamedKey::F10) => Some(vt::Key::F10),
        WinitKey::Named(NamedKey::F11) => Some(vt::Key::F11),
        WinitKey::Named(NamedKey::F12) => Some(vt::Key::F12),
        WinitKey::Character(s) => {
            let s: &str = s.as_ref();
            if let Some(c) = s.chars().next() {
                if c.is_ascii_uppercase() {
                    return Some(LETTERS[(c as u8 - b'A') as usize]);
                }
                if c.is_ascii_lowercase() {
                    return Some(LETTERS[(c.to_ascii_uppercase() as u8 - b'A') as usize]);
                }
                if c.is_ascii_digit() {
                    return Some(DIGITS[(c as u8 - b'0') as usize]);
                }
                // 阶段 6B t2b：符号键此前未映射，Ctrl+符号（ctrl+minus、ctrl+semicolon 等）
                // 在 Encoded 路径下 key 为 None → 一个字节都不写。这里映射到 vt::Key
                // 已有的对应项，具体字节仍由上游编码器决定。
                let punct = match c {
                    '-' => Some(vt::Key::Minus),
                    '=' => Some(vt::Key::Equal),
                    ',' => Some(vt::Key::Comma),
                    '.' => Some(vt::Key::Period),
                    ';' => Some(vt::Key::Semicolon),
                    '\'' => Some(vt::Key::Quote),
                    '`' => Some(vt::Key::Backquote),
                    '[' | '{' => Some(vt::Key::BracketLeft),
                    ']' | '}' => Some(vt::Key::BracketRight),
                    '\\' | '|' => Some(vt::Key::Backslash),
                    '/' => Some(vt::Key::Slash),
                    ' ' => Some(vt::Key::Space),
                    _ => None,
                };
                if punct.is_some() {
                    return punct;
                }
            }
            None
        }
        _ => None,
    }
}

/// config::Config → render::FontSettings（family/style 字符串原样传递）。
fn render_font_settings(config: &config::Config) -> render::FontSettings {
    let spec = |f: &config::FontSpec| render::FontSpec {
        family: f.family.clone(),
        style: f.style.clone(),
    };
    render::FontSettings {
        size: config.font.size,
        normal: spec(&config.font.normal),
        bold: spec(&config.font.bold),
        italic: spec(&config.font.italic),
        bold_italic: spec(&config.font.bold_italic),
        fallback: fallback_chain(config),
    }
}

/// 内置符号/emoji 兜底字体（阶段 9I）。
///
/// 用户配置的 `font.fallback` 之后依次尝试这些系统字体；不存在时由加载器跳过并打印一行提示。
/// 动机（leantty-zig 的对照清单 T2）：emoji 与符号码点此前只能命中用户配置的回退链，
/// 本机只配了 CJK 字体 → `😀 🚀 🦄` 全是豆腐块。alacritty 通过 fontconfig 自动找字，
/// 这里用一份固定清单达到同样效果，避免每次缺字都去扫系统字体。
const DEFAULT_SYMBOL_FALLBACKS: &[&str] = &["Symbola", "DejaVu Sans", "Noto Sans Symbols2"];

fn fallback_chain(config: &config::Config) -> Vec<render::FontSpec> {
    let mut chain: Vec<render::FontSpec> = config
        .font
        .fallback
        .iter()
        .map(|f| render::FontSpec {
            family: f.family.clone(),
            style: f.style.clone(),
        })
        .collect();
    for family in DEFAULT_SYMBOL_FALLBACKS {
        if chain.iter().any(|f| f.family.eq_ignore_ascii_case(family)) {
            continue;
        }
        chain.push(render::FontSpec {
            family: (*family).to_string(),
            style: "Regular".to_string(),
        });
    }
    chain
}

/// config font 渲染选项 → render::RenderOptions。
fn render_options(config: &config::Config) -> render::RenderOptions {
    render::RenderOptions {
        antialiasing: match config.font.antialiasing {
            config::Antialiasing::Grayscale => render::Antialiasing::Grayscale,
            config::Antialiasing::None => render::Antialiasing::None,
        },
        subpixel: match config.font.subpixel {
            config::Subpixel::None => render::Subpixel::None,
            config::Subpixel::Rgb => render::Subpixel::Rgb,
            config::Subpixel::Bgr => render::Subpixel::Bgr,
        },
        hinting: config.font.hinting,
        text_gamma: config.font.text_gamma,
        draw_bold_with_bright: config.colors.draw_bold_text_with_bright_colors,
    }
}

/// config colors → render::Palette（config::Rgb 与 vt::Rgb 字段同名转换）。
/// 配置 `[colors]` → VT 层调色板（阶段 4F t2：让 ANSI 索引色/默认反色解析出配置色）。
fn vt_palette(colors: &config::Colors) -> vt::VtPalette {
    let c = |c: config::Rgb| vt::Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    };
    vt::VtPalette {
        foreground: c(colors.primary.foreground),
        background: c(colors.primary.background),
        cursor: c(colors.cursor.cursor),
        normal: [
            c(colors.normal.black),
            c(colors.normal.red),
            c(colors.normal.green),
            c(colors.normal.yellow),
            c(colors.normal.blue),
            c(colors.normal.magenta),
            c(colors.normal.cyan),
            c(colors.normal.white),
        ],
        bright: [
            c(colors.bright.black),
            c(colors.bright.red),
            c(colors.bright.green),
            c(colors.bright.yellow),
            c(colors.bright.blue),
            c(colors.bright.magenta),
            c(colors.bright.cyan),
            c(colors.bright.white),
        ],
    }
}

fn render_palette(colors: &config::Colors) -> render::Palette {
    let c = |c: config::Rgb| vt::Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    };
    render::Palette {
        primary_bg: c(colors.primary.background),
        primary_fg: c(colors.primary.foreground),
        cursor_text: c(colors.cursor.text),
        cursor: c(colors.cursor.cursor),
        normal: [
            c(colors.normal.black),
            c(colors.normal.red),
            c(colors.normal.green),
            c(colors.normal.yellow),
            c(colors.normal.blue),
            c(colors.normal.magenta),
            c(colors.normal.cyan),
            c(colors.normal.white),
        ],
        bright: [
            c(colors.bright.black),
            c(colors.bright.red),
            c(colors.bright.green),
            c(colors.bright.yellow),
            c(colors.bright.blue),
            c(colors.bright.magenta),
            c(colors.bright.cyan),
            c(colors.bright.white),
        ],
    }
}

/// 绑定查表/默认绑定单测（任务 2.5：纯函数可测）。
#[cfg(test)]
mod binding_tests {
    use super::*;

    fn mods(control: bool, shift: bool) -> ConfigMods {
        ConfigMods {
            control,
            shift,
            alt: false,
            super_: false,
            hyper: false,
            meta: false,
        }
    }

    #[test]
    fn find_binding_later_overrides_and_mods_matter() {
        let b = |action: ConfigAction| config::Binding {
            mouse: Some(ConfigMouse::Middle),
            key: None,
            mods: mods(false, false),
            action,
        };
        let bindings = vec![b(ConfigAction::Copy), b(ConfigAction::Paste)];
        assert_eq!(
            find_binding(&bindings, ConfigMouse::Middle, &mods(false, false)),
            Some(ConfigAction::Paste),
            "同按钮后者覆盖前者"
        );
        assert_eq!(
            find_binding(&bindings, ConfigMouse::Right, &mods(false, false)),
            None,
            "按钮不匹配应 None"
        );
    }

    #[test]
    fn key_matches_is_case_insensitive_for_single_chars() {
        // Ctrl+Shift+C 在 winit 里上报 Character("C")，绑定写 "c"：必须命中。
        assert!(key_matches("c", "C"));
        assert!(key_matches("C", "c"));
        assert!(key_matches("c", "c"));
        // 多字符键（Named 键名）保持精确匹配，不做大小写折叠。
        assert!(key_matches("Insert", "Insert"));
        assert!(!key_matches("Insert", "insert"));
        assert!(!key_matches("c", "v"));
    }

    #[test]
    fn find_key_binding_matches_key_and_mods() {
        let b = |key: &str, control: bool, action: ConfigAction| config::Binding {
            mouse: None,
            key: Some(key.to_string()),
            mods: mods(control, false),
            action,
        };
        let bindings = vec![
            b("c", true, ConfigAction::Copy),
            b("v", true, ConfigAction::Paste),
        ];
        assert_eq!(
            find_key_binding(&bindings, "c", &mods(true, false)),
            Some(ConfigAction::Copy)
        );
        assert_eq!(
            find_key_binding(&bindings, "c", &mods(false, false)),
            None,
            "mods 不匹配应 None"
        );
        assert_eq!(
            find_key_binding(&bindings, "x", &mods(true, false)),
            None,
            "key 不匹配应 None"
        );
    }

    #[test]
    fn key_string_named_and_character() {
        assert_eq!(
            key_string(&WinitKey::Named(NamedKey::Insert)),
            Some("Insert".to_string())
        );
        assert_eq!(
            key_string(&WinitKey::Character("c".into())),
            Some("c".to_string())
        );
    }

    #[test]
    fn default_bindings_shape() {
        let mb = default_mouse_bindings();
        assert_eq!(mb.len(), 2);
        assert_eq!(
            find_binding(&mb, ConfigMouse::Middle, &mods(false, false)),
            Some(ConfigAction::Copy)
        );
        assert_eq!(
            find_binding(&mb, ConfigMouse::Right, &mods(false, false)),
            Some(ConfigAction::Paste)
        );
        let kb = default_key_bindings();
        let ctrl_shift = ConfigMods {
            control: true,
            shift: true,
            alt: false,
            super_: false,
            hyper: false,
            meta: false,
        };
        assert_eq!(
            find_key_binding(&kb, "c", &ctrl_shift),
            Some(ConfigAction::Copy)
        );
        assert_eq!(
            find_key_binding(&kb, "v", &ctrl_shift),
            Some(ConfigAction::Paste)
        );
    }
}

/// IME 候选窗定位单测（任务 2C.3）。
#[cfg(test)]
mod ime_cursor_tests {
    use super::*;

    #[test]
    fn no_preedit_follows_terminal_cursor() {
        assert_eq!(
            ime_cursor_cell("", 5, 3, 80, 24),
            (3, 5),
            "无 Preedit 跟随光标（row,col）"
        );
    }

    #[test]
    fn preedit_end_position() {
        // ASCII：起点 (5,3) + 3 宽 → (8,3)
        assert_eq!(ime_cursor_cell("abc", 5, 3, 80, 24), (3, 8));
        // 宽字符：2 个汉字占 4 格
        assert_eq!(ime_cursor_cell("中文", 5, 3, 80, 24), (3, 9));
    }

    #[test]
    fn preedit_wraps_at_right_edge() {
        // 光标在倒数第 2 列，3 个字符超宽 → 换到下一行行首，末尾 (3,1)
        assert_eq!(ime_cursor_cell("abc", 78, 0, 80, 24), (1, 3));
    }

    #[test]
    fn preedit_cleared_keeps_cursor() {
        assert_eq!(
            ime_cursor_cell("", 0, 23, 80, 24),
            (23, 0),
            "清空后回到光标格"
        );
    }
}

/// IME 事件状态更新单测（任务 2C.3：Preedit/Commit/Disabled 清空语义）。
#[cfg(test)]
mod alt_tab_tail_tests {
    use super::tab_is_alt_tab_tail;
    use std::time::Duration;
    use vt::Key;

    #[test]
    fn tab_shortly_after_alt_release_is_swallowed() {
        assert!(tab_is_alt_tab_tail(
            Some(Key::Tab),
            false,
            Some(Duration::from_millis(40))
        ));
        assert!(tab_is_alt_tab_tail(
            Some(Key::Tab),
            false,
            Some(Duration::from_millis(200))
        ));
    }

    #[test]
    fn plain_tab_later_is_forwarded() {
        assert!(!tab_is_alt_tab_tail(
            Some(Key::Tab),
            false,
            Some(Duration::from_millis(400))
        ));
        assert!(
            !tab_is_alt_tab_tail(Some(Key::Tab), false, None),
            "从未按过 Alt"
        );
    }

    #[test]
    fn alt_still_held_is_handled_elsewhere() {
        // Alt 按住时由 wm_reserved_key 负责，这里返回 false 以免重复判定。
        assert!(!tab_is_alt_tab_tail(
            Some(Key::Tab),
            true,
            Some(Duration::from_millis(10))
        ));
    }

    #[test]
    fn non_tab_keys_are_untouched() {
        assert!(!tab_is_alt_tab_tail(
            Some(Key::A),
            false,
            Some(Duration::from_millis(10))
        ));
        assert!(!tab_is_alt_tab_tail(
            None,
            false,
            Some(Duration::from_millis(10))
        ));
    }
}

#[cfg(test)]
mod focus_replay_tests {
    use super::tab_is_focus_replay;
    use std::time::Duration;
    use vt::Key;

    #[test]
    fn bare_tab_right_after_focus_is_swallowed() {
        // 实测三种间隔：+0 ms、+1 ms、+7 ms。
        for ms in [0u64, 1, 7, 13, 100] {
            assert!(
                tab_is_focus_replay(
                    Some(Key::Tab),
                    false,
                    false,
                    Some(Duration::from_millis(ms))
                ),
                "焦点后 {ms} ms 的裸 Tab 应被拦"
            );
        }
    }

    #[test]
    fn bare_tab_after_the_window_is_forwarded() {
        assert!(!tab_is_focus_replay(
            Some(Key::Tab),
            false,
            false,
            Some(Duration::from_millis(150))
        ));
        assert!(!tab_is_focus_replay(Some(Key::Tab), false, false, None));
    }

    #[test]
    fn modified_tab_is_not_affected() {
        assert!(!tab_is_focus_replay(
            Some(Key::Tab),
            true,
            false,
            Some(Duration::from_millis(5))
        ));
        assert!(!tab_is_focus_replay(
            Some(Key::Tab),
            false,
            true,
            Some(Duration::from_millis(5))
        ));
    }

    #[test]
    fn other_keys_right_after_focus_are_forwarded() {
        assert!(!tab_is_focus_replay(
            Some(Key::A),
            false,
            false,
            Some(Duration::from_millis(5))
        ));
        assert!(!tab_is_focus_replay(
            None,
            false,
            false,
            Some(Duration::from_millis(5))
        ));
    }
}

#[cfg(test)]
mod ctrl_byte_tests {
    use super::{ctrl_byte, is_csi_u};

    #[test]
    fn letters_and_symbols_follow_xterm() {
        assert_eq!(ctrl_byte('a'), Some(0x01));
        assert_eq!(ctrl_byte('A'), Some(0x01), "按 unshifted 字符换算，大小写等价");
        assert_eq!(ctrl_byte('z'), Some(0x1a));
        assert_eq!(ctrl_byte(' '), Some(0x00));
        assert_eq!(ctrl_byte('['), Some(0x1b));
        assert_eq!(ctrl_byte('\\'), Some(0x1c));
        assert_eq!(ctrl_byte(']'), Some(0x1d));
        assert_eq!(ctrl_byte('?'), Some(0x7f));
        assert_eq!(ctrl_byte('1'), None, "数字没有对应的控制字节");
    }

    #[test]
    fn csi_u_detection() {
        assert!(is_csi_u(b"\x1b[97;6u"));
        assert!(is_csi_u(b"\x1b[123;5u"));
        assert!(!is_csi_u(b"\x1b[A"));
        assert!(!is_csi_u(b"\x01"));
        assert!(!is_csi_u(b""));
    }
}

#[cfg(test)]
mod wm_reserved_tests {
    use super::wm_reserved_key;
    use vt::Key;
    use winit::keyboard::ModifiersState;

    fn mods(shift: bool, ctrl: bool, alt: bool, sup: bool) -> ModifiersState {
        let mut m = ModifiersState::empty();
        m.set(ModifiersState::SHIFT, shift);
        m.set(ModifiersState::CONTROL, ctrl);
        m.set(ModifiersState::ALT, alt);
        m.set(ModifiersState::SUPER, sup);
        m
    }

    #[test]
    fn alt_tab_is_not_forwarded() {
        assert!(wm_reserved_key(
            &mods(true, false, true, false),
            Some(Key::Tab)
        ));
        assert!(wm_reserved_key(
            &mods(false, false, true, false),
            Some(Key::Tab)
        ));
    }

    #[test]
    fn plain_tab_is_forwarded() {
        assert!(!wm_reserved_key(
            &mods(false, false, false, false),
            Some(Key::Tab)
        ));
    }

    #[test]
    fn alt_letter_still_reaches_the_shell() {
        // Alt+a 走 xterm 规则（ESC a），不属于 WM 保留组合。
        assert!(!wm_reserved_key(&mods(false, false, true, false), None));
        assert!(!wm_reserved_key(
            &mods(false, false, true, false),
            Some(Key::A)
        ));
    }

    #[test]
    fn super_combinations_are_reserved() {
        // Super 组合是桌面快捷键：任何键都不该写进 PTY（此前修饰位被丢弃 →
        // Super+Space 会打出空格）。
        assert!(wm_reserved_key(&mods(false, false, false, true), None));
        assert!(wm_reserved_key(
            &mods(false, false, false, true),
            Some(Key::Space)
        ));
        assert!(wm_reserved_key(
            &mods(true, false, false, true),
            Some(Key::Tab)
        ));
    }
}

#[cfg(test)]
mod ime_area_tests {
    use super::{ImeGeometry, ime_area_rect};

    fn geom(is_x11: bool, scale: f64) -> ImeGeometry {
        ImeGeometry {
            pad_x: 8,
            pad_y: 4,
            scale,
            cell_w: 10,
            cell_h: 20,
            is_x11,
        }
    }

    #[test]
    fn x11_anchor_is_below_the_cursor_row() {
        let (x, y, w, h) = ime_area_rect(geom(true, 1.0), 5, 3);
        assert_eq!(x, 58.0, "x = pad_x*scale + col*cell_w");
        assert_eq!(y, 84.0, "X11 把锚点下移一行：pad_y + (row+1)*cell_h");
        assert_eq!((w, h), (20.0, 20.0), "宽两格、高一格");
    }

    #[test]
    fn non_x11_anchor_is_the_cursor_row_top() {
        let (_x, y, w, h) = ime_area_rect(geom(false, 1.0), 5, 3);
        assert_eq!(y, 64.0, "非 X11（Wayland text-input）给的是光标格本身");
        assert_eq!((w, h), (20.0, 20.0));
    }

    #[test]
    fn padding_is_scaled_once() {
        // 逻辑 padding 8/4 在 2 倍缩放下是物理 16/8，cell 已是物理像素（不再乘）。
        let (x, y, _, _) = ime_area_rect(geom(true, 2.0), 2, 1);
        assert_eq!(x, 36.0, "16 + 2*10");
        assert_eq!(y, 48.0, "8 + (1+1)*20");
    }

    #[test]
    fn bottom_row_does_not_overflow_the_cell_index() {
        let (_x, y, _, _) = ime_area_rect(geom(true, 1.0), 0, u16::MAX);
        assert!(y.is_finite() && y > 0.0, "饱和加法不应回绕");
    }
}

#[cfg(test)]
mod ime_event_tests {
    use super::*;
    use winit::event::Ime;

    #[test]
    fn preedit_sets_text_and_cursor() {
        let (changed, text, cursor) = ime_preedit_update(&Ime::Preedit("中".into(), Some((0, 3))));
        assert!(changed);
        assert_eq!(text, "中");
        assert_eq!(cursor, Some((0, 3)));
    }

    #[test]
    fn commit_clears_preedit() {
        let (changed, text, cursor) = ime_preedit_update(&Ime::Commit("确定".into()));
        assert!(changed, "Commit 后应清空组合态");
        assert!(text.is_empty());
        assert_eq!(cursor, None);
    }

    #[test]
    fn disabled_clears_preedit() {
        let (changed, text, cursor) = ime_preedit_update(&Ime::Disabled);
        assert!(changed, "Disabled 应清空组合态");
        assert!(text.is_empty());
        assert_eq!(cursor, None);
    }

    #[test]
    fn enabled_keeps_state() {
        let (changed, _, _) = ime_preedit_update(&Ime::Enabled);
        assert!(!changed, "Enabled 不改动组合态（仅重定位候选窗）");
    }
}

#[cfg(test)]
mod console_tests {
    use super::should_free_console;

    #[test]
    fn free_console_only_when_sole_user() {
        // 只有自己是控制台唯一使用者时才释放；≥2 说明还有别的进程共享控制台。
        assert!(should_free_console(1), "1 个进程（只有自己）应释放");
        assert!(!should_free_console(0), "0 表示查询失败，不释放");
        assert!(
            !should_free_console(2),
            "2 个及以上不释放（会关掉对方的窗口）"
        );
        assert!(!should_free_console(10));
    }
}

#[cfg(test)]
mod icon_tests {
    /// 阶段 5A t3：窗口图标的内嵌字节必须是 128x128 的 RGBA8（winit `Icon::from_rgba` 的要求）。
    /// 由 `scripts/gen-icons.sh` 生成，长度不对说明资源与生成脚本脱节。
    #[test]
    fn window_icon_rgba_is_128x128() {
        let bytes = include_bytes!("../assets/window-icon-128.rgba");
        assert_eq!(
            bytes.len(),
            128 * 128 * 4,
            "window-icon-128.rgba 必须是 128x128 RGBA8（65536 字节）"
        );
        // 非全零：至少存在一个不透明像素（否则图标是全透明占位图）
        assert!(
            bytes.as_chunks::<4>().0.iter().any(|px| px[3] > 0),
            "图标不应为全透明"
        );
    }
}

#[cfg(test)]
mod wake_tests {
    use super::next_wake;
    use std::time::{Duration, Instant};

    const INTERVAL: Duration = Duration::from_secs(1);

    #[test]
    fn only_liveness_returns_that_instant() {
        let now = Instant::now();
        let live = now + INTERVAL;
        assert_eq!(
            next_wake(now, Some(live), None, None),
            Some(live),
            "只有存活检查时，下一次唤醒就是它的到期时刻"
        );
    }

    #[test]
    fn liveness_and_render_takes_earliest() {
        let now = Instant::now();
        let live = now + INTERVAL;
        let render = now + Duration::from_millis(8);
        assert_eq!(
            next_wake(now, Some(live), Some(render), None),
            Some(render),
            "渲染到期更早时按渲染唤醒（存活检查顺延到下一次迭代）"
        );
        let render_late = now + Duration::from_millis(3000);
        assert_eq!(
            next_wake(now, Some(live), Some(render_late), None),
            Some(live),
            "渲染更晚时不能被它拖住存活检查"
        );
    }

    #[test]
    fn liveness_and_paste_takes_earliest() {
        let now = Instant::now();
        let live = now + INTERVAL;
        let paste = now + Duration::from_millis(2);
        assert_eq!(
            next_wake(now, Some(live), None, Some(paste)),
            Some(paste),
            "粘贴排空更早时按它唤醒"
        );
    }

    #[test]
    fn none_when_no_pending_work() {
        let now = Instant::now();
        assert_eq!(next_wake(now, None, None, None), None, "无待办时应无限等待");
    }

    #[test]
    fn elapsed_deadlines_collapse_to_now() {
        let now = Instant::now();
        let past = now - Duration::from_millis(500);
        assert_eq!(
            next_wake(now, Some(past), None, None),
            Some(now),
            "已过期的到期时刻应归一化为 now（立即唤醒一次，不忙等重试）"
        );
    }
}

#[cfg(test)]
mod liveness_tests {
    use super::next_liveness_wake;
    use std::time::{Duration, Instant};

    const INTERVAL: Duration = Duration::from_secs(1);

    #[test]
    fn schedules_immediately_when_never_checked() {
        let now = Instant::now();
        assert_eq!(
            next_liveness_wake(now, true, None, INTERVAL),
            Some(now),
            "从未检查过时应立即唤醒一次"
        );
    }

    #[test]
    fn returns_none_when_child_exited() {
        let now = Instant::now();
        assert_eq!(
            next_liveness_wake(now, false, Some(now), INTERVAL),
            None,
            "子进程已退出时无需再安排（退出路径已接管）"
        );
        assert_eq!(next_liveness_wake(now, false, None, INTERVAL), None);
    }

    #[test]
    fn waits_for_interval_when_just_checked() {
        let now = Instant::now();
        let wake = next_liveness_wake(now, true, Some(now), INTERVAL).expect("应安排一次");
        assert_eq!(wake, now + INTERVAL, "刚检查过应等满一个间隔");
    }

    #[test]
    fn wakes_now_when_interval_already_elapsed() {
        let now = Instant::now();
        let last = now - Duration::from_millis(2500);
        assert_eq!(
            next_liveness_wake(now, true, Some(last), INTERVAL),
            Some(now),
            "距上次检查已超过间隔时应立即唤醒"
        );
    }

    #[test]
    fn never_more_than_one_wake_per_interval() {
        // 连续两轮：第一轮立即唤醒，第二轮必须等满间隔（即 ≤1 次/秒）
        let t0 = Instant::now();
        let w1 = next_liveness_wake(t0, true, None, INTERVAL).unwrap();
        assert_eq!(w1, t0);
        let w2 = next_liveness_wake(w1, true, Some(w1), INTERVAL).unwrap();
        assert_eq!(w2, w1 + INTERVAL);
    }
}

/// 阶段 6C C4：Windows 键映射单测（`map_logical_key` + 默认 Shift+PageUp/PageDown 绑定）。
/// 只测纯函数与默认表，不依赖窗口/输入注入；`cfg(windows)` 门控，Linux 侧构建不受影响。
#[cfg(all(test, windows))]
mod keymap_tests {
    use super::*;
    use winit::keyboard::{Key as WinitKey, NamedKey};

    #[test]
    fn map_logical_key_named_keys() {
        let cases = [
            (NamedKey::Enter, vt::Key::Enter),
            (NamedKey::Tab, vt::Key::Tab),
            (NamedKey::Backspace, vt::Key::Backspace),
            (NamedKey::Escape, vt::Key::Escape),
            (NamedKey::ArrowUp, vt::Key::ArrowUp),
            (NamedKey::ArrowDown, vt::Key::ArrowDown),
            (NamedKey::ArrowLeft, vt::Key::ArrowLeft),
            (NamedKey::ArrowRight, vt::Key::ArrowRight),
            (NamedKey::Home, vt::Key::Home),
            (NamedKey::End, vt::Key::End),
            (NamedKey::Insert, vt::Key::Insert),
            (NamedKey::Delete, vt::Key::Delete),
            (NamedKey::PageUp, vt::Key::PageUp),
            (NamedKey::PageDown, vt::Key::PageDown),
            (NamedKey::F1, vt::Key::F1),
            (NamedKey::F2, vt::Key::F2),
            (NamedKey::F3, vt::Key::F3),
            (NamedKey::F4, vt::Key::F4),
            (NamedKey::F5, vt::Key::F5),
            (NamedKey::F6, vt::Key::F6),
            (NamedKey::F7, vt::Key::F7),
            (NamedKey::F8, vt::Key::F8),
            (NamedKey::F9, vt::Key::F9),
            (NamedKey::F10, vt::Key::F10),
            (NamedKey::F11, vt::Key::F11),
            (NamedKey::F12, vt::Key::F12),
        ];
        for (named, want) in cases {
            assert_eq!(
                map_logical_key(&WinitKey::Named(named)),
                Some(want),
                "named {:?} 未按预期映射",
                named
            );
        }
    }

    #[test]
    fn map_logical_key_punctuation_and_ascii() {
        let punct = [
            ('-', vt::Key::Minus),
            ('=', vt::Key::Equal),
            (',', vt::Key::Comma),
            ('.', vt::Key::Period),
            (';', vt::Key::Semicolon),
            ('\'', vt::Key::Quote),
            ('`', vt::Key::Backquote),
            ('\\', vt::Key::Backslash),
            ('[', vt::Key::BracketLeft),
            (']', vt::Key::BracketRight),
            ('/', vt::Key::Slash),
            (' ', vt::Key::Space),
        ];
        for (c, want) in punct {
            let s: String = c.to_string();
            assert_eq!(
                map_logical_key(&WinitKey::Character(s.into())),
                Some(want),
                "符号 {c:?} 未按预期映射"
            );
        }
        // 字母/数字大小写归一
        assert_eq!(
            map_logical_key(&WinitKey::Character("a".into())),
            Some(vt::Key::A)
        );
        assert_eq!(
            map_logical_key(&WinitKey::Character("A".into())),
            Some(vt::Key::A)
        );
        assert_eq!(
            map_logical_key(&WinitKey::Character("7".into())),
            Some(vt::Key::Digit7)
        );
    }

    #[test]
    fn default_bindings_shift_pageup_pagedown_are_scroll() {
        let bindings = default_key_bindings();
        let has = |key: &str, action: ConfigAction| {
            bindings.iter().any(|b| {
                b.mouse.is_none()
                    && b.key.as_deref() == Some(key)
                    && b.mods.shift
                    && !b.mods.control
                    && !b.mods.alt
                    && b.action == action
            })
        };
        assert!(
            has("PageUp", ConfigAction::ScrollPageUp),
            "缺少 Shift+PageUp -> ScrollPageUp 默认绑定"
        );
        assert!(
            has("PageDown", ConfigAction::ScrollPageDown),
            "缺少 Shift+PageDown -> ScrollPageDown 默认绑定"
        );
    }
}
