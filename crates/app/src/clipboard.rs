//! 剪贴板抽象（任务 2.4）：Windows 共用此 trait；Linux 侧 arboard 实现。
//!
//! 线程安全：Clipboard 实现**非 Send**（arboard X11 连接在主线程创建，且 X11
//! 所有权语义要求 set 后持有者存活至进程退出）——所有方法仅限主线程调用；
//! trait 不加 Send 约束，调用方（app 事件循环）保证单线程。

#[cfg(unix)]
use arboard::{GetExtLinux, SetExtLinux};
use crate::Error;

/// 剪贴板种类（X11 CLIPBOARD / PRIMARY）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardKind {
    Clipboard,
    Primary,
}

/// 剪贴板 trait（arboard 与测试注入共用接口；Windows 侧同此 trait）。
pub trait Clipboard {
    /// 读剪贴板文本；无内容/未持有返 Err（调用方按需上报）。
    fn get(&mut self, kind: ClipboardKind) -> Result<String, Error>;
    /// 写剪贴板文本。X11 所有权语义：set 后持有者存活至进程退出，内容不丢。
    fn set(&mut self, kind: ClipboardKind, text: &str) -> Result<(), Error>;
}

/// arboard 实现（Linux X11 含 PRIMARY+CLIPBOARD；Windows 同接口）。
pub struct ArboardClipboard {
    inner: arboard::Clipboard,
}

impl ArboardClipboard {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: arboard::Clipboard::new()
                .map_err(|e| Error::Clipboard(format!("arboard: {e}")))?,
        })
    }
}

impl Clipboard for ArboardClipboard {
    fn get(&mut self, kind: ClipboardKind) -> Result<String, Error> {
        match kind {
            ClipboardKind::Clipboard => self
                .inner
                .get_text()
                .map_err(|e| Error::Clipboard(format!("get clipboard: {e}"))),
            ClipboardKind::Primary => primary_get(&self.inner),
        }
    }

    fn set(&mut self, kind: ClipboardKind, text: &str) -> Result<(), Error> {
        match kind {
            ClipboardKind::Clipboard => self
                .inner
                .set_text(text)
                .map_err(|e| Error::Clipboard(format!("set clipboard: {e}"))),
            ClipboardKind::Primary => primary_set(&self.inner, text),
        }
    }
}

/// arboard X11 PRIMARY 扩展（仅 unix：arboard 的 Linux 扩展 trait，
/// Windows 后端无 PRIMARY 概念）。
#[cfg(unix)]
fn primary_get(clip: &arboard::Clipboard) -> Result<String, Error> {
    clip.get()
        .clipboard(arboard::LinuxClipboardKind::Primary)
        .text()
        .map_err(|e| Error::Clipboard(format!("get primary: {e}")))
}

/// Windows 无 PRIMARY（arboard Windows 后端仅 CLIPBOARD）：读返 Err，
/// 实际行为（Copy 仅 save_to_clipboard 时落 CLIPBOARD、PasteSelection 为
/// no-op+日志、不回落）由调用方实测记录，见 PHASE2B-WIN.md 与 WIN-NOTES。
#[cfg(not(unix))]
fn primary_get(_clip: &arboard::Clipboard) -> Result<String, Error> {
    Err(Error::Clipboard(
        "primary selection unavailable: Windows 无 PRIMARY（arboard Windows 仅 CLIPBOARD）".to_string(),
    ))
}

/// arboard X11 PRIMARY 扩展（仅 unix）。
#[cfg(unix)]
fn primary_set(clip: &arboard::Clipboard, text: &str) -> Result<(), Error> {
    clip.set()
        .clipboard(arboard::LinuxClipboardKind::Primary)
        .text(text)
        .map_err(|e| Error::Clipboard(format!("set primary: {e}")))
}

/// Windows 无 PRIMARY：写返 Err（行为记录同上）。
#[cfg(not(unix))]
fn primary_set(_clip: &arboard::Clipboard, _text: &str) -> Result<(), Error> {
    Err(Error::Clipboard(
        "primary selection unavailable: Windows 无 PRIMARY（arboard Windows 仅 CLIPBOARD）".to_string(),
    ))
}

/// 测试注入实现（内存存储；Send 不要求——同主线程）。
pub struct InjectClipboard {
    store: std::sync::Mutex<std::collections::HashMap<ClipboardKind, String>>,
}

impl InjectClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for InjectClipboard {
    fn default() -> Self {
        Self {
            store: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Clipboard for InjectClipboard {
    fn get(&mut self, kind: ClipboardKind) -> Result<String, Error> {
        self.store
            .lock()
            .map_err(|e| Error::Clipboard(format!("inject lock: {e}")))?
            .get(&kind)
            .cloned()
            .ok_or_else(|| Error::Clipboard("inject: kind 为空".to_string()))
    }

    fn set(&mut self, kind: ClipboardKind, text: &str) -> Result<(), Error> {
        self.store
            .lock()
            .map_err(|e| Error::Clipboard(format!("inject lock: {e}")))?
            .insert(kind, text.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_get_set_roundtrip() {
        let mut c = InjectClipboard::new();
        assert!(c.get(ClipboardKind::Clipboard).is_err(), "未 set 前 get 应 Err");
        c.set(ClipboardKind::Clipboard, "a").unwrap();
        c.set(ClipboardKind::Primary, "b").unwrap();
        assert_eq!(c.get(ClipboardKind::Clipboard).unwrap(), "a");
        assert_eq!(c.get(ClipboardKind::Primary).unwrap(), "b");
    }
}
