//! cell_metrics 单测：字号与缩放因子缩放性（系统字体缺失时跳过并说明）。
//!
//! 字体按**族名**经 fontdb 查询（阶段 4E t4：此前写死 Linux 绝对路径，导致本文件
//! 在 Windows 上整片被 `#![cfg(unix)]` 排除）。找不到任何等宽字体时显式 SKIP。

use fontdb::{Database, Family, Query, Source, Weight};

/// 依次尝试的等宽字体族（覆盖 Linux / Windows 常见系统字体）。
const CANDIDATES: &[&str] = &["DejaVu Sans Mono", "Consolas", "Courier New", "Liberation Mono"];

/// 按族名查询并返回字体字节（找不到返回 None）。
fn load_mono_font_bytes() -> Option<Vec<u8>> {
    let mut db = Database::new();
    db.load_system_fonts();
    for fam in CANDIDATES {
        let q = Query {
            families: &[Family::Name(fam), Family::Monospace],
            weight: Weight::NORMAL,
            ..Default::default()
        };
        let Some(id) = db.query(&q) else { continue };
        let Some(face) = db.face(id) else { continue };
        let bytes = match &face.source {
            Source::File(p) => std::fs::read(p).ok()?,
            Source::Binary(d) => d.as_ref().as_ref().to_vec(),
            Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
        };
        return Some(bytes);
    }
    None
}

#[test]
fn cell_metrics_scales_with_size_and_scale_factor() {
    let Some(data) = load_mono_font_bytes() else {
        eprintln!(
            "SKIP: 系统中找不到任何候选等宽字体（{:?}）",
            CANDIDATES
        );
        return;
    };
    let font = fontdue::Font::from_bytes(data.as_slice(), Default::default())
        .expect("测试字体应可解析");
    let (w1, h1) = render::cell_metrics(&font, 13.0, 1.0);
    let (w2, h2) = render::cell_metrics(&font, 26.0, 1.0);
    assert!(w2 > w1, "字号翻倍宽应增大: {w1} -> {w2}");
    assert!(h2 > h1, "字号翻倍高应增大: {h1} -> {h2}");
    let (w3, h3) = render::cell_metrics(&font, 13.0, 2.0);
    assert_eq!((w3, h3), (w2, h2), "scale=2.0 应等价于字号翻倍");
    assert!(w1 >= 1 && h1 >= 1, "格子至少 1 像素: {w1}x{h1}");
}
