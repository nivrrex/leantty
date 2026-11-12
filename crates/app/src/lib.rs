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
mod paste;
mod ime;
mod scrollbar;
mod selection;

pub use clipboard::{ArboardClipboard, Clipboard, ClipboardKind, InjectClipboard};
pub use ime::{normalize_ime_commit, preedit_display};
pub use paste::{
    drain_pending, normalize_paste, paste_chunks, DrainBudget, DrainReport, PASTE_CHUNK,
};
pub use scrollbar::{scrollbar_geometry, track_y_to_offset, ScrollbarGeom};
pub use selection::{
    abs_to_viewport, auto_scroll_step, extract_rows, extract_text, selection_viewport_region,
    viewport_to_abs, word_region, CellPos, Selection,
};

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
        let event_loop = EventLoop::new()
            .map_err(|e| Error::App(format!("EventLoop::new: {}", e)))?;
        let mut handler = AppHandler {
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
            #[cfg(debug_assertions)]
            present_full: std::env::var("APP_PRESENT_FULL").is_ok(),
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
        };
        event_loop
            .run_app(&mut handler)
            .map_err(|e| Error::App(format!("event loop: {}", e)))?;
        // run_app 返回后回收：先 close（先释放主端再回收子进程，kill 子进程使 reader
        // 读端得 EOF 而退出），再带超时 join reader 线程（顺序倒置将空等 5s 超时）。
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
}

struct AppHandler {
    config: Option<App>,
    vt: Option<VtTerminal>,
    pty: Option<NativePty>,
    renderer: Option<CpuRenderer>,
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
    /// 强制整缓冲 present（仅 debug：APP_PRESENT_FULL=1，用于对比 damage 提交）。
    #[cfg(debug_assertions)]
    present_full: bool,
    /// present 耗时采样（仅 debug：APP_PRESENT_STATS=1 每 100 帧打印一次）。
    #[cfg(debug_assertions)]
    present_stats: Option<(bool, Vec<u128>)>,
}

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let config = self
            .config
            .take()
            .expect("resumed 只能调用一次");
        // 字体渲染器先行创建：格子尺寸由字体度量算出（任务2，替代固定 8x16）。
        let renderer = match CpuRenderer::new(&config.font_settings, config.options, config.palette) {
            Ok(mut r) => {
                r.set_scale_factor(1.0);
                r
            }
            Err(e) => {
                eprintln!("CpuRenderer::new: {}", e);
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
        // 阶段 3B 任务 3：滚动位移复用默认关闭；仅 debug 开关启用
        // （混合场景对拍存在未解决差异，见 render 字段文档与 PHASE3B 报告）。
        #[cfg(debug_assertions)]
        let mut renderer = renderer;
        #[cfg(debug_assertions)]
        renderer.set_scroll_reuse(std::env::var("APP_SCROLL_REUSE").is_ok());
        // 启动分段计时（阶段 3A 任务 2）：窗口创建与 surface 初始化。
        if let Some(t0) = crate::start_time() {
            eprintln!(
                "[perf] window_ms={:.1} since_start={:.1}",
                t_window.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        // IME：窗口创建后无条件开启（任务 5）；Xvfb 无 IME 总线时事件不产生，
        // 开启调用本身无害（winit 内部按平台降级）。
        window.set_ime_allowed(true);
        let t_surface = Instant::now();
        let context = match softbuffer::Context::new(event_loop.owned_display_handle()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("softbuffer context: {}", e);
                return;
            }
        };
        let mut surface = match softbuffer::Surface::new(&context, window.clone()) {
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
            && let Err(e) = surface.resize(w, h)
        {
            eprintln!("surface initial resize: {}", e);
            return;
        }
        if let Some(t0) = crate::start_time() {
            eprintln!(
                "[perf] surface_ms={:.1} since_start={:.1}",
                t_surface.elapsed().as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        let mut vt = match VtTerminal::new(config.cols, config.rows) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("VtTerminal::new: {}", e);
                return;
            }
        };
        let mut pty = NativePty::new();
        if let Err(e) = pty.spawn(&config.shell, config.cols, config.rows) {
            eprintln!("pty spawn: {}", e);
            return;
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
        let reader_thread = std::thread::spawn(move || {
            reader_body(&mut reader, &tx, waker.as_ref(), &mut |m| eprintln!("{}", m));
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
                eprintln!("request_inner_size: 窗口未接受 HiDPI 修正尺寸");
            }
            (cw, ch)
        } else {
            (init_cw, init_ch)
        };
        self.cell_w = cell_w;
        self.cell_h = cell_h;
        self.scale_factor = scale;
        self.window = Some(window);
        self.surface = Some(surface);
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
                            if let Some(pty) = self.pty.as_mut() {
                                write_pty(pty, s.as_bytes());
                            }
                        }
                        KeyRoute::Encoded { key, mods } => {
                            if let (Some(key), Some(vt)) = (key, self.vt.as_mut()) {
                                match vt.key_encoder() {
                                    Ok(mut enc) => match enc.encode_key(key, mods, Action::Press) {
                                        Ok(bytes) => {
                                            if let Some(pty) = self.pty.as_mut() {
                                                write_pty(pty, &bytes);
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
                // 窗口物理尺寸 = 行列*格子(物理) + 2*padding(逻辑*scale)；反算行列，min(1) 兜底。
                let pad_x = u32::from(self.pad_x) * self.scale_factor as u32;
                let pad_y = u32::from(self.pad_y) * self.scale_factor as u32;
                let cw = self.cell_w.max(1);
                let ch = self.cell_h.max(1);
                let cols = (physical_size.width.saturating_sub(2 * pad_x) / cw)
                    .max(1)
                    .min(u16::MAX as u32) as u16;
                let rows = (physical_size.height.saturating_sub(2 * pad_y) / ch)
                    .max(1)
                    .min(u16::MAX as u32) as u16;
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
                self.last_size = Some((physical_size.width, physical_size.height));
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

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // 大段粘贴排空（任务 2D.2 修复）：每轮按预算写出有限字节，剩余数据
        // 通过 request_redraw 在下一帧继续驱动。
        // 原缺陷：每轮只写一块且写完不自驱动，事件循环进入 Wait 后不再运行，
        // 静止场景下粘贴停在第 1 块（Linux 实测 65536 字节后停住）。
        if !self.pending_paste.is_empty() {
            let mut report = crate::paste::DrainReport::default();
            let mut pty_gone = false;
            match self.pty.as_mut() {
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
            if pty_gone {
                // 窗口/PTY 已关闭：未发完的数据丢弃，不挂起事件循环。
                self.pending_paste.clear();
            } else if report.needs_more(self.pending_paste.is_empty())
                && let Some(w) = self.window.as_ref()
            {
                // 主动请求下一帧，避免事件循环空闲停摆（不忙等：每帧只写预算量）。
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
                eprintln!(
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
            while let Ok(bytes) = rx.try_recv() {
                vt.feed(&bytes);
                n_drained += 1;
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
                write_pty(pty, &bytes);
            }
        }
        // 6. 子进程退出 → 关闭窗口
        if let Some(pty) = self.pty.as_mut() {
            match pty.try_wait() {
                Ok(Some(_status)) => _event_loop.exit(),
                Ok(None) => {}
                Err(e) => {
                    eprintln!("try_wait: {}", e);
                    _event_loop.exit();
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
        let due = throttle_disabled
            || match self.last_render {
                Some(t) => self.urgent_render || now.duration_since(t) >= RENDER_MIN_INTERVAL,
                None => true,
            };
        if !due {
            if let Some(t) = self.last_render {
                _event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    t + RENDER_MIN_INTERVAL,
                ));
            }
            return;
        }
        self.last_render = Some(now);
        self.urgent_render = false;
        // 渲染一帧（尺寸取缓存，窗口销毁后不查询 X11 geometry）
        let sb_render = self.sb_rows();
        if let (Some(vt), Some(renderer), Some(surface)) =
            (self.vt.as_ref(), self.renderer.as_mut(), self.surface.as_mut())
        {
            match vt.snapshot_incremental() {
                Ok(mut snap) => {
                    // 叠加层脏合并（阶段 3A 任务 3）：libghostty 只上报行内容脏，
                    // 选区（反色）与视口滚动需 app 侧补齐。
                    // 视口滚动：整屏重绘（不做位移复用，记录为后续优化点）。
                    if self.offset_from_top != self.last_render_offset {
                        snap.full_redraw = true;
                        self.last_render_offset = self.offset_from_top;
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
                    dump_snapshot_if_requested(&snap, Some(&*renderer));
                    if let Err(e) = renderer.render(&snap, pw, ph) {
                        eprintln!("render: {}", e);
                    }
                    // 本帧脏区（阶段 3A 任务 3）：只拷贝并提交脏区；
                    // 无脏区则跳过 present（静止画面零提交）。
                    let damage: Vec<(u32, u32, u32, u32)> = renderer.damage().to_vec();
                    let fb = renderer.framebuffer();
                    let fb_w = pw;
                    #[cfg(debug_assertions)]
                    let force_full_present = self.present_full;
                    #[cfg(not(debug_assertions))]
                    let force_full_present = false;
                    if !force_full_present && damage.is_empty() {
                        // 完全静态帧：不重绘、不提交（仍可更新 perf 日志）。
                        if !self.first_frame_logged {
                            self.first_frame_logged = true;
                            if let Some(t0) = crate::start_time() {
                                eprintln!("[perf] first_frame_ms={}", t0.elapsed().as_millis());
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
                            #[cfg(debug_assertions)]
                            if let Some((full_flag, samples)) = self.present_stats.as_mut() {
                                samples.push(t_present.elapsed().as_micros());
                                if samples.len() >= 20 {
                                    let mut v = samples.clone();
                                    v.sort_unstable();
                                    eprintln!(
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
                            if !self.first_frame_logged {
                                self.first_frame_logged = true;
                                if let Some(t0) = crate::start_time() {
                                    eprintln!("[perf] first_frame_ms={}", t0.elapsed().as_millis());
                                }
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
    }
}

impl AppHandler {
    /// 鼠标是否命中滚动条条带（右缘 4px；条带判定优先于选区坐标转换）。
    fn hit_scrollbar(&self, position: Option<winit::dpi::PhysicalPosition<f64>>) -> bool {
        let Some(pos) = position else { return false; };
        let Some((w, _h)) = self.last_size else { return false; };
        pos.x >= w.saturating_sub(4) as f64 && w > 4
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
                    Ok(bytes) => write_pty(pty, &bytes),
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
                write_pty(pty, &bytes);
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
            if !norm.is_empty()
                && let Some(pty) = self.pty.as_mut()
            {
                write_pty(pty, norm.as_bytes());
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
                    write_pty(pty, &bytes);
                }
                if let (Some(vt), Some(pty)) = (self.vt.as_mut(), self.pty.as_mut())
                    && let Ok(mut enc) = vt.mouse_encoder()
                    && let Ok(bytes) =
                        enc.encode_button(vt::MouseAction::Release, Some(btn), mods, pos)
                {
                    write_pty(pty, &bytes);
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
                    write_pty(pty, &bytes);
                    write_pty(pty, &bytes2);
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

    /// 当前 scrollback 行数（snapshot 查询；失败回落 0）。
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
        let bracketed = self
            .vt
            .as_ref()
            .map(|v| v.is_bracketed_paste().unwrap_or(false))
            .unwrap_or(false);
        let norm = normalize_paste(&text, bracketed);
        if norm.len() >= 1024 * 1024 {
            for chunk in paste_chunks(&norm) {
                self.pending_paste.push_back(chunk.into_bytes());
            }
        } else if let Some(pty) = self.pty.as_mut() {
            write_pty(pty, norm.as_bytes());
        }
    }
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
        if b.key.as_deref() == Some(key) && b.mods == *mods {
            found = Some(b.action);
        }
    }
    found
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

/// 写字节到 PTY 并 flush（外部 I/O 失败路径覆盖）。
fn write_pty(pty: &mut NativePty, bytes: &[u8]) {
    match pty.writer() {
        Ok(w) => {
            if let Err(e) = w.write_all(bytes).and_then(|()| w.flush()) {
                eprintln!("pty write: {}", e);
            }
        }
        Err(e) => eprintln!("pty writer: {}", e),
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
