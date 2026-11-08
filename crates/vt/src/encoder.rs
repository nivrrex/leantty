//! KeyEncoder / MouseEncoder 封装。

use libghostty_vt::key::{Action, Event as KeyEvent, Key, Mods};
use libghostty_vt::mouse::{self as vt_mouse, Event as MouseEvent};

use crate::error::Error;
use crate::terminal::VtTerminal;

/// 键盘编码器封装。
pub struct KeyEncoder {
    inner: libghostty_vt::key::Encoder<'static>,
}

impl KeyEncoder {
    /// 新建编码器。
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: libghostty_vt::key::Encoder::new()?,
        })
    }

    /// 选项取自终端当前模式。
    pub fn set_options_from_terminal(&mut self, term: &VtTerminal) -> Result<(), Error> {
        self.inner.set_options_from_terminal(term.inner());
        Ok(())
    }

    /// 设置应用光标键模式。
    pub fn set_cursor_key_application(&mut self, value: bool) -> &mut Self {
        self.inner.set_cursor_key_application(value);
        self
    }

    /// 编码单个按键事件为字节序列。
    pub fn encode_key(&mut self, key: Key, mods: Mods, action: Action) -> Result<Vec<u8>, Error> {
        let mut ev = KeyEvent::new()?;
        ev.set_action(action);
        ev.set_key(key);
        ev.set_mods(mods);
        let mut out = Vec::new();
        self.inner.encode_to_vec(&ev, &mut out)?;
        Ok(out)
    }
}

impl Default for KeyEncoder {
    fn default() -> Self {
        Self::new().expect("KeyEncoder::new")
    }
}

/// 鼠标编码器封装。
pub struct MouseEncoder {
    inner: vt_mouse::Encoder<'static>,
}

impl MouseEncoder {
    /// 新建编码器。
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: vt_mouse::Encoder::new()?,
        })
    }

    /// 选项取自终端当前模式。
    pub fn set_options_from_terminal(&mut self, term: &VtTerminal) -> Result<(), Error> {
        self.inner.set_options_from_terminal(term.inner());
        Ok(())
    }

    /// 编码单个鼠标事件为字节序列。
    pub fn encode_button(
        &mut self,
        action: vt_mouse::Action,
        button: Option<vt_mouse::Button>,
        mods: Mods,
        pos: vt_mouse::Position,
    ) -> Result<Vec<u8>, Error> {
        let mut ev = MouseEvent::new()?;
        ev.set_action(action);
        ev.set_button(button);
        ev.set_mods(mods);
        ev.set_position(pos);
        let mut out = Vec::new();
        self.inner.encode_to_vec(&ev, &mut out)?;
        Ok(out)
    }
}

impl Default for MouseEncoder {
    fn default() -> Self {
        Self::new().expect("MouseEncoder::new")
    }
}
