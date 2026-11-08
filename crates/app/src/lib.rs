//! app crate：winit 事件循环接线。
//!
//! 接线顺序：
//! 1. 起 PTY（pty::Pty::spawn），读线程经 channel 把字节交回主线程；
//! 2. 主线程 channel 收字节 → vt::VtTerminal::feed；
//! 3. 键盘事件 → vt::KeyEncoder → PTY 写；
//! 4. 窗口 resize → vt::VtTerminal::resize 与 pty::Pty::resize；
//! 5. vt::VtTerminal::on_pty_write 回调 → channel → 主线程写 PTY
//!    （回调在 feed 调用栈内同步触发，闭包内不能直接借用 PTY writer，故经 channel 中转）；
//! 6. 子进程退出（pty::Pty::try_wait）→ 关闭窗口。
//!
//! 事件循环用 winit 0.30 的 ApplicationHandler 模式（run_app），窗口经
//! `Rc<Window>` 与 softbuffer Surface 共享句柄，避免闭包自引用借用冲突。

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

/// 阶段1 固定网格像素尺寸（字符格 8x16 像素）。
const CELL_W: u32 = 8;
const CELL_H: u32 = 16;

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
    /// 字体 family 覆盖。
    pub font_family: Option<String>,
    /// 字号覆盖（pt）。
    pub font_size: Option<f32>,
}

impl App {
    /// 新建应用配置。
    pub fn new(cols: u16, rows: u16, shell: impl Into<String>) -> Self {
        Self {
            cols,
            rows,
            shell: shell.into(),
            font_family: None,
            font_size: None,
        }
    }

    /// 命令行字体覆盖。
    pub fn set_font(&mut self, family: Option<String>, size: Option<f32>) {
        self.font_family = family;
        self.font_size = size;
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
            mods_state: ModifiersState::default(),
        };
        event_loop
            .run_app(&mut handler)
            .map_err(|e| Error::App(format!("event loop: {}", e)))?;
        // run_app 返回后回收：读线程带超时 join，再 close（先释放主端再回收子进程）。
        if let Some(thread) = handler.reader_thread.take() {
            pty::unix::join_thread_timeout(thread, Duration::from_secs(5))
                .map_err(Error::from)?;
        }
        if let Some(mut pty) = handler.pty.take() {
            pty.close()?;
        }
        Ok(())
    }
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
    mods_state: ModifiersState,
}

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let config = self
            .config
            .take()
            .expect("resumed 只能调用一次");
        let mut attrs = WindowAttributes::default();
        attrs.title = "tty".to_string();
        attrs.inner_size = Some(winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
            f64::from(config.cols) * f64::from(CELL_W),
            f64::from(config.rows) * f64::from(CELL_H),
        )));
        let window = Rc::new(
            event_loop
                .create_window(attrs)
                .expect("create_window"),
        );
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");

        let mut vt = VtTerminal::new(config.cols, config.rows).expect("VtTerminal::new");
        let mut pty = NativePty::new();
        pty.spawn(&config.shell, config.cols, config.rows).expect("pty spawn");
        let reader = pty.take_reader().expect("take_reader");

        // 1. PTY 读线程 → channel → 主线程喂 vt
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let reader_thread = std::thread::spawn(move || {
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

        // 5. on_pty_write → channel → 主线程写 PTY
        let (pty_write_tx, pty_write_rx) = mpsc::channel::<Vec<u8>>();
        vt.on_pty_write(move |_term, data| {
            let _ = pty_write_tx.send(data.to_vec());
        })
        .expect("on_pty_write");

        let renderer = CpuRenderer::new(config.font_family, config.font_size)
            .expect("CpuRenderer::new");

        self.window = Some(window);
        self.surface = Some(surface);
        self.vt = Some(vt);
        self.pty = Some(pty);
        self.renderer = Some(renderer);
        self.rx = Some(rx);
        self.pty_write_rx = Some(pty_write_rx);
        self.reader_thread = Some(reader_thread);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
                    if let Some(key) = map_logical_key(&event.logical_key)
                        && let Some(vt) = self.vt.as_mut()
                    {
                        match vt.key_encoder() {
                            Ok(mut enc) => {
                                if let Ok(bytes) = enc.encode_key(key, mods, Action::Press)
                                    && let Some(pty) = self.pty.as_mut()
                                    && let Err(e) = pty.writer().map(|w| w.write_all(&bytes))
                                {
                                    eprintln!("pty write: {}", e);
                                }
                            }
                            Err(e) => eprintln!("key_encoder: {}", e),
                        }
                    }
                }
            }
            WindowEvent::Resized(physical_size) => {
                let cols = (physical_size.width / CELL_W).max(1);
                let rows = (physical_size.height / CELL_H).max(1);
                if let Some(vt) = self.vt.as_mut()
                    && let Err(e) = vt.resize(cols as u16, rows as u16)
                {
                    eprintln!("vt resize: {}", e);
                }
                if let Some(pty) = self.pty.as_mut()
                    && let Err(e) = pty.resize(cols as u16, rows as u16)
                {
                    eprintln!("pty resize: {}", e);
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
                if let Err(e) = pty.writer().map(|w| w.write_all(&bytes)) {
                    eprintln!("pty write: {}", e);
                }
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
        // 渲染一帧
        if let (Some(vt), Some(renderer), Some(window), Some(surface)) =
            (self.vt.as_ref(), self.renderer.as_mut(), self.window.as_ref(), self.surface.as_mut())
        {
            match vt.snapshot() {
                Ok(snap) => {
                    let size = window.inner_size();
                    if let Err(e) = renderer.render(&snap, size.width, size.height) {
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
            window.request_redraw();
        }
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
    /// 终端错误。
    #[error("vt error: {0}")]
    Vt(#[from] vt::Error),
    /// 渲染错误。
    #[error("render error: {0}")]
    Render(#[from] render::Error),
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
