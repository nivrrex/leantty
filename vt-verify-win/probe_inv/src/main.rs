// What does the VT layer report for a default-coloured cell under ESC[7m?
fn main() {
    let c = |r: u8, g: u8, b: u8| vt::Rgb { r, g, b };
    let mut p = vt::VtPalette {
        foreground: c(0xff, 0xff, 0xff),
        background: c(0x00, 0x00, 0x00),
        cursor: c(0x00, 0x00, 0xff),
        normal: [c(0,0,0); 8],
        bright: [c(0,0,0); 8],
    };
    p.normal[1] = c(0xd3, 0x2f, 0x2f);
    p.normal[7] = c(0x75, 0x75, 0x75);
    let mut t = vt::VtTerminal::new_with_palette(20, 3, &p).expect("term");
    t.feed(b"\x1b[2J\x1b[H\x1b[7m          ");
    let snap = t.snapshot().expect("snap");
    for (i, cell) in snap.cells[0].iter().take(3).enumerate() {
        println!("cell{i} inverse={} fg={:?} bg={:?} chars={:?}", cell.inverse, cell.fg, cell.bg, cell.chars);
    }
    // and an indexed red cell
    t.feed(b"\x1b[0m\x1b[31mX");
    let snap2 = t.snapshot().expect("snap2");
    let cell = &snap2.cells[1][0];
    println!("red cell fg={:?} bg={:?}", cell.fg, cell.bg);
}
