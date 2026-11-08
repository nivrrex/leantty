fn main() {
    let mut terminal = libghostty_vt::Terminal::new(80, 24).expect("Terminal::new");
    terminal.vt_write(b"hello\r\n");
    println!("downstream link ok");
}
