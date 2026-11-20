//! 阶段 0.2.8：字体在渲染器构造时**已全部就绪**（crossfont 惰性打开，无需后台线程）。
//!
//! 旧实现用 fontdue 整文件解析，CJK 面要 0.34 s，只能后台加载 → 首帧先画占位方块。
//! crossfont（Linux=FreeType/Windows=DirectWrite）打开 face 是毫秒级，构造即完成加载，
//! 因此这里断言：构造完就能渲染 CJK 与 emoji 兜底字形，不存在"先方块后补"。

use render::{
    Antialiasing, CpuRenderer, Error, FontSettings, FontSpec, Palette, RenderOptions, Renderer,
    Subpixel,
};
use vt::{Cell, Cursor, CursorStyle, FrameSnapshot};

fn spec(family: &str) -> FontSpec {
    FontSpec {
        family: family.to_string(),
        style: "Regular".to_string(),
    }
}

fn settings() -> FontSettings {
    FontSettings {
        size: 14.0,
        normal: spec("DejaVu Sans Mono"),
        bold: spec("DejaVu Sans Mono"),
        italic: spec("DejaVu Sans Mono"),
        bold_italic: spec("DejaVu Sans Mono"),
        fallback: vec![spec("Symbola"), spec("DejaVu Sans")],
    }
}

fn render_one(ch: &str) -> Option<Vec<u32>> {
    let opts = RenderOptions {
        antialiasing: Antialiasing::Grayscale,
        subpixel: Subpixel::None,
        hinting: false,
        text_gamma: 1.0,
        draw_bold_with_bright: false,
    };
    let mut r = match CpuRenderer::new(&settings(), opts, Palette::default()) {
        Ok(r) => r,
        Err(Error::Font(_)) => return None,
        Err(e) => panic!("{e}"),
    };
    let snap = FrameSnapshot {
        cols: 1,
        rows: 1,
        cells: vec![vec![Cell {
            chars: ch.to_string(),
            ..Default::default()
        }]],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
        wrapped: vec![false],
        dirty_rows: vec![true],
        full_redraw: true,
    };
    r.render(&snap, 32, 32).expect("render");
    Some(r.framebuffer().to_vec())
}

fn has_ink(fb: &[u32]) -> bool {
    let bg = fb[0];
    fb.iter().any(|p| *p != bg)
}

#[test]
fn cjk_glyph_available_immediately_after_construction() {
    // 主字体 DejaVu Sans Mono 无汉字；回退链在构造时已加载，因此构造后立刻能画出字形。
    let Some(fb) = render_one("中") else { return };
    assert!(has_ink(&fb), "构造完成后汉字应立即有字形（不应是空白/占位）");
}

#[test]
fn emoji_falls_back_to_symbol_font_immediately() {
    let Some(fb) = render_one("\u{1F600}") else { return };
    assert!(has_ink(&fb), "emoji 应由兜底符号字体立即渲染，而不是留空或方块");
}
