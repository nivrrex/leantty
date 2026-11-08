//! 探针：render 合成快照背景格被侵入的像素来源 + fontdue 度量实测。
use render::{CpuRenderer, Renderer};
use vt::{Cell, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline, CellWide};

fn main() {
    let snap = FrameSnapshot {
        cols: 2,
        rows: 1,
        cells: vec![vec![
            Cell::default(),
            Cell {
                chars: "X".to_string(),
                fg: Some(Rgb { r: 255, g: 0, b: 0 }),
                bg: Some(Rgb { r: 0, g: 0, b: 0 }),
                bold: false,
                italic: false,
                faint: false,
                blink: false,
                inverse: false,
                invisible: false,
                strikethrough: false,
                overline: false,
                underline: Underline::None,
                wide: CellWide::Narrow,
            },
        ]],
        cursor: Cursor { x: 0, y: 0, visible: true, style: CursorStyle::Block },
        scrollback_rows: 0,
        alternate_screen: false,
    };
    let mut r = CpuRenderer::new(None, None).expect("renderer");
    r.render(&snap, 32, 16).expect("render");
    let fb = r.framebuffer();
    let bg = fb[0];
    let mut diffs = 0;
    for (i, p) in fb.iter().enumerate() {
        if i < 16 * 16 && *p != bg {
            println!("slice[..256] diff idx={} real_px={} real_py={} val={:08x}", i, i % 32, i / 32, p);
            diffs += 1;
        }
    }
    println!("bg={:08x} left_diffs={} right_has_diff={}", bg, diffs, fb[16 * 16..].iter().any(|p| *p != bg));
    println!("font={} size={} fb=({},{})", r.font_family(), r.font_size(), r.framebuffer_size().0, r.framebuffer_size().1);

    // fontdue 直测：同一字体同一字号的 'X' 度量与位图边界。
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let id = db
        .query(&fontdb::Query {
            families: &[fontdb::Family::Name("Consolas")],
            ..Default::default()
        })
        .expect("Consolas face id");
    let face = db.face(id).expect("face");
    let bytes: Vec<u8> = match &face.source {
        fontdb::Source::File(p) => std::fs::read(p).expect("font file"),
        fontdb::Source::Binary(d) => d.as_ref().as_ref().to_vec(),
        fontdb::Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
    };
    let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default()).expect("fontdue");
    let (metrics, bitmap) = font.rasterize('X', 14.0);
    println!("X metrics: advance_w={} advance_h={} xmin={} ymin={} bitmap={}x{} bounds.xmin={}",
        metrics.advance_width, metrics.advance_height, metrics.xmin, metrics.ymin,
        metrics.width, metrics.height, metrics.bounds.xmin);
    let mut min_px = i32::MAX;
    let mut max_px = i32::MIN;
    for gy in 0..metrics.height as i32 {
        for gx in 0..metrics.width as i32 {
            if bitmap[(gy as usize) * metrics.width + gx as usize] > 0 {
                min_px = min_px.min(gx + metrics.bounds.xmin as i32);
                max_px = max_px.max(gx + metrics.bounds.xmin as i32);
            }
        }
    }
    println!("X ink span relative to origin: [{}, {}] (cell origin offset should be >=0 for no left bleed)", min_px, max_px);
    println!("--- framebuffer map (.=bg #=red-ish) ---");
    for row in 0..16u32 {
        let mut line = String::new();
        for col in 0..32u32 {
            let p = fb[(row * 32 + col) as usize];
            let r = (p >> 16) & 0xff;
            let g = (p >> 8) & 0xff;
            let b = p & 0xff;
            if r > 40 && r > g + 20 && r > b + 20 {
                line.push('#');
            } else {
                line.push('.');
            }
        }
        println!("{}", line);
    }
}
