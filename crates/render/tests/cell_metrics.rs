#![cfg(unix)]
//! cell_metrics 单测：字号与缩放因子缩放性（系统字体缺失时跳过并说明）。

const FONT: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf";

#[test]
fn cell_metrics_scales_with_size_and_scale_factor() {
    let data = match std::fs::read(FONT) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("skip: {FONT} 不存在（系统缺 DejaVu 字体）");
            return;
        }
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
