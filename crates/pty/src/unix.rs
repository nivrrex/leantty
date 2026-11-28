//! Unix 实现（portable-pty）。
//!
//! close 顺序：先释放 PTY 主端（drop writer 与 master），再 kill + wait
//! 回收子进程；遵守模块级契约，避免先等子进程退出导致主端 EOF 延迟。

use portable_pty::{
    Child, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize, native_pty_system,
};
use std::io::{Read, Write};
use std::os::raw::c_int;
use std::time::Duration;

use super::{Error, Pty};

/// 读线程 join 超时上限。禁止无界 join。
pub const READER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// 非阻塞 PTY 读端：WouldBlock 时用 poll 等待，避免读线程把正常无数据误报为故障。
struct UnixReader {
    inner: Box<dyn Read + Send>,
    fd: c_int,
}

impl Read for UnixReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match self.inner.read(buf) {
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    super::wait_fd(self.fd, libc::POLLIN, -1)?;
                }
                other => return other,
            }
        }
    }
}

fn set_nonblocking(fd: c_int) -> std::io::Result<()> {
    // SAFETY: fd is the live PTY master returned by portable-pty.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: same live fd; O_NONBLOCK only changes file status flags.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

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
        // 阶段 9H：子进程必须有可用的 TERM。桌面启动（lxpanel/openbox）时环境里没有 TERM，
        // bash 会按 "dumb" 处理：收到 SIGWINCH 后它用空格重画提示符，把第一行擦成空白，
        // 于是看起来"首行多了一个空行"（用户实测：命令行启动无此现象，桌面启动有）。
        // 其它终端（xterm/alacritty）都会给子进程设 TERM，这里同样兜底。
        let term_missing = std::env::var_os("TERM")
            .and_then(|v| v.into_string().ok())
            .map(|v| v.is_empty() || v == "dumb")
            .unwrap_or(true);
        if term_missing {
            cmd.env("TERM", "xterm-256color");
        }
        let child = slave.spawn_command(cmd)?;
        drop(slave);
        let fd = master.as_raw_fd().ok_or(Error::NotSpawned)?;
        set_nonblocking(fd)?;
        let writer = master.take_writer()?;
        self.master = Some(master);
        self.child = Some(child);
        self.writer = Some(writer);
        Ok(())
    }

    fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, Error> {
        let master = self.master.as_ref().ok_or(Error::NotSpawned)?;
        let fd = master.as_raw_fd().ok_or(Error::NotSpawned)?;
        let reader = master.try_clone_reader()?;
        Ok(Box::new(UnixReader { inner: reader, fd }))
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

    fn control_fd(&self) -> Option<c_int> {
        self.master.as_ref()?.as_raw_fd()
    }

    fn interrupt(&self) {
        let Some(master) = self.master.as_ref() else {
            return;
        };
        let Some(pgid) = master.process_group_leader() else {
            return;
        };
        // SAFETY: pgid is the foreground process group of this PTY; negative pid targets the group.
        unsafe {
            libc::kill(-pgid, libc::SIGINT);
        }
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
