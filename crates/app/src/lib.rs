//! app crate：winit 事件循环接线。
//!
//! 接线顺序（实现阶段落地）：
//! 1. 起 PTY（pty::Pty::spawn），读线程经 channel 把字节交回主线程；
//! 2. 主线程 channel 收字节 → vt::VtTerminal::feed；
//! 3. 键盘事件 → vt::KeyEncoder → pty writer；
//! 4. 窗口 resize → vt::VtTerminal::resize 与 pty::Pty::resize；
//! 5. vt::VtTerminal::on_pty_write 回调 → pty writer；
//! 6. 子进程退出（pty::Pty::try_wait）→ 关闭窗口。

use pty::{NativePty, Pty};
use render::CpuRenderer;
use vt::VtTerminal;

/// 渲染错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
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

/// 应用程序。
pub struct App {
    cols: u16,
    rows: u16,
    shell: String,
    font_family: Option<String>,
    font_size: Option<f32>,
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

    /// 运行事件循环（实现阶段完整接线）。
    pub fn run(self) -> Result<(), Error> {
        let mut vt_terminal = VtTerminal::new(self.cols, self.rows)?;
        let mut pty = NativePty::new();
        pty.spawn(&self.shell, self.cols, self.rows)?;
        let mut renderer = CpuRenderer::new(self.font_family, self.font_size)?;
        let _frame = vt_terminal.snapshot()?;
        // 阶段1骨架：winit 事件循环接线在实现阶段完成。
        let _ = &mut renderer;
        let _ = &mut vt_terminal;
        Ok(())
    }
}
