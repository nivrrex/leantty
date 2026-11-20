//! 阶段 9E：`Alt+Tab` 在编码器层到底产出什么字节（app 侧"不转发"规则的依据）。
//!
//! 背景（已验证事实，用户反馈）：`Alt+Tab` 切窗口后终端里留下一个 Tab。
//! 若该按键真的到达客户端，`route_key` 会把 `(Key::Tab, Mods::ALT)` 交给上游编码器，
//! 得到 `ESC` + `0x09`——shell/readline 收到 `ESC Tab` 会触发一次补全，看起来就是
//! "遗留了一个 Tab"。因此 app 侧必须在写 PTY 之前把它挡掉（见 `wm_reserved_key`）。

use vt::{Action, Key, KeyEncoder, Mods};

#[test]
fn alt_tab_encodes_to_esc_tab() {
    let mut enc = KeyEncoder::new().unwrap();
    let bytes = enc
        .encode_key_full(Key::Tab, Mods::ALT, Action::Press, None, None)
        .unwrap();
    assert_eq!(bytes, vec![0x1b, b'\t'], "Alt+Tab 会被编码成 ESC + Tab");
}

#[test]
fn plain_tab_is_a_single_byte() {
    let mut enc = KeyEncoder::new().unwrap();
    let bytes = enc
        .encode_key_full(Key::Tab, Mods::empty(), Action::Press, None, None)
        .unwrap();
    assert_eq!(bytes, vec![b'\t'], "无修饰 Tab 仍应正常转发");
}
