//! pty crate：跨平台 PTY 抽象。
//!
//! 注：`pub mod unix` 不加 cfg 门控——unix.rs 仅用 portable-pty 跨平台 API，
//! 在 Windows 上同样编译；app 的 join_thread_timeout 引用依赖此布局。
//!
//! ## close 契约
//! `close()` 必须先释放 PTY 主端再回收子进程：先等子进程退出会让主端
//! EOF 延迟（Windows 验证数据约 83s），Unix 实现同样遵守该顺序。
//!
//! ## 读线程契约
//! 读线程 join 必须有超时，禁止无界 join。
pub mod unix;

#[cfg(windows)]
pub mod windows;

use portable_pty::ExitStatus;
use std::io::{Read, Write};

#[cfg(unix)]
pub use unix::NativePty;
#[cfg(windows)]
pub use windows::WindowsPty as NativePty;

/// PTY 抽象。实现须遵守模块级 close 与读线程契约。
pub trait Pty {
    /// 起 shell，cols/rows 为行列数。
    fn spawn(&mut self, shell: &str, cols: u16, rows: u16) -> Result<(), Error>;
    /// 带显式启动参数（`terminal.shell.args`）的 spawn。
    ///
    /// 默认实现忽略 args，走既有 `spawn`（Unix 侧不注入任何默认参数，等价；
    /// 阶段 3U Windows 侧覆盖它以让用户配置优先）。
    fn spawn_with_args(
        &mut self,
        shell: &str,
        args: Option<&[String]>,
        cols: u16,
        rows: u16,
    ) -> Result<(), Error> {
        let _ = args;
        self.spawn(shell, cols, rows)
    }
    /// 取走读端（PTY 主端读句柄），供读线程使用；只能取一次。
    fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, Error>;
    /// 取写句柄。
    fn writer(&mut self) -> Result<&mut dyn Write, Error>;
    /// 取走**可独立持有**的写端（阶段 4H t3：供 app 侧独立写线程独占使用）。
    ///
    /// 默认返回 `None`：调用方沿用 `writer()` 的同步写路径（Windows ConPTY 走此
    /// 默认实现，行为不变）。取走后 `writer()` 不再可用，二者不应混用。
    fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        None
    }
    /// Unix 主端原始 fd：用于非阻塞写与 Ctrl-C 丢弃已排队输入。Windows 返回 None。
    fn control_fd(&self) -> Option<std::os::raw::c_int> {
        None
    }
    /// Ctrl-C 且写端阻塞时，中断前台进程组；默认无操作。
    fn interrupt(&self) {}
    /// 调整 PTY 行列。
    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error>;
    /// 非阻塞查询子进程退出状态。
    fn try_wait(&mut self) -> Result<Option<ExitStatus>, Error>;
    /// 释放 PTY 主端并回收子进程（顺序见模块文档）。
    fn close(&mut self) -> Result<(), Error>;
}

/// 等待 fd 可读或可写；timeout_ms < 0 表示无限等待。仅 Unix。
#[cfg(unix)]
pub fn wait_fd(
    fd: std::os::raw::c_int,
    events: std::os::raw::c_short,
    timeout_ms: std::os::raw::c_int,
) -> std::io::Result<()> {
    let mut pfd = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    loop {
        // SAFETY: pfd is valid for the duration of poll; timeout is a plain c_int.
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        return Ok(());
    }
}

/// 等待 fd 可写；仅 Unix。
#[cfg(unix)]
pub fn wait_writable(
    fd: std::os::raw::c_int,
    timeout_ms: std::os::raw::c_int,
) -> std::io::Result<()> {
    wait_fd(fd, libc::POLLOUT, timeout_ms)
}

/// 丢弃 PTY 已排队但尚未被子进程读取的输入。仅 Unix。
#[cfg(unix)]
pub fn flush_input(fd: std::os::raw::c_int) -> std::io::Result<()> {
    // SAFETY: fd is the live PTY master owned by NativePty.
    if unsafe { libc::tcflush(fd, libc::TCIFLUSH) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// pty crate 错误枚举。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O 错误。
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// portable-pty 经 anyhow 返回的错误。
    #[error("pty error: {0}")]
    PortablePty(#[from] anyhow::Error),
    /// 在未 spawn 的 PTY 上调用操作。
    #[error("pty not spawned")]
    NotSpawned,
    /// 在已 spawn 的 PTY 上再次 spawn。
    #[error("pty already spawned")]
    AlreadySpawned,
    /// 读线程 join 超过超时上限。
    #[error("reader thread join timeout")]
    ReaderJoinTimeout,
}
