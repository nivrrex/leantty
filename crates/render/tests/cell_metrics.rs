//! 格子度量：字号/缩放联动（阶段 0.2.8 起由 crossfont 提供度量）。



#[test]
fn cell_metrics_scales_with_size_and_scale_factor() {
    // 阶段 0.2.8：度量改由 crossfont 提供（不再暴露 fontdue::Font）。
    let spec = |f: &str| render::FontSpec {
        family: f.to_string(),
        style: "Regular".to_string(),
    };
    let settings = render::FontSettings {
        size: 13.0,
        normal: spec("DejaVu Sans Mono"),
        bold: spec("DejaVu Sans Mono"),
        italic: spec("DejaVu Sans Mono"),
        bold_italic: spec("DejaVu Sans Mono"),
        fallback: Vec::new(),
    };
    let Ok(set) = render::FontSet::load(&settings) else {
        eprintln!("SKIP: 系统无 DejaVu Sans Mono");
        return;
    };
    let mut set = set.lock().unwrap();
    let (w1, h1) = set.cell_metrics(13.0, 1.0);
    let (w2, h2) = set.cell_metrics(26.0, 1.0);
    assert!(w2 > w1, "字号翻倍宽应增大: {w1} -> {w2}");
    assert!(h2 > h1, "字号翻倍高应增大: {h1} -> {h2}");
    let (w3, h3) = set.cell_metrics(13.0, 2.0);
    assert_eq!((w3, h3), (w2, h2), "scale=2.0 应等价于字号翻倍");
    assert!(w1 >= 1 && h1 >= 1, "格子至少 1 像素: {w1}x{h1}");
}
