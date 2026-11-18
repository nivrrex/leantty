//! Windows 实现（portable-pty ConPTY）。
//!
//! ## close 契约
//! 先释放 PTY 主端（drop writer 与 master），再 kill + wait 回收子进程；
//! 先等子进程退出会让主端 EOF 延迟（Windows 验证数据约 83s），故顺序不可颠倒。
//!
//! ## 读线程契约
//! 读线程 join 必须带超时、禁止无界 join；超时上限见 READER_JOIN_TIMEOUT。
//!
//! ## 线程安全
//! WindowsPty 可 Send（字段均为 Send），不可 Sync：writer 需 &mut 独占访问。
//! 同一实例的所有方法必须在同一线程调用（与 vt::VtTerminal 同栈使用）。
//!
//! ## UTF-8 代码页
//! 实测确定：cmd.exe 以 `/K chcp 65001`、powershell.exe 以
//! `-NoLogo -NoExit -Command chcp 65001` 启动时，会话代码页为 UTF-8，
//! 其后 `echo 你好` 的输入回显与网格断言均为宽字符。默认代码页 936 下
//! UTF-8 字节会按 GBK 解码产生乱码（已验证事实，W5 实录）。

use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::time::Duration;

use super::{Error, Pty};

/// SetConsoleCtrlHandler（kernel32，手工 FFI：pty crate 无 windows-sys 直接依赖，
/// 不引入新依赖）。HANDLER_ROUTINE = BOOL (WINAPI *)(DWORD)，可空。
type CtrlHandlerRoutine = Option<unsafe extern "system" fn(u32) -> i32>;
unsafe extern "system" {
    fn SetConsoleCtrlHandler(lp_handler: CtrlHandlerRoutine, b_add: i32) -> i32;
}

/// 清除本进程可能继承的"忽略控制台 Ctrl 事件"状态
/// （SetConsoleCtrlHandler(NULL, TRUE) 的效果经子进程继承假设）。
/// 假设来源：app 从 agent 宿主环境启动时可能带忽略标志，致 ping -t 等
/// 非 stdin 型子进程不响应控制台 Ctrl-C 事件（见 PHASE2A-WIN-ADD 实验 X）。
fn clear_console_ctrl_ignore() -> Result<(), Error> {
    map_ctrl_handler_result(unsafe { SetConsoleCtrlHandler(None, 0) })
}

/// FFI 返回值映射：0 = 失败（错误取自最后一次 OS 错误），非零 = 成功。
/// 分离为纯函数以便失败路径可测。
fn map_ctrl_handler_result(rc: i32) -> Result<(), Error> {
    if rc == 0 {
        Err(Error::Io(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

/// 读线程 join 超时上限。禁止无界 join。
pub const READER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// 部署自检：程序目录里"有 conpty.dll 但缺 OpenConsole.exe"。
///
/// 背景（阶段 3L/3M 实测）：两个文件必须放在 app.exe 同目录；只剩 conpty.dll 时
/// 进程仍会正常启动，但 pty 宿主会静默换成系统 `C:\Windows\system32\conhost.exe`
/// ——折行会被物化、`ESC[7m` 被改写，用户看不到任何提示。这里只做**默认可见**
/// 的 stderr 警告，不改变任何行为。
pub fn conpty_pair_incomplete(dir: &std::path::Path) -> bool {
    dir.join("conpty.dll").exists() && !dir.join("OpenConsole.exe").exists()
}

/// 在 spawn 前做一次部署自检并打印警告（警告文本固定，便于脚本断言）。
fn warn_if_conpty_pair_incomplete() {
    let dir = match std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        Some(d) => d,
        None => return,
    };
    if conpty_pair_incomplete(&dir) {
        eprintln!(
            "[pty] 警告：程序目录存在 conpty.dll 但缺少 OpenConsole.exe——两文件必须成对部署；本次将改用系统 conhost（折行会被物化、ESC[7m 会被改写）。"
        );
    }
}

/// portable-pty 的 Windows (ConPTY) 实现。
pub struct WindowsPty {
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    writer: Option<Box<dyn Write + Send>>,
}

impl WindowsPty {
    /// 新建未 spawn 的实例。
    pub fn new() -> Self {
        Self {
            master: None,
            child: None,
            writer: None,
        }
    }

    /// shell 文件名 → 启动参数（把会话代码页置为 UTF-8；仅 cmd/powershell 有做法）。
    fn utf8_codepage_args(shell: &str) -> Vec<&'static str> {
        let base = shell.rsplit(['\\', '/']).next().unwrap_or(shell);
        if base.eq_ignore_ascii_case("cmd.exe") {
            vec!["/K", "chcp 65001"]
        } else if base.eq_ignore_ascii_case("powershell.exe") {
            vec!["-NoLogo", "-NoExit", "-Command", "chcp 65001"]
        } else {
            Vec::new()
        }
    }

    /// 用户配置的启动参数优先；未配置（`None`）时沿用既有默认（`utf8_codepage_args`）。
    /// `Some(vec![])` 表示显式空数组 —— 不注入任何默认参数（阶段 3U t2e）。
    pub fn decide_shell_args(program: &str, configured: Option<&[String]>) -> Vec<String> {
        match configured {
            Some(a) => a.to_vec(),
            None => Self::utf8_codepage_args(program)
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    /// 带 `terminal.shell.args` 的 spawn 实现（`Pty::spawn_with_args` 的 Windows 版本）。
    fn spawn_inner(
        &mut self,
        shell: &str,
        configured: Option<&[String]>,
        cols: u16,
        rows: u16,
    ) -> Result<(), Error> {
        if self.master.is_some() {
            return Err(Error::AlreadySpawned);
        }
        // 默认启动 cmd.exe（任务契约）；调用方传空时回落 cmd.exe。
        let shell = if shell.trim().is_empty() {
            "cmd.exe"
        } else {
            shell
        };
        if cols == 0 || rows == 0 {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cols and rows must be > 0",
            )));
        }
        // 实验 X：创建子进程前清除本进程可能继承的"忽略 Ctrl 事件"状态；
        // 失败路径返回 Error（不 unwrap、不忽略）。
        clear_console_ctrl_ignore()?;
        // 部署自检（3O）：只打警告，不改变启动路径
        warn_if_conpty_pair_incomplete();
        let system = native_pty_system();
        let PtyPair { master, slave } = system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let program = Self::resolve_program(shell);
        let mut cmd = CommandBuilder::new(program);
        for a in Self::decide_shell_args(shell, configured) {
            cmd.arg(a);
        }
        let child = slave.spawn_command(cmd)?;
        drop(slave);
        let writer = master.take_writer()?;
        self.master = Some(master);
        self.child = Some(child);
        self.writer = Some(writer);
        Ok(())
    }
}

impl Default for WindowsPty {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsPty {
    /// 子进程 PID（Ctrl-C 等经 Win32 控制台事件路径时需要；trait 无此方法，
    /// 故以固有方法暴露，不改 Pty 规格）。
    pub fn child_pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.process_id())
    }

    /// 把 shell 名解析为绝对路径。
    ///
    /// 已验证事实：app 由 bash+nohup 直接启动时（PATH 为 POSIX 形态），
    /// portable-pty 的 search_path 解析失败，CreateProcessW 收到拼接 cwd 的
    /// 错误路径（如 <dev-root>\cmd.exe）而 exit code 2。此处在 pty 层自行
    /// 解析，不改 Pty trait、不动上游。解析顺序：已是绝对路径且存在 → 原样；
    /// 否则按 PATH（含 POSIX 形态条目的 Windows 转换）+ PATHEXT 找文件。
    pub fn resolve_program(shell: &str) -> String {
        let path = std::path::Path::new(shell);
        if path.is_absolute() && path.is_file() {
            return shell.to_owned();
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(shell);
        let has_ext = name.contains('.');
        let mut exts: Vec<String> = Vec::new();
        if has_ext {
            exts.push(String::new());
        } else {
            let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE".to_string());
            for e in pathext.split(';') {
                let e = e.trim();
                if !e.is_empty() {
                    exts.push(e.to_ascii_lowercase());
                }
            }
        }
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let mut dirs = vec![dir.clone()];
                if let Some(converted) = Self::posix_dir_to_win(&dir) {
                    dirs.push(converted);
                }
                for d in &dirs {
                    for ext in &exts {
                        let mut candidate = d.clone();
                        if ext.is_empty() {
                            candidate.push(name);
                        } else {
                            candidate.push(format!("{}{}", name, ext));
                        }
                        if candidate.is_file() {
                            return candidate.to_string_lossy().into_owned();
                        }
                    }
                }
            }
        }
        shell.to_owned()
    }

    /// POSIX 形态条目（/c/Windows/System32）转 Windows 形态（C:\Windows\System32）；
    /// 非该形态返回 None。外部输入仅在此转换，不做其它假设。
    fn posix_dir_to_win(dir: &std::path::Path) -> Option<std::path::PathBuf> {
        let s = dir.to_str()?;
        let b = s.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
            let drive = (b[1] as char).to_ascii_uppercase();
            let rest = &s[3..];
            Some(std::path::PathBuf::from(format!(
                "{}:\\{}",
                drive,
                rest.replace('/', "\\")
            )))
        } else {
            None
        }
    }
}

impl Pty for WindowsPty {
    fn spawn(&mut self, shell: &str, cols: u16, rows: u16) -> Result<(), Error> {
        self.spawn_inner(shell, None, cols, rows)
    }

    /// 阶段 3U：`terminal.shell.args` 接线入口（用户配置优先，未配置走默认编码页参数）。
    fn spawn_with_args(
        &mut self,
        shell: &str,
        args: Option<&[String]>,
        cols: u16,
        rows: u16,
    ) -> Result<(), Error> {
        self.spawn_inner(shell, args, cols, rows)
    }

    fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, Error> {
        let master = self.master.as_ref().ok_or(Error::NotSpawned)?;
        Ok(master.try_clone_reader()?)
    }

    fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        // 与 unix 语义一致：把 ConPTY 输入管道的写端移交给调用方（app 的写线程），
        // 之后 writer() 返回 NotSpawned。spawn 时已从 master 取出该写端，
        // 因此不需要再从 master 拿一次（避免两个写端并发写同一管道）。
        self.writer.take()
    }

    fn writer(&mut self) -> Result<&mut dyn Write, Error> {
        self.writer
            .as_mut()
            .map(|w| w.as_mut() as &mut dyn Write)
            .ok_or(Error::NotSpawned)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        if cols == 0 || rows == 0 {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cols and rows must be > 0",
            )));
        }
        let master = self.master.as_ref().ok_or(Error::NotSpawned)?;
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    fn try_wait(&mut self) -> Result<Option<portable_pty::ExitStatus>, Error> {
        match self.child.as_mut() {
            Some(child) => Ok(child.try_wait()?),
            None => Err(Error::NotSpawned),
        }
    }

    /// 契约：先释放主端（writer 与 master），再 kill + wait 回收子进程。
    fn close(&mut self) -> Result<(), Error> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 实验 X 测试 1：spawn 前调用的返回值——本机实测调用（无忽略状态时移除
    /// 操作幂等，返回成功）。
    #[test]
    fn clear_console_ctrl_ignore_returns_ok() {
        assert!(clear_console_ctrl_ignore().is_ok());
    }

    /// 实验 X 测试 2：失败路径——FFI 返回 0 映射为 Err（非零为 Ok）；
    /// 覆盖 spawn 的 `?` 提前返回分支所依赖的映射。
    #[test]
    fn ctrl_handler_zero_maps_to_error() {
        assert!(map_ctrl_handler_result(0).is_err());
        assert!(map_ctrl_handler_result(1).is_ok());
    }

    /// 阶段 3U：`terminal.shell.args` 决策表（用户配置优先；未配置走默认编码页参数）。
    #[test]
    fn decide_shell_args_table() {
        let none: Option<&[String]> = None;
        assert_eq!(
            WindowsPty::decide_shell_args("cmd.exe", none),
            vec!["/K".to_string(), "chcp 65001".to_string()]
        );
        assert_eq!(
            WindowsPty::decide_shell_args("CMD.EXE", none),
            vec!["/K".to_string(), "chcp 65001".to_string()]
        );
        assert_eq!(
            WindowsPty::decide_shell_args(r"C:\Windows\System32\cmd.exe", none),
            vec!["/K".to_string(), "chcp 65001".to_string()]
        );
        assert_eq!(
            WindowsPty::decide_shell_args("powershell.exe", none),
            vec![
                "-NoLogo".to_string(),
                "-NoExit".to_string(),
                "-Command".to_string(),
                "chcp 65001".to_string()
            ]
        );
        assert!(WindowsPty::decide_shell_args("pwsh.exe", none).is_empty());
        let empty: Vec<String> = Vec::new();
        assert!(WindowsPty::decide_shell_args("cmd.exe", Some(&empty)).is_empty());
        let user = vec!["/D".to_string(), "/K".to_string(), "echo X".to_string()];
        assert_eq!(WindowsPty::decide_shell_args("cmd.exe", Some(&user)), user);
    }
}
