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
use std::sync::mpsc;
use std::time::Duration;

use pty::{NativePty, Pty};
use render::{CpuRenderer, Renderer};
use vt::{Action, Mods, VtTerminal};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes};

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
    tx: &mpsc::Sender<Vec<u8>>,
    waker: &dyn Wake,
    report: &mut dyn FnMut(&str),
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
                if let Err(e) = waker.wake() {
                    if !matches!(e, Error::EventLoopClosed) {
                        report(&format!("wake: {}", e));
                    }
                    break;
                }
            }
        }
    }
}

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
        }
    }

    /// 运行事件循环（含上述六条接线）。
    pub fn run(self) -> Result<(), Error> {
        let event_loop = EventLoop::new()
            .map_err(|e| Error::App(format!("EventLoop::new: {}", e)))?;
        let mut handler = AppHandler {
            config: Some(self),
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
            mods_state: ModifiersState::default(),
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
    /// 事件循环代理（run_app 前创建，读线程经其唤醒事件循环）。
    event_proxy: Option<winit::event_loop::EventLoopProxy<()>>,
    mods_state: ModifiersState,
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
        attrs.title = "tty".to_string();
        attrs.inner_size = Some(winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
            f64::from(config.cols) * f64::from(init_cw) + 2.0 * f64::from(config.pad_x),
            f64::from(config.rows) * f64::from(init_ch) + 2.0 * f64::from(config.pad_y),
        )));
        // 启动失败路径：记录原因并退出事件循环，不 panic（外部输入/系统调用须覆盖失败路径）。
        let window = Rc::new(match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("create_window: {}", e);
                return;
            }
        });
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
        let waker: Box<dyn Wake> = match self.event_proxy.take() {
            Some(proxy) => Box::new(ProxyWaker::new(proxy)),
            None => {
                eprintln!("create_proxy: no proxy available");
                return;
            }
        };

        // 1. PTY 读线程 → channel → 主线程喂 vt
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
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
            WindowEvent::ModifiersChanged(m) => self.mods_state = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let mut mods = Mods::empty();
                    if self.mods_state.shift_key() {
                        mods |= Mods::SHIFT;
                    }
                    if self.mods_state.control_key() {
                        mods |= Mods::CTRL;
                    }
                    if self.mods_state.alt_key() {
                        mods |= Mods::ALT;
                    }
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
            WindowEvent::Resized(physical_size) => {
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
        // 2. channel 收字节 → 喂 vt
        if let Some(rx) = self.rx.as_ref()
            && let Some(vt) = self.vt.as_mut()
        {
            while let Ok(bytes) = rx.try_recv() {
                vt.feed(&bytes);
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
        // 渲染一帧（尺寸取缓存，窗口销毁后不查询 X11 geometry）
        if let (Some(vt), Some(renderer), Some(surface)) =
            (self.vt.as_ref(), self.renderer.as_mut(), self.surface.as_mut())
        {
            match vt.snapshot() {
                Ok(snap) => {
                    #[cfg(debug_assertions)]
                    dump_snapshot_if_requested(&snap, Some(&*renderer));
                    let (pw, ph) = self.last_size.unwrap_or((8, 16));
                    if let Err(e) = renderer.render(&snap, pw, ph) {
                        eprintln!("render: {}", e);
                    }
                    match surface.buffer_mut() {
                        Ok(mut buffer) => {
                            let fb = renderer.framebuffer();
                            for (i, px) in fb.iter().enumerate() {
                                if i < buffer.len() {
                                    buffer[i] = *px;
                                }
                            }
                            if let Err(e) = buffer.present() {
                                eprintln!("surface present: {}", e);
                            }
                        }
                        Err(e) => eprintln!("buffer_mut: {}", e),
                    }
                }
                Err(e) => eprintln!("snapshot: {}", e),
            }
        }
    }
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
