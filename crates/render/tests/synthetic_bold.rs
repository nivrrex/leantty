//! 合成粗体：缺 Bold 面时 SGR 1 必须比 SGR 0 更重（`font.synthetic_bold`，默认开）。
//!
//! 背景（已验证事实）：crossfont 在族内没有 Bold 时会把两个请求解析到同一个 face，
//! 于是 `SGR 1` 与 `SGR 0` 渲染**逐字节相同**——很多 CJK 字体、部分等宽字体都这样，
//! 用户看到的就是"粗体没了"。alacritty 同样不做合成粗体；leantty-zig 0.1.15 起做。
//!
//! 断言只依赖"墨迹量"这种与字体无关的量，不写死像素值：换字体/换平台不会假红，
//! 但"粗体等于常规"这一缺陷一定会红。

use render::{
    Antialiasing, CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer, Subpixel,
};
use vt::{Cell, Cursor, CursorStyle, FrameSnapshot, Rgb};

/// 单字重字体的候选（Linux 常见的 CJK 单字重字体；都不在时相关用例跳过）。
const SINGLE_WEIGHT: &[&str] = &["汉仪旗黑", "HYQiHei", "WenQuanYi Zen Hei", "AR PL UKai CN"];

/// 有真 Bold 面的等宽字体（DejaVu 系一定有；缺失时跳过）。
const WITH_BOLD: &str = "DejaVu Sans Mono";

fn settings(family: &str, fallback: Vec<&str>, synthetic_bold: bool) -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 14.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: fallback
            .into_iter()
            .map(|f| FontSpec {
                family: f.to_string(),
                style: "Regular".to_string(),
            })
            .collect(),
        synthetic_bold,
    }
}

fn snapshot(text: &str, bold: bool) -> FrameSnapshot {
    let cells: Vec<Cell> = (0..12)
        .map(|_| Cell {
            chars: text.to_string(),
            fg: Some(Rgb {
                r: 255,
                g: 255,
                b: 255,
            }),
            bg: Some(Rgb { r: 0, g: 0, b: 0 }),
            bold,
            ..Default::default()
        })
        .collect();
    FrameSnapshot {
        cols: 12,
        rows: 1,
        cells: vec![cells],
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
    }
}

/// 渲染一次并返回 (覆盖率像素数, 覆盖率总和)。
fn ink(settings: &FontSettings, text: &str, bold: bool) -> Option<(usize, u64)> {
    let mut r = CpuRenderer::new(
        settings,
        RenderOptions {
            antialiasing: Antialiasing::Grayscale,
            subpixel: Subpixel::None,
            hinting: false,
            text_gamma: 1.0,
            draw_bold_with_bright: false,
        },
        Palette::default(),
    )
    .ok()?;
    r.render(&snapshot(text, bold), 12 * 10, 22).ok()?;
    let mut n = 0usize;
    let mut sum = 0u64;
    for p in r.framebuffer().iter() {
        let g = (p & 0xFF) as u8;
        if g > 40 {
            n += 1;
        }
        sum += g as u64;
    }
    Some((n, sum))
}

fn first_available(candidates: &[&'static str]) -> Option<&'static str> {
    for fam in candidates {
        // 用 1x1 渲染探活：字体不可用时 CpuRenderer::new 会失败。
        if ink(&settings(fam, vec![], true), "M", false).is_some() {
            return Some(fam);
        }
    }
    None
}

#[test]
fn single_weight_font_gets_synthetic_bold() {
    let Some(family) = first_available(SINGLE_WEIGHT) else {
        eprintln!("跳过：本机没有单字重候选字体（{SINGLE_WEIGHT:?}）");
        return;
    };
    let s = settings(family, vec![], true);
    let (_, plain) = ink(&s, "HAMBURGEFONT", false).expect("render");
    let (_, bold) = ink(&s, "HAMBURGEFONT", true).expect("render");
    let gain = bold as f64 / plain as f64 - 1.0;
    assert!(
        gain > 0.15,
        "单字重字体（{family}）的粗体必须明显更重：常规 {plain} → 粗体 {bold}（+{:.1}%）",
        gain * 100.0
    );
}

#[test]
fn synthetic_bold_can_be_disabled() {
    let Some(family) = first_available(SINGLE_WEIGHT) else {
        eprintln!("跳过：本机没有单字重候选字体");
        return;
    };
    let on = settings(family, vec![], true);
    let off = settings(family, vec![], false);
    let (_, bold_on) = ink(&on, "HAMBURGEFONT", true).expect("render");
    let (_, bold_off) = ink(&off, "HAMBURGEFONT", true).expect("render");
    assert!(
        bold_on > bold_off,
        "synthetic_bold=false 应回到\"粗体=常规\"：on={bold_on} off={bold_off}"
    );
    // 关掉之后粗体与常规逐字节相同（冻结 alacritty 行为）
    let (_, plain_off) = ink(&off, "HAMBURGEFONT", false).expect("render");
    assert_eq!(
        bold_off, plain_off,
        "synthetic_bold=false 时单字重字体的粗体墨迹必须等于常规"
    );
}

#[test]
fn real_bold_face_is_not_emboldened() {
    let s_on = settings(WITH_BOLD, vec![], true);
    let s_off = settings(WITH_BOLD, vec![], false);
    let Some((_, bold_on)) = ink(&s_on, "HAMBURGEFONT", true) else {
        eprintln!("跳过：本机没有 {WITH_BOLD}");
        return;
    };
    let (_, bold_off) = ink(&s_off, "HAMBURGEFONT", true).expect("render");
    assert_eq!(
        bold_on, bold_off,
        "有真 Bold 面的字体不应再被合成加粗：on={bold_on} off={bold_off}"
    );
    let (_, plain) = ink(&s_off, "HAMBURGEFONT", false).expect("render");
    assert!(
        bold_off as f64 > plain as f64 * 1.1,
        "真 Bold 面本来就该更重：常规 {plain} 粗体 {bold_off}"
    );
}

#[test]
fn fallback_face_without_bold_is_emboldened() {
    // 主字体有 Bold、回退的 CJK 字体没有：中文粗体也必须加粗（判断落在 face 上而非家族）
    let Some(cjk) = first_available(SINGLE_WEIGHT) else {
        eprintln!("跳过：本机没有单字重 CJK 候选字体");
        return;
    };
    let s = settings(WITH_BOLD, vec![cjk], true);
    let Some((_, plain)) = ink(&s, "中文粗体测试", false) else {
        eprintln!("跳过：{WITH_BOLD} 不可用");
        return;
    };
    let (_, bold) = ink(&s, "中文粗体测试", true).expect("render");
    assert!(
        bold as f64 > plain as f64 * 1.1,
        "回退 CJK 字体（{cjk}）缺 Bold 时，中文粗体也应加粗：常规 {plain} → 粗体 {bold}"
    );
}
