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
/// 设置环境变量 `APP_PERF_FRAMES`（已有变量，e2e 与帧日志共用）即打开。
/// 面向用户的警告/错误（配置告警、hinting 不支持、backend 回落等）不受此开关影响。
fn perf_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("APP_PERF_FRAMES").is_some_and(|v| !v.is_empty()))
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

use pty::{NativePty, Pty};
use render::{CpuRenderer, Renderer};
use vt::{Action, Cell, Mods, VtTerminal};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};
use winit::event::{MouseButton, MouseScrollDelta};
use std::collections::VecDeque;
use config::{Action as ConfigAction, Mods as ConfigMods, Mouse as ConfigMouse};
use vt::ScrollViewport;

/// 字体渲染配置与网格尺寸由字体度量算出（见 render::cell_metrics），无固定常量。
/// SHELL 环境变量未设置（或非 UTF-8）时的平台默认 shell。
///
/// Windows 回落 cmd.exe，其他平台回落 /bin/sh。
pub fn default_shell() -> &'static str {
    if cfg!(windows) {
        "cmd.exe"
    } else {
        "/bin/sh"
    }
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
    if let Some(t) = text
        && !t.is_empty()
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
        self.proxy.send_event(()).map_err(|_| Error::EventLoopClosed)
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

mod clipboard;
mod follow;
mod paste;
mod framepace;
mod ime;
mod scrollbar;
mod writer;
mod selection;

pub use clipboard::{ArboardClipboard, Clipboard, ClipboardKind, InjectClipboard};
pub use framepace::{next_ema, target_interval, PACE_EMA_ALPHA, PACE_K, PACE_MAX_MS, PACE_MIN_MS};
pub use ime::{normalize_ime_commit, preedit_display};
pub use paste::{
    drain_pending, drain_to_writer, normalize_paste, paste_chunks, DrainBudget, DrainReport, PASTE_CHUNK,
};
pub use scrollbar::{scrollbar_geometry, track_y_to_offset, ScrollbarGeom};
pub use selection::{
    abs_to_viewport, auto_scroll_step, extract_rows, extract_text, selection_viewport_region,
    viewport_to_abs, word_region, CellPos, Selection,
};

/// 帧耗时诊断统计（APP_PERF_FRAMES=1；性能诊断日志，release 保留，默认关闭）。
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
        let feed_rate = elapsed_s
            .map(|s| self.feed_bytes as f64 / s)
            .unwrap_or(0.0);
        eperf!(
            "[perf] frames n={} render_ms(med={} p95={}) present_ms(med={} p95={}) coalesced={} interval_ms={} ~{}fps feed_bytes_per_s={:.0} chan_depth_max={} loop_gap_max_ms={} feed_ms_max={:.1} feed_bytes_max={} writer_queued_max={} {}",
            Self::N, r_med, r_p95, p_med, p_p95, self.coalesced, self.interval_ms, fps,
            feed_rate, self.chan_depth_max, self.loop_gap_max_us / 1000,
            self.feed_ms_max, self.feed_bytes_max, self.writer_queued_max, mem
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

type SoftSurface =
    softbuffer::Surface<winit::event_loop::OwnedDisplayHandle, Rc<Window>>;

/// 应用程序配置。
pub struct App {
    /// 初始列数。
    pub cols: u16,
    /// 初始行数。
    pub rows: u16,
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
        let event_loop = EventLoop::new()
            .map_err(|e| Error::App(format!("EventLoop::new: {}", e)))?;
        // 阶段 4H t2：EventLoop::new（X11/socket 连接、平台后端初始化）此前无打点。
        if let Some(t0) = crate::start_time() {
            eperf!(
                "[perf] event_loop_ms={:.1} since_start={:.1}",
                t_loop.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let mut handler = AppHandler {
            perf_reset_at: std::env::var("APP_PERF_RESET_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|ms| Instant::now() + std::time::Duration::from_millis(ms)),
            config: Some(self),
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
            force_full_redraw: std::env::var("APP_FORCE_FULL_REDRAW").is_ok(),
            drained_any: false,
            last_render: None,
            urgent_render: false,
            #[cfg(debug_assertions)]
            no_render: std::env::var("APP_NO_RENDER").is_ok(),
            #[cfg(debug_assertions)]
            no_throttle: std::env::var("APP_NO_THROTTLE").is_ok(),
            present_full: std::env::var("APP_PRESENT_FULL").is_ok(),
            render_ema_ms: None,
            frame_interval: Duration::from_millis(PACE_MIN_MS as u64),
            perf_frames: std::env::var("APP_PERF_FRAMES")
                .is_ok()
                .then(PerfFrames::default),
            #[cfg(debug_assertions)]
            present_stats: std::env::var("APP_PRESENT_STATS")
                .is_ok()
                .then(|| (std::env::var("APP_PRESENT_FULL").is_ok(), Vec::new())),
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
            event_proxy: Some(event_loop.create_proxy()),
            bg_rx: None,
            mods_state: ModifiersState::default(),
            first_frame_logged: false,
            first_pty_byte_logged: false,
            pre_first_submit_logged: false,
            writer: None,
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
            pty::unix::join_thread_timeout(thread, Duration::from_secs(5))
                .map_err(Error::from)?;
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
    #[cfg(target_os = "linux")]
    Gpu(Box<render::gpu::GpuRenderer>),
}

impl RendererBackend {
    /// 后端名字（日志/诊断用）。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cpu(_) => "cpu",
            #[cfg(target_os = "linux")]
            Self::Gpu(_) => "gpu",
        }
    }

    /// 是否 GPU 后端（app 层据此决定叠加层/脏区分发）。
    pub fn is_gpu(&self) -> bool {
        match self {
            Self::Cpu(_) => false,
            #[cfg(target_os = "linux")]
            Self::Gpu(_) => true,
        }
    }

    /// 当前字体的格子尺寸（物理像素）。
    pub fn cell_metrics(&self, scale: f64) -> (u32, u32) {
        match self {
            Self::Cpu(r) => r.cell_metrics(scale),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.cell_metrics(scale),
        }
    }

    /// 更新缩放因子（字形缓存失效 + 整屏重绘）。
    pub fn set_scale_factor(&mut self, scale: f64) {
        match self {
            Self::Cpu(r) => r.set_scale_factor(scale),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.set_scale_factor(scale),
        }
    }

    /// 加载后台字体（CJK 回退）。
    pub fn add_background_fonts(&mut self, fonts: Vec<render::Font>) {
        match self {
            Self::Cpu(r) => r.add_background_fonts(fonts),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.add_background_fonts(fonts),
        }
    }

    /// 选区块（内容绝对坐标，含端点）。
    pub fn set_selection(&mut self, region: Option<(u16, u16, u16, u16)>) {
        match self {
            Self::Cpu(r) => r.set_selection(region),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.set_selection(region),
        }
    }

    /// 滚动条拇指几何 (thumb_y, thumb_h)。
    pub fn set_scrollbar(&mut self, geom: Option<(u32, u32)>) {
        match self {
            Self::Cpu(r) => r.set_scrollbar(geom),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.set_scrollbar(geom),
        }
    }

    /// IME 预编辑叠加层。
    pub fn set_preedit(&mut self, overlay: Option<render::PreeditOverlay>) {
        match self {
            Self::Cpu(r) => r.set_preedit(overlay),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.set_preedit(overlay),
        }
    }

    /// 渲染一帧（CPU：写入内部帧缓冲，随后由 softbuffer 提交）。
    pub fn render(
        &mut self,
        frame: &vt::FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<(), render::Error> {
        match self {
            Self::Cpu(r) => r.render(frame, pixel_width, pixel_height),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.render_surface(frame, pixel_width, pixel_height),
        }
    }

    /// 本帧脏区（CPU 专有；GPU 走后端自己的整帧提交）。
    pub fn damage(&self) -> &[(u32, u32, u32, u32)] {
        match self {
            Self::Cpu(r) => r.damage(),
            // GPU 后端每帧整帧提交，没有"脏区"概念（surface 纹理内容不保留）。
            #[cfg(target_os = "linux")]
            Self::Gpu(_) => &[],
        }
    }

    /// 帧缓冲（softbuffer 提交与诊断用）。
    pub fn framebuffer(&self) -> &[u32] {
        match self {
            Self::Cpu(r) => r.framebuffer(),
            #[cfg(target_os = "linux")]
            Self::Gpu(_) => &[],
        }
    }

    /// 帧缓冲尺寸。
    pub fn framebuffer_size(&self) -> (u32, u32) {
        match self {
            Self::Cpu(r) => r.framebuffer_size(),
            #[cfg(target_os = "linux")]
            Self::Gpu(r) => r.pixel_size(),
        }
    }

    /// 取 CPU 渲染器引用（诊断钩子/帧哈希用；GPU 后端返回 None）。
    pub fn as_cpu(&self) -> Option<&CpuRenderer> {
        match self {
            Self::Cpu(r) => Some(r),
            #[cfg(target_os = "linux")]
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
            #[cfg(target_os = "linux")]
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
    let build_cpu = |scale: f64, existing: Option<RendererBackend>| -> Result<RendererBackend, Error> {
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
            #[cfg(target_os = "linux")]
            {
                let t = std::time::Instant::now();
                let reuse = existing.as_ref().and_then(|r| match r {
                    RendererBackend::Cpu(c) => Some(c.font_groups()),
                    _ => None,
                });
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
            #[cfg(not(target_os = "linux"))]
            {
                build_cpu(scale, existing)
            }
        }
        config::Backend::Gpu => {
            #[cfg(target_os = "linux")]
            {
                let t = std::time::Instant::now();
                let reuse = existing.as_ref().and_then(|r| match r {
                    RendererBackend::Cpu(c) => Some(c.font_groups()),
                    _ => None,
                });
                let (gpu, adapter) = build_gpu(font_settings, options, palette, window, scale, reuse)?;
                eperf!(
                    "[perf] gpu_init_ms={:.1} backend=gpu adapter='{}'",
                    t.elapsed().as_secs_f64() * 1000.0,
                    adapter
                );
                Ok(gpu)
            }
            #[cfg(not(target_os = "linux"))]
            {
                eprintln!("[renderer] backend=gpu 在当前平台不可用，回退 cpu");
                build_cpu(scale, existing)
            }
        }
    }
}

/// 用 winit 窗口的 raw handle 创建 GPU surface 渲染器（仅 Linux）。
#[cfg(target_os = "linux")]
fn build_gpu(
    font_settings: &render::FontSettings,
    options: render::RenderOptions,
    palette: render::Palette,
    window: &winit::window::Window,
    scale: f64,
    existing_cpu_groups: Option<std::sync::Arc<Vec<Vec<std::sync::Arc<render::Font>>>>>
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
    let reuse = existing_cpu_groups;
    let mut gpu = render::gpu::GpuRenderer::new_surface_with_groups(
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
    )?;
    gpu.set_scale_factor(scale);
    let name = gpu.adapter_name().to_string();
    Ok((RendererBackend::Gpu(Box::new(gpu)), name))
}

struct AppHandler {
    config: Option<App>,
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
    /// 首帧 present 耗时日志标志（perf 钩子，仅打印一次）。
    first_frame_logged: bool,
    /// 首个 PTY 字节到达日志标志（perf 钩子，仅打印一次）。
    first_pty_byte_logged: bool,
    /// PTY 写线程（阶段 4H t3）：拿到可独立持有的写端时启用；None = 沿用同步写
    /// （Windows ConPTY 的 `take_writer` 走 trait 默认实现返回 None）。
    writer: Option<writer::PtyWriter>,
    /// 上次事件循环迭代时刻（阶段 4G t2a 的 loop gap 观测）。
    last_loop_at: Option<std::time::Instant>,
    /// 下次独立打印 loop gap 的时刻（每 5 秒）。
    loop_report_at: Option<std::time::Instant>,
    /// 观测窗口重置时刻（阶段 4I t3，`APP_PERF_RESET_MS`，release 可用）。
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
    /// debug 测试钩子：延迟触发一次同步粘贴（APP_TEST_PASTE / APP_TEST_PASTE_AT_MS）。
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
    bg_rx: Option<mpsc::Receiver<Vec<render::Font>>>,
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
    /// 测试钩子：预编辑自动清除时刻（debug 专有，配合 APP_TEST_PREEDIT_CLEAR_MS）。
    #[cfg(debug_assertions)]
    test_preedit_clear_at: Option<std::time::Instant>,
    /// 上次应用的窗口标题（避免每帧重复 set_title）。
    last_title: Option<String>,
    /// 上次渲染时的选区区域（绝对坐标），用于选区叠加层脏判定。
    last_sel_region: Option<(u16, u16, u16, u16)>,
    /// 上次渲染时的视口偏移（变化即整屏重绘：视口滚动不做位移复用）。
    last_render_offset: usize,
    /// 强制整屏重绘（仅 debug：APP_FORCE_FULL_REDRAW=1，用于 e2e 增量/全量对拍）。
    #[cfg(debug_assertions)]
    force_full_redraw: bool,
    /// 本轮 about_to_wait 是否收到新 PTY 数据（供 follow/截断检查复用，减少快照）。
    drained_any: bool,
    /// 上次渲染时刻（帧合并节流用）。
    last_render: Option<Instant>,
    /// 交互事件触发的紧急渲染（键盘/鼠标/IME/resize）：跳过节流立即渲染。
    urgent_render: bool,
    /// 关闭渲染（仅 debug：APP_NO_RENDER=1，用于吞吐基线"只 feed 不渲染"）。
    #[cfg(debug_assertions)]
    no_render: bool,
    /// 关闭帧合并节流（仅 debug：APP_NO_THROTTLE=1），用于对照"节流前"的输入回显延迟。
    #[cfg(debug_assertions)]
    no_throttle: bool,
    /// 强制整缓冲 present（诊断开关 APP_PRESENT_FULL=1，release 与 debug 均可用，
    /// 默认关闭；用于对比脏区提交与全缓冲提交的 present 耗时，与 APP_PERF_FRAMES 同类）。
    present_full: bool,
    /// 渲染耗时 EMA（毫秒），用于自适应帧间隔；None 表示尚无样本。
    render_ema_ms: Option<f64>,
    /// 当前目标帧间隔（由 EMA 推导，最小 PACE_MIN_MS）。
    frame_interval: Duration,
    /// 帧耗时诊断采样（APP_PERF_FRAMES=1，release 与 debug 均可用，默认关闭）。
    /// 每 50 帧输出一行：渲染/present 中位与 p95、被合并丢弃的唤醒次数、当前帧间隔。
    perf_frames: Option<PerfFrames>,
    /// present 耗时采样（仅 debug：APP_PRESENT_STATS=1 每 100 帧打印一次）。
    #[cfg(debug_assertions)]
    present_stats: Option<(bool, Vec<u128>)>,
}

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let t_resumed = Instant::now();
        let config = self
            .config
            .take()
            .expect("resumed 只能调用一次");
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
            .unwrap_or_else(|| "tty".to_string());
        attrs.title = bin_name.clone();
        // 初始位置（逻辑像素；Wayland 下合成器可能忽略——记录日志，不视为错误；
        // 越出屏幕范围交给窗口管理器处理，不自行钳制）。
        if let Some(pos) = config.config.window.position {
            attrs = attrs.with_position(winit::dpi::LogicalPosition::new(
                pos.x as f64,
                pos.y as f64,
            ));
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
        // window.position：winit 0.30 的 X11 后端在创建路径上不应用 attributes.position
        // （只在映射后经 set_outer_position 生效），因此创建后显式设置一次。
        // Wayland 下合成器可能忽略（已有提示日志）；越界交给窗口管理器。
        if let Some(pos) = config.config.window.position {
            window.set_outer_position(winit::dpi::LogicalPosition::new(
                pos.x as f64,
                pos.y as f64,
            ));
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
            )
                && let Err(e) = s.resize(w, h)
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
        if let Err(e) = pty.spawn(&config.shell, config.cols, config.rows) {
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
        let bg_proxy = self.event_proxy.clone();
        let waker: Box<dyn Wake> = match self.event_proxy.take() {
            Some(proxy) => Box::new(ProxyWaker::new(proxy)),
            None => {
                eprintln!("create_proxy: no proxy available");
                return;
            }
        };
        // 任务 2B.1：fallback/系统兜底字体后台懒加载（首帧不等 CJK from_bytes 实测
        // 806.8ms）；到位经 channel 并入 renderer 并发 proxy 唤醒重绘。线程安全：
        // 仅 owned 数据（FontSpec/Font）经 channel 传递；Database 线程内自建；
        // 线程 detached（完成即退，进程退出随灭，无共享可变状态）。
        let (bg_tx, bg_rx) = mpsc::channel::<Vec<render::Font>>();
        let bg_specs = config.font_settings.fallback.clone();
        std::thread::spawn(move || {
            let fonts = render::load_font_list(&bg_specs);
            let _ = bg_tx.send(fonts);
            if let Some(proxy) = bg_proxy {
                let _ = proxy.send_event(());
            }
        });
        self.bg_rx = Some(bg_rx);

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
                    f64::from(config.cols) * f64::from(cw) / scale
                        + 2.0 * f64::from(config.pad_x),
                    f64::from(config.rows) * f64::from(ch) / scale
                        + 2.0 * f64::from(config.pad_y),
                ))
                .is_none()
            {
                // winit 0.30：request_inner_size 返回 None 表示"请求已异步提交，
                // 结果稍后经 WindowEvent::Resized 返回"，不是被窗口管理器拒绝
                // （阶段 4B-fix1 任务 B 取证）。实际尺寸以 Resized 为准，
                // Resized 分支会按窗口真实像素反算 cols/rows 并 resize vt/pty。
                eprintln!("request_inner_size: 尺寸请求异步提交，实际尺寸以 Resized 事件为准");
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
        // 测试专用开关（仅 debug 构建；release 中不存在）：APP_TEST_PREEDIT
        // 注入一个预编辑串，供 e2e 断言叠加层渲染而不经过真实输入法。
        #[cfg(debug_assertions)]
        if let Ok(t) = std::env::var("APP_TEST_PREEDIT")
            && !t.is_empty()
        {
            self.test_preedit = Some(t);
            // 配套测试钩子（debug 专有）：设置 APP_TEST_PREEDIT_CLEAR_MS 后，
            // 从注入时刻起经过该毫秒数即清除预编辑，用于验证"清除后不留残影"
            // （阶段 4B-fix1 任务 C 的 e2e 场景）。
            if let Ok(ms) = std::env::var("APP_TEST_PREEDIT_CLEAR_MS")
                && let Ok(ms) = ms.parse::<u64>()
            {
                self.test_preedit_clear_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                eperf!("[perf] test_preedit_clear_scheduled ms={ms}");
            }
        }
        // 测试钩子（debug 专有）：延迟触发一次同步粘贴，用于验证"粘贴后主动排帧"。
        #[cfg(debug_assertions)]
        if let Ok(t) = std::env::var("APP_TEST_PASTE")
            && !t.is_empty()
        {
            let at_ms = std::env::var("APP_TEST_PASTE_AT_MS")
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
        // 唤醒：读线程来数据后经 proxy 发来；about_to_wait 将消费 channel。
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
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
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                // HiDPI：重算格子物理尺寸 → InnerSizeWriter 保持 dimensions 行列数
                // （逻辑尺寸 = 行列*逻辑格子 + 2*padding）；随后 Resized 走统一路径
                // 重排网格并 resize vt/pty。字形位图缓存整体失效（set_scale_factor）。
                let (Some(renderer), Some(app_cfg)) = (self.renderer.as_mut(), self.config.as_ref())
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
            WindowEvent::ModifiersChanged(m) => self.mods_state = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                self.urgent_render = true;
                if event.state == ElementState::Pressed {
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
                                    Ok(mut enc) => match enc.encode_key(key, mods, Action::Press) {
                                        Ok(bytes) => {
                                            // 阶段 4I t2：Ctrl-C 取消进行中的粘贴。
                                            // 写线程只能丢弃"已在队列里"的块（并补发括号
                                            // 粘贴结束标记），但 app 侧的 pending_paste
                                            // 会在下一帧继续回灌，粘贴实际没停（实测：
                                            // 首次 Ctrl-C 后仍持续写入，第二次 Ctrl-C 又丢
                                            // 1MiB）。中断语义要求粘贴整体作废，故此处清空。
                                            if bytes.contains(&0x03) {
                                                self.pending_paste.clear();
                                            }
                                            self.follow_viewport_before_input(
                                                follow::InputEvent::EncodedKey,
                                            );
                                            if let Some(pty) = self.pty.as_mut() {
                                                write_pty(pty, self.writer.as_ref(), &bytes);
                                            }
                                        }
                                        Err(e) => eprintln!("encode: {}", e),
                                    },
                                    Err(e) => eprintln!("key_encoder: {}", e),
                                }
                            }
                        }
                    }
                    }
                }
            }
            WindowEvent::Resized(physical_size) => {
                self.urgent_render = true;
                let pad_x = u32::from(self.pad_x) * self.scale_factor as u32;
                let pad_y = u32::from(self.pad_y) * self.scale_factor as u32;
                // 零尺寸/小于一格（Windows 最小化会发 1×1 或 0×0）：保持原网格、
                // 原渲染尺寸与 surface 配置不变，只请求一次重绘（恢复时的 Resized
                // 会走正常路径）。若在此处 resize，内容会被裁成 1×1 且不可恢复。
                let Some((cols, rows)) = resize_target(
                    physical_size.width,
                    physical_size.height,
                    self.cell_w,
                    self.cell_h,
                    pad_x,
                    pad_y,
                ) else {
                    #[cfg(debug_assertions)]
                    eperf!(
                        "[perf] resize_ignored {}x{}（小于一个字符格）",
                        physical_size.width, physical_size.height
                    );
                    return;
                };
                // 幂等：网格未变时不重复 resize（恢复尺寸与最小化前相同时必须无副作用）。
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
                self.last_size = Some((physical_size.width, physical_size.height));
                if let Some(r) = self.renderer.as_mut()
                    && r.is_gpu()
                {
                    r.resize_surface(physical_size.width, physical_size.height);
                }
                if let Some(surface) = self.surface.as_mut()
                    && let (Some(w), Some(h)) = (
                        std::num::NonZeroU32::new(physical_size.width),
                        std::num::NonZeroU32::new(physical_size.height),
                    )
                    && let Err(e) = surface.resize(w, h)
                {
                    eprintln!("surface resize: {}", e);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 阶段 4I t3：观测窗口重置（APP_PERF_RESET_MS，默认关闭）。用于把启动期
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
        // 仅在 APP_PERF_FRAMES 开启时统计（默认关闭，代价一次 Option 判空）。
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
                eperf!(
                    "[perf] loop_gap_ms={ms} feed_ms_max={fms:.1} feed_bytes_max={fb} writer_queued_max={wq}"
                );
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
                        eprintln!("paste drain: pty writer 不可用（{}），丢弃剩余 {} 块", e, self.pending_paste.len());
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
        // 后台字体到位检查：并入 renderer（清缓存）后本帧重绘即用新字体。
        if let Some(rx) = &self.bg_rx
            && let Ok(fonts) = rx.try_recv()
            && let Some(r) = self.renderer.as_mut()
        {
            // perf 钩子（任务 2C.5）：后台字体（CJK fallback）到位时刻，
            // 与首帧耗时同口径（相对进程启动），仅打印一次。
            if let Some(t0) = crate::start_time() {
                eperf!(
                    "[perf] bg_fonts_ms={} count={}",
                    t0.elapsed().as_millis(),
                    fonts.len()
                );
            }
            r.add_background_fonts(fonts);
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
                eprintln!(
                    "[selection] scrollback 截断：选区末行 {} 超出可见历史边界 {}，clamp",
                    max_abs, visible_max
                );
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
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(target));
            }
            return;
        }
        let prev_render = self.last_render;
        self.last_render = Some(now);
        #[cfg(debug_assertions)]
        let was_urgent = self.urgent_render;
        self.urgent_render = false;
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
                    let vis_region = self.selection.and_then(|s| {
                        let (c0, a0, c1, a1) = s.region();
                        selection_viewport_region((c0, a0, c1, a1), self.offset_from_top, self.rows)
                    });
                    if vis_region != self.last_sel_region {
                        if let Some((_, r0, _, r1)) = self.last_sel_region {
                            snap.mark_rows_dirty(r0, r1);
                        }
                        if let Some((_, r0, _, r1)) = vis_region {
                            snap.mark_rows_dirty(r0, r1);
                        }
                        self.last_sel_region = vis_region;
                    }
                    // debug-only 强制全量（e2e 对拍用；release 不含该分支）
                    #[cfg(debug_assertions)]
                    if self.force_full_redraw {
                        snap.full_redraw = true;
                    }
                    let (pw, ph) = self.last_size.unwrap_or((8, 16));
                    renderer.set_selection(vis_region);
                    // 滚动条几何（scrollback=0 时不绘制）
                    let sb = sb_render;
                    let geom = if sb == 0 {
                        None
                    } else {
                        scrollbar_geometry(sb as u64, u64::from(self.rows), self.offset_from_top, ph)
                            .map(|g| (g.thumb_y, g.thumb_h))
                    };
                    renderer.set_scrollbar(geom);
                    // IME 预编辑叠加层（不写入网格；空串=清除）。
                    // debug 测试注入（APP_TEST_PREEDIT）优先于真实 Ime 事件。
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
                    if let Err(e) = renderer.render(&snap, pw, ph) {
                        eprintln!("render: {}", e);
                    }
                    let render_ms = t_render_start.elapsed().as_secs_f64() * 1000.0;
                    self.render_ema_ms = Some(next_ema(self.render_ema_ms, render_ms));
                    self.frame_interval = target_interval(self.render_ema_ms.unwrap_or(render_ms));
                    if let Some(pf) = self.perf_frames.as_mut() {
                        pf.render_us
                            .push((render_ms * 1000.0) as u128);
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
                            if let Some(t0) = crate::start_time() {
                                eperf!("[perf] first_frame_ms={}", t0.elapsed().as_millis());
                            }
                        }
                        self.update_ime_cursor_area();
                        return;
                    }
                    // 本帧脏区（阶段 3A 任务 3）：只拷贝并提交脏区；
                    // 无脏区则跳过 present（静止画面零提交）。
                    let damage: Vec<(u32, u32, u32, u32)> = renderer.damage().to_vec();
                    let fb = renderer.framebuffer();
                    let fb_w = pw;
                    let Some(surface) = surface_guard.as_mut() else {
                        return;
                    };
                    // APP_PRESENT_FULL=1 时强制整缓冲提交（诊断用，release 亦可用）。
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
                        return;
                    }
                    match surface.buffer_mut() {
                        Ok(mut buffer) => {
                            let t_present = Instant::now();
                            // 拷贝：damage 覆盖整屏时整缓冲拷贝，否则按矩形逐行拷贝。
                            let full_cover = force_full_present
                                || damage.len() == 1
                                    && damage[0] == (0, 0, pw, ph);
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
                                        let src_end = (row * fb_w + x + w).min((row + 1) * fb_w) as usize;
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
        // IME 光标区跟随快照光标；该调用内部会取快照，故仅在本次确实要渲染时执行
        // （高频输出下每帧多一次全量快照会显著拖慢 channel 排空）。
        let will_render = match self.last_render {
            Some(t) => self.urgent_render || Instant::now().duration_since(t) >= RENDER_MIN_INTERVAL,
            None => true,
        };
        if will_render {
            self.update_ime_cursor_area();
        }
        // 钩子待触发时必须保持定时唤醒：渲染路径在"到期渲染"后不设 ControlFlow，
        // 事件循环会进入无限 Wait，导致 APP_TEST_PASTE_AT_MS 永远不触发（实测）。
        #[cfg(debug_assertions)]
        if let Some(t) = self.test_paste_at {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(t));
        }
    }
}

impl AppHandler {
    /// 鼠标是否命中滚动条条带（右缘 4px；条带判定优先于选区坐标转换）。
    fn hit_scrollbar(&self, position: Option<winit::dpi::PhysicalPosition<f64>>) -> bool {
        let Some(pos) = position else { return false; };
        let Some((w, _h)) = self.last_size else { return false; };
        pos.x >= w.saturating_sub(render::SCROLLBAR_WIDTH) as f64
            && w > render::SCROLLBAR_WIDTH
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
        Some(CellPos { col: col as u16, row: row as u16 })
    }

    /// 鼠标按钮事件：上报优先（非 Shift 且 vt 开跟踪→编码写 PTY）；
    /// 否则走选区（左键）与绑定（中/右键）。Shift 绕过上报。
    fn on_mouse_input(&mut self, state: ElementState, button: MouseButton) {
        let shift = self.mods_state.shift_key();
        if !shift && self.vt.as_ref().is_some_and(|v| v.is_mouse_tracking().unwrap_or(false)) {
            // 位置先算（避免与下方 vt/pty 可变借用冲突）。
            let pos = self
                .last_mouse_pos
                .and_then(|p| self.pixel_to_cell(p))
                .map(|c| vt::Position { x: f32::from(c.col), y: f32::from(c.row) })
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
                        self.offset_from_top = track_y_to_offset(
                            y as u32,
                            track_h,
                            sb as u64,
                        )
                        .min(sb);
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
            self.offset_from_top =
                track_y_to_offset(position.y as u32, track_h, sb as u64).min(sb);
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
            && self.vt.as_ref().is_some_and(|v| v.is_mouse_tracking().unwrap_or(false))
        {
            let pos = self
                .pixel_to_cell(position)
                .map(|c| vt::Position { x: f32::from(c.col), y: f32::from(c.row) })
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

    /// 更新 IME 光标区（快照光标所在格像素矩形；窗口未建时跳过）。
    fn update_ime_cursor_area(&self) {
        let (Some(window), Some(vt)) = (self.window.as_ref(), self.vt.as_ref()) else {
            return;
        };
        let Ok(snap) = vt.snapshot() else { return; };
        // Preedit 期间 IME 候选窗跟随组合末尾（任务 2C.3），否则跟随终端光标。
        let (cy_cell, cx_cell) = ime_cursor_cell(
            &self.ime_preedit,
            snap.cursor.x,
            snap.cursor.y,
            snap.cols,
            snap.rows,
        );
        let cx = u32::from(cx_cell);
        let cy = u32::from(cy_cell);
        let x = f64::from(self.pad_x) * self.scale_factor + f64::from(cx) * f64::from(self.cell_w);
        let y = f64::from(self.pad_y) * self.scale_factor + f64::from(cy) * f64::from(self.cell_h);
        window.set_ime_cursor_area(
            winit::dpi::LogicalPosition::new(x, y),
            winit::dpi::LogicalSize::new(f64::from(self.cell_w), f64::from(self.cell_h)),
        );
    }

    /// 滚轮（任务 3/4）：鼠标跟踪模式下上报 Four/Five（非 Shift 优先）；备用屏且
    /// alt_screen_scroll 时发方向键；否则 scroll_viewport Delta（×multiplier）。
    fn on_mouse_scroll(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_x, y) => y as isize,
            MouseScrollDelta::PixelDelta(pos) => {
                (pos.y / f64::from(self.cell_h.max(1))) as isize
            }
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
                    .map(|c| vt::Position { x: f32::from(c.col), y: f32::from(c.row) })
                    .unwrap_or(vt::Position { x: 0.0, y: 0.0 });
                let mods = vt_mods_from(&self.mods_state);
                if let (Some(vt), Some(pty)) = (self.vt.as_mut(), self.pty.as_mut())
                    && let Ok(mut enc) = vt.mouse_encoder()
                    && let Ok(bytes) = enc.encode_button(vt::MouseAction::Press, Some(btn), mods, pos)
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
        if alternate && self.config.as_ref().is_some_and(|c| c.config.scrolling.alt_screen_scroll)
        {
            let mult = self
                .config
                .as_ref()
                .map(|c| c.config.scrolling.multiplier)
                .unwrap_or(3.0)
                .max(1.0) as isize;
            let key = if lines > 0 { vt::Key::ArrowUp } else { vt::Key::ArrowDown };
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
            2 => self.word_region_at(pos.row, pos.col).map(|(sc, ec)| Selection {
                anchor: CellPos { col: sc, row: abs_pos.row },
                focus: CellPos { col: ec, row: abs_pos.row },
            }),
            3 => Some(Selection {
                anchor: CellPos { col: 0, row: abs_pos.row },
                focus: CellPos { col: self.cols.saturating_sub(1), row: abs_pos.row },
            }),
            _ => Some(Selection::new(abs_pos)),
        };
        if let Some(r) = region {
            self.selection = Some(r);
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
                if matches!(cell.wide, vt::CellWide::SpacerTail | vt::CellWide::SpacerHead) {
                    continue;
                }
                line.push_str(&cell.chars);
            }
        }
        line
    }

    /// 选区定型/复制：提取文本写 PRIMARY；save_to_clipboard 时兼写 CLIPBOARD。
    fn commit_selection(&mut self) {
        let Some(sel) = self.selection else { return; };
        let Some(vt) = self.vt.as_ref() else { return; };
        let Ok(snap) = vt.snapshot() else { return; };
        // 绝对行→视口行（视口内走 snapshot；跨视口走 rows_text 只读读历史，
        // 同语义提取 extract_rows，不改变视口）。
        let (c0, a0, c1, a1) = sel.region();
        let offset = self.offset_from_top;
        let viewport_rows = self.rows as usize;
        let in_viewport = (a0 as usize) >= offset && (a1 as usize) < offset + viewport_rows;
        let text = if in_viewport {
            let v0 = (a0 as usize - offset).min(u16::MAX as usize) as u16;
            let v1 = (a1 as usize - offset).min(u16::MAX as usize) as u16;
            extract_text(&snap, (c0, v0, c1, v1))
        } else {
            match vt.rows_text(a0 as usize, a1 as usize, self.cols) {
                Ok(history) => {
                    let rows_ref: Vec<(&[Cell], bool)> = history
                        .iter()
                        .map(|(cs, w)| (cs.as_slice(), *w))
                        .collect();
                    extract_rows(&rows_ref, c0, c1)
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
        let Some(clip) = self.clipboard.as_mut() else { return; };
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
                self.offset_from_top = self.offset_from_top.saturating_sub(self.rows as usize).min(sb);
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
        let sb_rows = self.vt.as_ref().map(|v| v.scrollback_rows().unwrap_or(0)).unwrap_or(0);
        let (glyphs, fb_bytes) = match self.renderer.as_ref() {
            Some(RendererBackend::Cpu(c)) => c.cache_stats(),
            // GPU 后端不回读帧缓冲：字形缓存复用同一份 Font 链，缓存规模不可直接取。
            // （Gpu 变体仅 Linux 存在，此处按平台门控，保证 Windows 目标可编译。）
            #[cfg(target_os = "linux")]
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
    /// `do_paste`（剪贴板）与 debug 测试钩子 `APP_TEST_PASTE` 都走这里，
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
    Some((cols.min(u16::MAX as u32) as u16, rows.min(u16::MAX as u32) as u16))
}

/// IME 事件对预编辑状态的更新（纯函数，便于单测）：
/// 返回 (是否改动, 新文本, 新光标范围)。Enabled 不改动（仅重新定位候选窗）。
pub fn ime_preedit_update(
    event: &winit::event::Ime,
) -> (bool, String, Option<(usize, usize)>) {
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

/// 诊断钩子（仅 debug，默认关闭）：APP_FRAME_HASH=<路径> 时，每帧渲染后追加一行
/// `frame=<n> all=<hash> rows=<h0,h1,...>`，用于逐帧对拍"复用 vs 强制全量"，
/// 定位首个出现像素差异的帧与差异行范围（阶段 3C 任务 6）。
#[cfg(debug_assertions)]
fn dump_frame_hash_if_requested(renderer: &render::CpuRenderer) {
    let Ok(path) = std::env::var("APP_FRAME_HASH") else { return; };
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
    let line = format!("frame={n} all={all:016x} rows={}
", rows.join(","));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 测试钩子（默认关闭）：APP_DUMP_SNAPSHOT 设置时，每帧把 FrameSnapshot 文本
/// （行文本 + 宽格统计）写入该路径，供 e2e 断言而不依赖 OCR。
/// 测试钩子（默认关闭，仅 debug 构建；任务 4 门控）：APP_DUMP_SNAPSHOT 设置时，
/// 每帧把 FrameSnapshot 文本（行文本 + 宽格统计 + 每格字体索引）写入该路径，
/// 供 e2e 断言而不依赖 OCR。release 构建中本函数与钩子字符串均不存在。
#[cfg(debug_assertions)]
fn dump_snapshot_if_requested(snap: &vt::FrameSnapshot, renderer: Option<&render::CpuRenderer>) {
    let Ok(path) = std::env::var("APP_DUMP_SNAPSHOT") else { return; };
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
            text.push_str(&line.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" "));
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
    vt::Key::A, vt::Key::B, vt::Key::C, vt::Key::D, vt::Key::E, vt::Key::F, vt::Key::G, vt::Key::H,
    vt::Key::I, vt::Key::J, vt::Key::K, vt::Key::L, vt::Key::M, vt::Key::N, vt::Key::O, vt::Key::P,
    vt::Key::Q, vt::Key::R, vt::Key::S, vt::Key::T, vt::Key::U, vt::Key::V, vt::Key::W, vt::Key::X,
    vt::Key::Y, vt::Key::Z,
];

const DIGITS: [vt::Key; 10] = [
    vt::Key::Digit0, vt::Key::Digit1, vt::Key::Digit2, vt::Key::Digit3, vt::Key::Digit4,
    vt::Key::Digit5, vt::Key::Digit6, vt::Key::Digit7, vt::Key::Digit8, vt::Key::Digit9,
];

/// winit 逻辑键 → vt::Key（阶段1 子集：命名键 + ASCII 字母数字）。
fn map_logical_key(key: &WinitKey) -> Option<vt::Key> {
    match key {
        WinitKey::Named(NamedKey::Enter) => Some(vt::Key::Enter),
        WinitKey::Named(NamedKey::Tab) => Some(vt::Key::Tab),
        WinitKey::Named(NamedKey::Backspace) => Some(vt::Key::Backspace),
        WinitKey::Named(NamedKey::Escape) => Some(vt::Key::Escape),
        WinitKey::Named(NamedKey::ArrowUp) => Some(vt::Key::ArrowUp),
        WinitKey::Named(NamedKey::ArrowDown) => Some(vt::Key::ArrowDown),
        WinitKey::Named(NamedKey::ArrowLeft) => Some(vt::Key::ArrowLeft),
        WinitKey::Named(NamedKey::ArrowRight) => Some(vt::Key::ArrowRight),
        WinitKey::Named(NamedKey::Home) => Some(vt::Key::Home),
        WinitKey::Named(NamedKey::End) => Some(vt::Key::End),
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
        fallback: config.font.fallback.iter().map(spec).collect(),
    }
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
    let c = |c: config::Rgb| vt::Rgb { r: c.r, g: c.g, b: c.b };
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
    let c = |c: config::Rgb| vt::Rgb { r: c.r, g: c.g, b: c.b };
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
        let bindings = vec![b("c", true, ConfigAction::Copy), b("v", true, ConfigAction::Paste)];
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
        assert_eq!(ime_cursor_cell("", 5, 3, 80, 24), (3, 5), "无 Preedit 跟随光标（row,col）");
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
        assert_eq!(ime_cursor_cell("", 0, 23, 80, 24), (23, 0), "清空后回到光标格");
    }
}

/// IME 事件状态更新单测（任务 2C.3：Preedit/Commit/Disabled 清空语义）。
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
