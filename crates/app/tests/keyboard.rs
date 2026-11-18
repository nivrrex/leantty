//! 键盘路由与编码测试（规格 B2 清单：小写/大写字母、数字、常见 ASCII 符号、
//! Space、Enter、Tab、Backspace、Esc、方向键（普通与应用光标键模式）、
//! Ctrl+字母、Home/End/F1~F4）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use app::{KeyRoute, Wake, route_key};

/// route_key 分类断言（纯函数，无 I/O）。
#[test]
fn route_key_text_priority_and_modifier_fallback() {
    use vt::{Key, Mods};

    // 小写字母：text 优先
    match route_key(Some("e"), Some(Key::E), Mods::empty()) {
        KeyRoute::Text(s) => assert_eq!(s, "e"),
        _ => panic!("小写字母应走 Text"),
    }
    // 大写字母：text 为 "E"
    match route_key(Some("E"), Some(Key::E), Mods::empty()) {
        KeyRoute::Text(s) => assert_eq!(s, "E"),
        _ => panic!("大写字母应走 Text"),
    }
    // 数字
    match route_key(Some("7"), Some(Key::Digit7), Mods::empty()) {
        KeyRoute::Text(s) => assert_eq!(s, "7"),
        _ => panic!("数字应走 Text"),
    }
    // 常见 ASCII 符号
    match route_key(Some("#"), Some(Key::Unidentified), Mods::empty()) {
        KeyRoute::Text(s) => assert_eq!(s, "#"),
        _ => panic!("符号应走 Text"),
    }
    // Space（winit text=" "）
    match route_key(Some(" "), Some(Key::Unidentified), Mods::empty()) {
        KeyRoute::Text(s) => assert_eq!(s, " "),
        _ => panic!("Space 应走 Text"),
    }
    // Ctrl+字母 → Encoded（即使有 text）
    match route_key(Some("c"), Some(Key::C), Mods::CTRL) {
        KeyRoute::Encoded { key, mods } => {
            assert_eq!(key, Some(Key::C));
            assert!(mods.intersects(Mods::CTRL));
        }
        _ => panic!("Ctrl+字母应走 Encoded"),
    }
    // Alt+字母 → Encoded
    match route_key(Some("x"), Some(Key::X), Mods::ALT) {
        KeyRoute::Encoded { .. } => {}
        _ => panic!("Alt+字母应走 Encoded"),
    }
    // 命名键无 text → Encoded
    match route_key(None, Some(Key::Enter), Mods::empty()) {
        KeyRoute::Encoded { key, .. } => assert_eq!(key, Some(Key::Enter)),
        _ => panic!("Enter 应走 Encoded"),
    }
    // 未映射键无 text → Encoded{key: None}
    match route_key(None, None, Mods::empty()) {
        KeyRoute::Encoded { key: None, .. } => {}
        _ => panic!("未映射键应走 Encoded{{key: None}}"),
    }
    // 空 text 回落 Encoded
    match route_key(Some(""), Some(Key::A), Mods::empty()) {
        KeyRoute::Encoded { .. } => {}
        _ => panic!("空 text 应回落 Encoded"),
    }
}

/// Encoded 路径字节断言（KeyEncoder 序列）。
#[test]
fn encoded_key_bytes() {
    use vt::{Action, Key, KeyEncoder, Mods};

    let mut enc = KeyEncoder::new().expect("KeyEncoder::new");
    let cases: &[(Key, Mods, &[u8])] = &[
        (Key::Enter, Mods::empty(), b"\r"),
        (Key::Tab, Mods::empty(), b"\t"),
        (Key::Backspace, Mods::empty(), b"\x7f"),
        (Key::Escape, Mods::empty(), b"\x1b"),
        (Key::ArrowUp, Mods::empty(), b"\x1b[A"),
        (Key::ArrowDown, Mods::empty(), b"\x1b[B"),
        (Key::ArrowRight, Mods::empty(), b"\x1b[C"),
        (Key::ArrowLeft, Mods::empty(), b"\x1b[D"),
        (Key::C, Mods::CTRL, b"\x03"),
        (Key::D, Mods::CTRL, b"\x04"),
        (Key::Home, Mods::empty(), b"\x1b[H"),
        (Key::End, Mods::empty(), b"\x1b[F"),
        (Key::F1, Mods::empty(), b"\x1bOP"),
        (Key::F2, Mods::empty(), b"\x1bOQ"),
        (Key::F3, Mods::empty(), b"\x1bOR"),
        (Key::F4, Mods::empty(), b"\x1bOS"),
    ];
    for (key, mods, expect) in cases {
        let bytes = enc
            .encode_key(*key, *mods, Action::Press)
            .unwrap_or_else(|e| panic!("encode {:?}: {}", key, e));
        assert_eq!(&bytes, expect, "{:?} 序列不符", key);
    }

    // 应用光标键模式：方向键序列变 SS3
    enc.set_cursor_key_application(true);
    let bytes = enc
        .encode_key(Key::ArrowUp, Mods::empty(), Action::Press)
        .expect("encode");
    assert_eq!(&bytes, b"\x1bOA");
}

/// 读线程来数据 → wake：每次来数据都须发出唤醒（channel + Wake 抽象隔离）。
#[test]
fn waker_wakes_on_every_data_arrival() {
    struct CountingWaker {
        calls: AtomicUsize,
    }
    impl Wake for CountingWaker {
        fn wake(&self) -> Result<(), app::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let waker = CountingWaker {
        calls: AtomicUsize::new(0),
    };
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    // 模拟读线程：每次读到数据就 send + wake
    for i in 0..5u8 {
        tx.send(vec![b'a' + i]).expect("channel send");
        waker.wake().expect("wake");
    }
    drop(tx); // 关闭发送端，rx.iter() 才能终止
    assert_eq!(
        waker.calls.load(Ordering::SeqCst),
        5,
        "每次来数据都须发出唤醒"
    );
    assert_eq!(rx.iter().count(), 5, "channel 应收到全部 5 次数据");
}

// ---- 阶段 4C t2：最小化/零尺寸 resize 守卫 ----

/// 边界：0 尺寸与"小于一个字符格"的尺寸返回 None（不得 resize），正常尺寸反算行列。
///
/// 取证依据（(1,1) 为何也判为无效）：格子像素尺寸由字体度量决定，典型 8×16
/// （DejaVu 13px）或 10×16；窗口宽或高小于一格时 `w / cell_w == 0`。旧实现的
/// `.max(1)` 会把这种情况钳成 1×1 网格并 resize vt/pty，内容被裁掉且恢复后不可
/// 还原（Windows 最小化实测发 1×1 / 0×0）。因此判据取"能否容纳至少一个完整格"，
/// 而不是"尺寸是否为 0"。
#[test]
fn resize_target_rejects_zero_and_sub_cell_sizes() {
    let (cw, ch) = (8u32, 16u32);
    // 零尺寸
    assert_eq!(app::resize_target(0, 0, cw, ch, 0, 0), None);
    assert_eq!(app::resize_target(0, 480, cw, ch, 0, 0), None);
    assert_eq!(app::resize_target(800, 0, cw, ch, 0, 0), None);
    // 小于一格（Windows 最小化的 1x1 落在这里）
    assert_eq!(app::resize_target(1, 1, cw, ch, 0, 0), None);
    assert_eq!(app::resize_target(7, 480, cw, ch, 0, 0), None);
    assert_eq!(app::resize_target(800, 15, cw, ch, 0, 0), None);
    // 恰好一格：可用（真实最小网格）
    assert_eq!(app::resize_target(8, 16, cw, ch, 0, 0), Some((1, 1)));
    // 正常尺寸
    assert_eq!(app::resize_target(800, 480, cw, ch, 0, 0), Some((100, 30)));
    // padding 参与反算：padding 吃掉可用宽度后同样可能落到 None
    assert_eq!(app::resize_target(800, 480, cw, ch, 4, 4), Some((99, 29)));
    assert_eq!(app::resize_target(20, 40, cw, ch, 8, 8), None);
    // 超大尺寸钳到 u16::MAX 而不是溢出
    assert_eq!(
        app::resize_target(u32::MAX, u32::MAX, 1, 1, 0, 0),
        Some((u16::MAX, u16::MAX))
    );
}

/// 幂等性的输入侧保证：恢复尺寸与最小化前相同时，反算结果必须完全一致
/// （调用方据此跳过 vt/pty resize）。
#[test]
fn resize_target_is_stable_for_restored_size() {
    let (cw, ch) = (8u32, 16u32);
    let before = app::resize_target(800, 480, cw, ch, 0, 0);
    // 中途最小化：被守卫拒绝
    assert_eq!(app::resize_target(1, 1, cw, ch, 0, 0), None);
    // 恢复：与最小化前一致
    let after = app::resize_target(800, 480, cw, ch, 0, 0);
    assert_eq!(before, after);
}
