//! 配置调色板到达 VT 层（阶段 4F t2）。
//!
//! 绝对正确性测试：用一份各色互不相同的调色板初始化终端，喂入 ANSI 索引色与反色
//! 序列，断言快照里的 cell 颜色等于**配置色**；同时断言 256 色立方/灰阶（16..255）
//! 与 24 位真彩色不受配置影响。
//!
//! 修复前必须为红：`VtTerminal::new` 不接受调色板，libghostty 用内置 16 色解析
//! ANSI 索引，renderer 的配置色只在 VT 未给色时才兜底 → ESC[31m 得到 #CC6666
//! 而不是配置的 #d32f2f。

use vt::{Rgb, VtTerminal};

/// 一份"每项都不相同"的调色板（便于定位是哪一项没生效）。
fn test_palette() -> vt::VtPalette {
    let c = |r: u8, g: u8, b: u8| Rgb { r, g, b };
    vt::VtPalette {
        foreground: c(0xE1, 0xE2, 0xE3),
        background: c(0x10, 0x11, 0x12),
        cursor: c(0x20, 0x21, 0x22),
        normal: [
            c(0x01, 0x02, 0x03), // 0 black
            c(0x11, 0x12, 0x13), // 1 red
            c(0x21, 0x22, 0x23), // 2 green
            c(0x31, 0x32, 0x33), // 3 yellow
            c(0x41, 0x42, 0x43), // 4 blue
            c(0x51, 0x52, 0x53), // 5 magenta
            c(0x61, 0x62, 0x63), // 6 cyan
            c(0x71, 0x72, 0x73), // 7 white
        ],
        bright: [
            c(0x81, 0x82, 0x83), // 8
            c(0x91, 0x92, 0x93), // 9
            c(0xA1, 0xA2, 0xA3), // 10
            c(0xB1, 0xB2, 0xB3), // 11
            c(0xC1, 0xC2, 0xC3), // 12
            c(0xD1, 0xD2, 0xD3), // 13
            c(0xE4, 0xE5, 0xE6), // 14
            c(0xF1, 0xF2, 0xF3), // 15
        ],
    }
}

fn term_with_palette() -> VtTerminal {
    VtTerminal::new_with_palette(20, 3, &test_palette()).expect("VtTerminal::new_with_palette")
}

/// 读第 0 行第 `col` 格的前景色/背景色。
fn cell_colors(t: &VtTerminal, col: usize) -> (Option<Rgb>, Option<Rgb>) {
    let snap = t.snapshot().expect("snapshot");
    let c = &snap.cells[0][col];
    (c.fg, c.bg)
}

/// ESC[30m..37m → 索引 0..7（normal 调色板）
#[test]
fn ansi_basic_colors_follow_configured_palette() {
    let p = test_palette();
    let mut t = term_with_palette();
    t.feed(b"\x1b[2J\x1b[H");
    for i in 0..8u8 {
        t.feed(format!("\x1b[{}mX", 30 + i).as_bytes());
    }
    t.feed(b"\x1b[0m");
    for i in 0..8usize {
        let (fg, _) = cell_colors(&t, i);
        assert_eq!(
            fg,
            Some(p.normal[i]),
            "ESC[{}m 应使用配置的 normal[{}]（实测 {:?}）",
            30 + i,
            i,
            fg
        );
    }
}

/// ESC[90m..97m → 索引 8..15（bright 调色板）
#[test]
fn ansi_bright_colors_follow_configured_palette() {
    let p = test_palette();
    let mut t = term_with_palette();
    t.feed(b"\x1b[2J\x1b[H");
    for i in 0..8u8 {
        t.feed(format!("\x1b[{}mX", 90 + i).as_bytes());
    }
    t.feed(b"\x1b[0m");
    for i in 0..8usize {
        let (fg, _) = cell_colors(&t, i);
        assert_eq!(
            fg,
            Some(p.bright[i]),
            "ESC[{}m 应使用配置的 bright[{}]",
            90 + i,
            i
        );
    }
}

/// ESC[40m..47m / ESC[100m..107m → 背景色
#[test]
fn ansi_background_colors_follow_configured_palette() {
    let p = test_palette();
    let mut t = term_with_palette();
    t.feed(b"\x1b[2J\x1b[H");
    for i in 0..8u8 {
        t.feed(format!("\x1b[{}mX", 40 + i).as_bytes());
    }
    for i in 0..8u8 {
        t.feed(format!("\x1b[{}mX", 100 + i).as_bytes());
    }
    t.feed(b"\x1b[0m");
    for i in 0..8usize {
        let (_, bg) = cell_colors(&t, i);
        assert_eq!(
            bg,
            Some(p.normal[i]),
            "ESC[{}m 背景应为配置 normal[{}]",
            40 + i,
            i
        );
    }
    for i in 0..8usize {
        let (_, bg) = cell_colors(&t, 8 + i);
        assert_eq!(
            bg,
            Some(p.bright[i]),
            "ESC[{}m 背景应为配置 bright[{}]",
            100 + i,
            i
        );
    }
}

/// ESC[7m（默认色反色）：libghostty 用 `inverse` 标记 + `fg/bg=None` 表示
/// "用默认色反色"，由渲染层用配置的 primary fg/bg 交换上色。这里断言该表示成立
/// （像素级验证见 `crates/render/tests/palette_config_render.rs`）；
/// 若 VT 直接给显式色，则必须等于配置色（两种实现都接受，但不得是内置默认色）。
#[test]
fn default_inverse_uses_configured_or_marked_defaults() {
    let p = test_palette();
    let mut t = term_with_palette();
    t.feed(b"\x1b[2J\x1b[H\x1b[7mX");
    let snap = t.snapshot().expect("snapshot");
    let c = &snap.cells[0][0];
    assert!(c.inverse, "ESC[7m 应标记 inverse");
    match (c.fg, c.bg) {
        (None, None) => { /* 由渲染层交换配置色，符合当前 libghostty 表示 */ }
        (fg, bg) => {
            assert_eq!(fg, Some(p.background), "若给显式前景，应为配置 background");
            assert_eq!(bg, Some(p.foreground), "若给显式背景，应为配置 foreground");
        }
    }
}

/// OSC 10/11/4 动态改色后新输出用新色；OSC 104/110/111 重置后回到**配置色**
/// （不是 libghostty 内置默认色）。
#[test]
fn osc_overrides_then_resets_back_to_configured_palette() {
    let p = test_palette();
    let mut t = term_with_palette();
    t.feed(b"\x1b[2J\x1b[H");

    // OSC 4;1 改索引 1（红）→ ESC[31m 应使用新色
    t.feed(b"\x1b]4;1;rgb:aa/bb/cc\x1b\\");
    t.feed(b"\x1b[31mX");
    let (fg_override, _) = cell_colors(&t, 0);
    assert_eq!(
        fg_override,
        Some(Rgb {
            r: 0xaa,
            g: 0xbb,
            b: 0xcc
        }),
        "OSC 4 改色后 ESC[31m 应使用新色（实测 {:?}）",
        fg_override
    );

    // OSC 104（重置全部调色板）→ 回到配置色
    t.feed(b"\x1b]104\x1b\\");
    t.feed(b"\x1b[0m\x1b[31mX");
    let (fg_reset, _) = cell_colors(&t, 0);
    assert_eq!(
        fg_reset,
        Some(p.normal[1]),
        "OSC 104 重置后 ESC[31m 应回到配置色（实测 {:?}）",
        fg_reset
    );
}

/// 24 位真彩色与 256 色（16..255）不受配置调色板影响。
#[test]
fn truecolor_and_256_cube_are_not_affected() {
    let mut t = term_with_palette();
    t.feed(b"\x1b[2J\x1b[H");
    t.feed(b"\x1b[38;2;12;34;56mX"); // 真彩色
    t.feed(b"\x1b[38;5;16mX"); // 256 色索引 16（立方体起点）
    t.feed(b"\x1b[38;5;196mX"); // 256 色索引 196
    t.feed(b"\x1b[38;5;232mX"); // 灰阶起点
    t.feed(b"\x1b[0m");
    let snap = t.snapshot().expect("snapshot");
    let fg = |i: usize| snap.cells[0][i].fg;
    assert_eq!(
        fg(0),
        Some(Rgb {
            r: 12,
            g: 34,
            b: 56
        }),
        "真彩色应原样保留"
    );
    // 16..255 是标准 xterm 立方/灰阶：用内置默认的同一索引作为期望值
    let builtin = libghostty_vt::style::Palette::default();
    let builtin_rgb = |i: usize| {
        let c = builtin.0[i];
        Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    };
    assert_eq!(fg(1), Some(builtin_rgb(16)), "256 色索引 16 不应被配置覆盖");
    assert_eq!(
        fg(2),
        Some(builtin_rgb(196)),
        "256 色索引 196 不应被配置覆盖"
    );
    assert_eq!(
        fg(3),
        Some(builtin_rgb(232)),
        "256 色索引 232 不应被配置覆盖"
    );
}
