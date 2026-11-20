//! 诊断：crossfont 的 "点 → 像素" 链条（Size=点、as_px=×96/72、metrics 需先 get_glyph）。
//!
//! 实测结论（本文件即是探针）：`Size::new(pt*scale)`、`Size::from_px(px)`、
//! `Size::new(px*0.75)` 三种写法在 as_px 相同时结果完全一致；px=32 时
//! advance=19、line_height=38、descent=-8。若应用侧量出别的格子尺寸，
//! 问题在"传进来的 scale/字号"，不在这条换算链上。

use crossfont::{FontDesc, Rasterize, Rasterizer, Size, Style, GlyphKey};


#[test]
fn probe_chain() {
    let mut r = Rasterizer::new().unwrap();
    let desc = FontDesc::new("DejaVu Sans Mono", Style::Specific("Regular".into()));
    for (pt, scale) in [(16.0f32, 1.0f32), (16.0, 1.5)] {
        // 目标像素 = pt*scale*96/72
        let px = pt * scale * 96.0 / 72.0;
        let key = r.load_font(&desc, Size::new(pt * scale)).unwrap();
        for (tag, size) in [("Size::new(pt*scale)", Size::new(pt * scale)),
                            ("Size::from_px(px)", Size::from_px(px)),
                            ("Size::new(px*0.75)", Size::new(px * 0.75))] {
            let g = r.get_glyph(GlyphKey { character: '0', font_key: key, size });
            let m = r.metrics(key, size);
            println!("pt={pt} scale={scale} px={px:.2} {tag}: as_px={:.2} glyph_adv={:?} glyph_h={:?} metrics_avg={:?} line={:?} desc={:?}",
                size.as_px(),
                g.as_ref().map(|g| g.advance).ok(),
                g.as_ref().map(|g| g.height).ok(),
                m.as_ref().map(|m| m.average_advance).ok(),
                m.as_ref().map(|m| m.line_height).ok(),
                m.as_ref().map(|m| m.descent).ok());
        }
    }
}

#[test]
fn fontset_cell_metrics_matches_chain() {
    let s = |f: &str| render::FontSpec { family: f.into(), style: "Regular".into() };
    let settings = render::FontSettings {
        size: 16.0,
        normal: s("DejaVu Sans Mono"),
        bold: s("DejaVu Sans Mono"),
        italic: s("DejaVu Sans Mono"),
        bold_italic: s("DejaVu Sans Mono"),
        fallback: Vec::new(),
    };
    let Ok(set) = render::FontSet::load(&settings) else { return };
    let mut set = set.lock().unwrap();
    for scale in [1.0f64, 1.5] {
        println!("FontSet scale={scale} cell={:?}", set.cell_metrics(16.0, scale));
    }
}
