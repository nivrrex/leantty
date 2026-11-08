//! V7：KeyEncoder 按键序列断言（普通 + 应用光标键模式）。

use libghostty_vt::key::{Action, Encoder, Event, Key, Mods};

fn encode(key: Key, mods: Mods, app_cursor: bool) -> Vec<u8> {
    let mut enc = Encoder::new().expect("Encoder::new");
    enc.set_cursor_key_application(app_cursor);
    let mut ev = Event::new().expect("Event::new");
    ev.set_action(Action::Press);
    ev.set_key(key);
    ev.set_mods(mods);
    let mut v = Vec::new();
    enc.encode_to_vec(&ev, &mut v).expect("encode_to_vec");
    v
}

fn assert_seq(key: Key, mods: Mods, app_cursor: bool, expect: &[u8]) {
    let got = encode(key, mods, app_cursor);
    assert_eq!(got, expect, "{:?} 序列不符", key);
}

#[test]
fn v7_keys_normal_mode() {
    assert_seq(Key::ArrowUp, Mods::empty(), false, b"\x1b[A");
    assert_seq(Key::ArrowDown, Mods::empty(), false, b"\x1b[B");
    assert_seq(Key::ArrowLeft, Mods::empty(), false, b"\x1b[D");
    assert_seq(Key::ArrowRight, Mods::empty(), false, b"\x1b[C");
    assert_seq(Key::Enter, Mods::empty(), false, b"\r");
    assert_seq(Key::Backspace, Mods::empty(), false, b"\x7f");
    assert_seq(Key::Tab, Mods::empty(), false, b"\t");
    assert_seq(Key::C, Mods::CTRL, false, b"\x03");
    assert_seq(Key::D, Mods::CTRL, false, b"\x04");
    assert_seq(Key::Escape, Mods::empty(), false, b"\x1b");
    assert_seq(Key::F1, Mods::empty(), false, b"\x1bOP");
    assert_seq(Key::Home, Mods::empty(), false, b"\x1b[H");
    assert_seq(Key::End, Mods::empty(), false, b"\x1b[F");
    assert_seq(Key::Tab, Mods::SHIFT, false, b"\x1b[Z");
}

#[test]
fn v7_keys_application_cursor_mode() {
    assert_seq(Key::ArrowUp, Mods::empty(), true, b"\x1bOA");
    assert_seq(Key::ArrowDown, Mods::empty(), true, b"\x1bOB");
    assert_seq(Key::ArrowLeft, Mods::empty(), true, b"\x1bOD");
    assert_seq(Key::ArrowRight, Mods::empty(), true, b"\x1bOC");
}
