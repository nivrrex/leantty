//! 字体加载耗时探针（阶段 2D 任务 3，只测量不改库）。
//!
//! 用 `cargo test --release -p render --test font_load_time -- --ignored --nocapture`
//! 打印系统字体中 fontdue::Font::from_bytes 的耗时（各 5 次）。
//! 默认 `#[ignore]`：依赖本机字体，不作为常规测试门禁。

use std::time::Instant;

/// 通过 fontdb 取字体字节（与 render 加载路径一致）。
fn face_bytes(family: &str) -> Option<(Vec<u8>, u64)> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })?;
    let face = db.face(id)?;
    // 与 render 的 load_font_styled 相同的取字节路径（fontdb Source 三种变体）。
    let bytes: Vec<u8> = match &face.source {
        fontdb::Source::File(path) => std::fs::read(path).ok()?,
        fontdb::Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        fontdb::Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };
    let len = bytes.len() as u64;
    // 改用内存字节重建（与 CpuRenderer 相同的 from_bytes 入口）
    Some((bytes, len))
}

fn measure(label: &str, family: &str) {
    let Some((bytes, len)) = face_bytes(family) else {
        println!("{label}: 字体 {family} 不存在，跳过");
        return;
    };
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let font = fontdue::Font::from_bytes(&bytes[..], fontdue::FontSettings::default());
        let dt = t.elapsed();
        assert!(font.is_ok(), "from_bytes 失败");
        times.push(dt.as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{label} family={family} size={:.1}MB from_bytes_ms={:?} median={:.1}",
        len as f64 / 1048576.0,
        times.iter().map(|t| format!("{t:.1}")).collect::<Vec<_>>(),
        times[2]
    );
}

#[test]
#[ignore = "依赖本机字体，手动运行"]
fn font_load_time_probe() {
    measure("small-mono", "DejaVu Sans Mono");
    measure("large-cjk", "Microsoft YaHei");
    measure("larger-cjk", "SimSun");
}
