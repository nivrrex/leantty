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
        // 兜底字体按平台列：Symbola（Linux 常见）+ Windows 的 Segoe UI Symbol / Microsoft YaHei；
        // 缺失的条目由 FontSet::load 跳过，因此两种平台都能命中各自可用的那一个。
        fallback: vec![
            spec("Symbola"),
            spec("Segoe UI Symbol"),
            spec("Microsoft YaHei"),
            spec("DejaVu Sans"),
        ],
        synthetic_bold: true,
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
    assert!(
        has_ink(&fb),
        "构造完成后汉字应立即有字形（不应是空白/占位）"
    );
}

#[test]
fn emoji_falls_back_to_symbol_font_immediately() {
    let Some(fb) = render_one("\u{1F600}") else {
        return;
    };
    assert!(
        has_ink(&fb),
        "emoji 应由兜底符号字体立即渲染，而不是留空或方块"
    );
}

/// 阶段 0.2.9 回归：CJK 必须落到**配置的 fallback**，ASCII 留在主字体。
///
/// 背景：crossfont 的 `get_glyph` 在缺字时会自动换 face，所以之前用"get_glyph 是否成功"
/// 判断覆盖是无效的——`index_for` 永远返回 0，配置里的 fallback 被绕过。
#[test]
fn cjk_resolves_to_configured_fallback() {
    let s = |f: &str| FontSpec {
        family: f.to_string(),
        style: "Regular".to_string(),
    };
    let settings = FontSettings {
        size: 14.0,
        normal: s("DejaVu Sans Mono"),
        bold: s("DejaVu Sans Mono"),
        italic: s("DejaVu Sans Mono"),
        bold_italic: s("DejaVu Sans Mono"),
        fallback: vec![s("Noto Sans CJK SC")],
        synthetic_bold: true,
    };
    let Ok(set) = render::FontSet::load(&settings) else {
        eprintln!("SKIP: 缺少 DejaVu Sans Mono");
        return;
    };
    let mut set = set.borrow_mut();
    assert_eq!(set.index_for(0, 'A'), Some(0), "ASCII 应由主字体渲染");
    assert_eq!(
        set.index_for(0, '中'),
        Some(1),
        "汉字必须落到 fallback（此前恒为 Some(0)：crossfont 自动换 face 导致判定失效）"
    );
    assert_eq!(
        set.index_for(0, '\u{1F984}'),
        None,
        "链上没有的字符不应被硬塞给主字体"
    );
}
