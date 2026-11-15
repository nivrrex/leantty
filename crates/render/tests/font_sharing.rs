//! 字体实例共享的回归保护（阶段 4N t3）。
//!
//! 规格：**同一字体面在进程内只能有一份** `fontdue::Font` 实例。
//! 判据：把四个样式组里所有字体取 `Arc::as_ptr`，去重后的指针数应等于"唯一字体面数"；
//! 加入 1 个后台回退字体后只应增加 **1**（而不是 4——曾经的 `cloned()` 会按组深拷贝）。
//!
//! 若把 `add_background_fonts` 改回 `g.extend(fonts.iter().cloned())`（深拷贝），
//! 本测试会因为唯一指针数多出 3 而变红。

use std::sync::Arc;

use render::{CpuRenderer, FontSettings, FontSpec, RenderOptions};
use vt::Rgb;

fn settings(fallback: Vec<FontSpec>) -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: "DejaVu Sans Mono".to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 13.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback,
    }
}

fn palette() -> render::Palette {
    let c = Rgb { r: 255, g: 255, b: 255 };
    let b = Rgb { r: 0, g: 0, b: 0 };
    render::Palette {
        primary_bg: b,
        primary_fg: c,
        cursor_text: b,
        cursor: c,
        normal: [b; 8],
        bright: [c; 8],
    }
}

/// 去重后的实例数（按 Arc 指针身份）。
fn unique_instances(groups: &Arc<Vec<Vec<Arc<render::Font>>>>) -> usize {
    let mut ptrs: Vec<usize> = Vec::new();
    for g in groups.iter() {
        for f in g.iter() {
            let p = Arc::as_ptr(f) as usize;
            if !ptrs.contains(&p) {
                ptrs.push(p);
            }
        }
    }
    ptrs.len()
}

/// 四个样式在本机实际解析出的**唯一 face 数**（不写死 4：换机器/换字体会不同）。
fn expected_unique_faces() -> Option<usize> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut ids: Vec<fontdb::ID> = Vec::new();
    for style in ["Regular", "Bold", "Italic", "Bold Italic"] {
        let (w, s) = match style {
            "Bold" => (fontdb::Weight::BOLD, fontdb::Style::Normal),
            "Italic" => (fontdb::Weight::NORMAL, fontdb::Style::Italic),
            "Bold Italic" => (fontdb::Weight::BOLD, fontdb::Style::Italic),
            _ => (fontdb::Weight::NORMAL, fontdb::Style::Normal),
        };
        let q = fontdb::Query {
            families: &[
                fontdb::Family::Name("DejaVu Sans Mono"),
                fontdb::Family::Monospace,
            ],
            weight: w,
            style: s,
            ..Default::default()
        };
        if let Some(id) = db.query(&q)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    if ids.is_empty() { None } else { Some(ids.len()) }
}

#[test]
fn same_face_is_shared_not_cloned() {
    let Some(expected) = expected_unique_faces() else {
        println!("SKIP: 本机没有 DejaVu Sans Mono / Monospace 族（无法建立字体链）");
        return;
    };
    let r = CpuRenderer::new(&settings(Vec::new()), RenderOptions::default(), palette())
        .expect("CpuRenderer::new");
    let groups = r.font_groups();
    let before = unique_instances(&groups);
    // 唯一实例数应等于"四个样式实际解析出的唯一 face 数"（本机 DejaVu 四样式为 4）
    assert_eq!(
        before, expected,
        "唯一实例数应等于唯一 face 数（实测 {before}，按 fontdb 推得 {expected}）；\
         若大于 expected 说明同一 face 被复制了多份"
    );

    // 每个组里同一字体面必须指向同一实例
    for (gi, g) in groups.iter().enumerate() {
        let mut seen: Vec<usize> = Vec::new();
        for f in g.iter() {
            let p = Arc::as_ptr(f) as usize;
            assert!(!seen.contains(&p), "组 {gi} 内出现重复实例（同一份字体被复制）");
            seen.push(p);
        }
    }

    // 加 1 个回退字体：四个组共享同一份 → 唯一实例数只 +1
    let extra: render::Font = (*groups[0][0]).clone();
    let mut r = r;
    r.add_background_fonts(vec![extra]);
    let groups2 = r.font_groups();
    let after = unique_instances(&groups2);
    assert_eq!(
        after,
        before + 1,
        "加入 1 个回退字体后唯一实例数应只 +1（实测 {before}→{after}）；\
         若 +4 说明仍在按样式组深拷贝"
    );

    // 每个组的末尾都指向同一个回退实例
    let mut tails: Vec<usize> = Vec::new();
    for g in groups2.iter() {
        let p = Arc::as_ptr(g.last().expect("每组至少一个字体")) as usize;
        tails.push(p);
    }
    assert!(
        tails.windows(2).all(|w| w[0] == w[1]),
        "四个组的回退字体必须是同一实例（实测各尾指针 {tails:?}）"
    );
}
