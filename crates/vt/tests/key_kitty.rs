//! 阶段 6A 回归：Kitty 键盘协议下 Ctrl+字母必须编码成 CSI-u。
//!
//! 背景（已验证事实）：`vendor/ghostty-src/src/input/key_encode.zig` 的 `kitty()` 对非功能键
//! 依赖 `unshifted_codepoint`；为 0 时找不到 kitty 表项直接 `return`（不输出任何字节）。
//! app 侧此前不带码点 → kitty 模式下 Ctrl+D/C/P 全部丢失（pi 的快捷键因此失效）。

use vt::{Action, Key, Mods, VtTerminal};

/// 建立终端并喂入模式序列，返回按其模式同步好的编码器。
fn encoder_with(init: &[u8]) -> vt::KeyEncoder {
    let mut term = VtTerminal::new(80, 24).expect("term");
    if !init.is_empty() {
        term.feed(init);
    }
    term.key_encoder().expect("encoder")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 表驱动：模式 × 键 → 实收字节（不带码点 vs 带码点）。
#[test]
fn ctrl_letter_encoding_by_mode() {
    // (模式名, 模式序列)
    let modes: &[(&str, &[u8])] = &[
        ("legacy", b""),
        ("kitty1", b"\x1b[>1u"),
        ("kitty7", b"\x1b[>7u"),
        ("mok2", b"\x1b[>4;2m"),
    ];
    // (键, 该键在 kitty 协议里的 unshifted codepoint, legacy 期望, kitty 期望)
    let keys: &[(Key, char, &str, &str)] = &[
        (Key::A, 'a', "01", "1b5b39373b3575"),   // ESC[97;5u
        (Key::C, 'c', "03", "1b5b39393b3575"),   // ESC[99;5u
        (Key::D, 'd', "04", "1b5b3130303b3575"), // ESC[100;5u
        (Key::P, 'p', "10", "1b5b3131323b3575"), // ESC[112;5u
    ];
    for (mode, init) in modes {
        for (key, cp, legacy_hex, kitty_hex) in keys {
            let mut enc = encoder_with(init);
            let with_cp = enc
                .encode_key_full(*key, Mods::CTRL, Action::Press, Some(*cp), None)
                .expect("encode");
            let without = enc
                .encode_key(*key, Mods::CTRL, Action::Press)
                .expect("encode");
            let expect = if *mode == "legacy" || *mode == "mok2" {
                *legacy_hex
            } else {
                *kitty_hex
            };
            assert_eq!(
                hex(&with_cp),
                expect,
                "{mode}: 带 unshifted codepoint 时 Ctrl+{cp} 的编码应为 {expect}"
            );
            if *mode == "kitty1" || *mode == "kitty7" {
                assert!(
                    without.is_empty(),
                    "{mode}: 不带码点时上游编码器应静默丢弃（这正是阶段 6A 的根因），实测 {}",
                    hex(&without)
                );
            }
        }
    }
}

/// 功能键不受影响（kitty 模式下 Up 走 kitty 表格，带事件类型）。
#[test]
fn functional_keys_stay_encoded() {
    let mut enc = encoder_with(b"\x1b[>7u");
    let up = enc
        .encode_key(Key::ArrowUp, Mods::empty(), Action::Press)
        .expect("encode");
    assert_eq!(
        hex(&up),
        "1b5b313b313a3141",
        "kitty 模式下 Up 应为 ESC[1;1:1A"
    );
    let mut enc = encoder_with(b"");
    let up = enc
        .encode_key(Key::ArrowUp, Mods::empty(), Action::Press)
        .expect("encode");
    assert_eq!(hex(&up), "1b5b41", "legacy 模式下 Up 应为 ESC[A");
}
