//! 阶段 9F：`Ctrl+Space` / `Ctrl+Shift+<字母或符号>` 的编码口径。
//!
//! 依据（与 Zig 侧的 alacritty 对拍）：alacritty 在 `Ctrl+Space` 发 `0x00`、
//! `Ctrl+Shift+A` 发 `0x01`、`Ctrl+Shift+[` 发 `0x1b`。
//!
//! 实测结论（本文件锁定上游行为）：把按键文本交给编码器后，
//! - `Ctrl+Space` 直接得到 `0x00`；
//! - `Ctrl+Shift+<字符>` 上游一律产出 `CSI u`（`ESC[97;6u`），**传统模式**下需要由 app
//!   按 xterm 口径回写控制字节（见 `crates/app/src/lib.rs::ctrl_byte` 与 `is_csi_u`）。

use vt::{Action, Key, KeyEncoder, Mods, VtTerminal};

fn encode(key: Key, mods: Mods, unshifted: Option<char>, utf8: Option<&str>) -> Vec<u8> {
    let term = VtTerminal::new(80, 24).expect("terminal");
    let mut enc = KeyEncoder::new().unwrap();
    enc.set_options_from_terminal(&term).expect("options");
    enc.encode_key_full(key, mods, Action::Press, unshifted, utf8)
        .unwrap()
}

#[test]
fn ctrl_space_sends_nul() {
    // 不带文本时上游也给 0x00；带文本同样。
    assert_eq!(encode(Key::Space, Mods::CTRL, Some(' '), Some(" ")), vec![0x00]);
    assert_eq!(encode(Key::Space, Mods::CTRL, None, None), vec![0x00]);
}

#[test]
fn ctrl_shift_letter_is_csi_u_upstream() {
    // 上游口径：传统模式也走 CSI u；app 侧据此判断是否需要回写控制字节。
    assert_eq!(
        encode(Key::A, Mods::CTRL | Mods::SHIFT, Some('a'), Some("A")),
        b"\x1b[97;6u".to_vec()
    );
}

#[test]
fn ctrl_shift_bracket_is_csi_u_upstream() {
    assert_eq!(
        encode(Key::BracketLeft, Mods::CTRL | Mods::SHIFT, Some('['), Some("{")),
        b"\x1b[123;5u".to_vec()
    );
}
