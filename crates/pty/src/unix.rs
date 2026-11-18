//! Unix 实现（portable-pty）。
//!
//! close 顺序：先释放 PTY 主端（drop writer 与 master），再 kill + wait
//! 回收子进程；遵守模块级契约，避免先等子进程退出导致主端 EOF 延迟。

use portable_pty::{
    Child, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize, native_pty_system,
};
use std::io::{Read, Write};
use std::time::Duration;

use super::{Error, Pty};

/// 读线程 join 超时上限。禁止无界 join。
pub const READER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// portable-pty 的 Unix 实现。
pub struct NativePty {
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Option<Box<dyn Write + Send>>,
}

impl NativePty {
    /// 新建未 spawn 的实例。
    pub fn new() -> Self {
        Self {
            master: None,
            child: None,
            writer: None,
        }
    }
}

impl Default for NativePty {
    fn default() -> Self {
        Self::new()
    }
}

impl Pty for NativePty {
    fn spawn(&mut self, shell: &str, cols: u16, rows: u16) -> Result<(), Error> {
        // 阶段 4X t2：等价于"未配置 args"。既有语义与字节序列均不变。
        self.spawn_with_args(shell, None, cols, rows)
    }

    fn spawn_with_args(
        &mut self,
        shell: &str,
        args: Option<&[String]>,
        cols: u16,
        rows: u16,
    ) -> Result<(), Error> {
        if self.master.is_some() {
            return Err(Error::AlreadySpawned);
        }
        let system = native_pty_system();
        let PtyPair { master, slave } = system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut cmd = CommandBuilder::new(shell);
        // 注意：portable-pty 0.8.1 的 `as_command()` 在 `cwd` 为 None 时会把子进程工作目录
        // 设为 **$HOME**（`cmdbuilder.rs:452-458` 的 `unwrap_or(home)`），不是继承父进程。
        // 因此这里显式传入本进程当前目录：未指定 `--working-directory` 时即启动目录，
        // 指定时 app 已 `set_current_dir`，子进程随之继承。
        if let Ok(dir) = std::env::current_dir() {
            cmd.cwd(dir);
        }
        // 阶段 4X t2：用户显式配置的 `terminal.shell.args` 逐项透传（`None` = 未配置，
        // 与 4W 之前的行为逐字节一致；Unix 侧不注入任何默认参数）。
        if let Some(list) = args {
            for a in list {
                cmd.arg(a);
            }
        }
        let child = slave.spawn_command(cmd)?;
        drop(slave);
        let writer = master.take_writer()?;
        self.master = Some(master);
        self.child = Some(child);
        self.writer = Some(writer);
        Ok(())
    }

    fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, Error> {
        let master = self.master.as_ref().ok_or(Error::NotSpawned)?;
        Ok(master.try_clone_reader()?)
    }

    fn writer(&mut self) -> Result<&mut dyn Write, Error> {
        self.writer
            .as_mut()
            .map(|w| w.as_mut() as &mut dyn Write)
            .ok_or(Error::NotSpawned)
    }

    fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        // 写端由 NativePty 持有（spawn 时 take_writer 自 master 取出），
        // 取走后 self.writer 为 None，writer() 将返回 NotSpawned。
        self.writer.take()
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        let master = self.master.as_ref().ok_or(Error::NotSpawned)?;
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, Error> {
        match self.child.as_mut() {
            Some(child) => Ok(child.try_wait()?),
            None => Err(Error::NotSpawned),
        }
    }

    fn close(&mut self) -> Result<(), Error> {
        // 契约：先释放 PTY 主端，再回收子进程。
        self.writer = None;
        self.master = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

/// 带超时的线程 join（经 channel 实现，规避 JoinHandle 无超时 join）。
pub fn join_thread_timeout(
    handle: std::thread::JoinHandle<()>,
    timeout: Duration,
) -> Result<(), Error> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    rx.recv_timeout(timeout)
        .map_err(|_| Error::ReaderJoinTimeout)?;
    Ok(())
}
