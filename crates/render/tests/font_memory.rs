//! 字体内存取证（阶段 4N t2）：`fontdue::Font` 的单实例大小与 `clone` 的增量。
//!
//! 为什么需要它：Windows 3K 实测 release 默认配置空闲工作集约 879MB、去掉回退后约
//! 189MB（差约 690MB）。假设是"fontdue 解析后常驻全部字形轮廓、Clone 是深拷贝，
//! 而同一回退字体面在四个样式组中各存一份"。本测试用**只依赖 std** 的计数分配器，
//! 直接量出"一份实例占多少字节"与"clone 再涨多少"，供与 RSS 观测对照。
//!
//! 注意：计数分配器统计的是**当前存活**的分配字节（alloc 加、dealloc 减），不含
//! 分配器自身的 arena 保留；因此它是"实例大小"的下界估计，与 RSS 的对照在报告里说明。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use render::{FontSettings, FontSpec};
use vt::Rgb;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE.fetch_add(new_size, Ordering::Relaxed);
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
}

#[global_allocator]
static A: Counting = Counting;

fn live_mb() -> f64 {
    LIVE.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
}

/// 按族名 + 样式在系统字体库中取字体文件字节（与产品同一条 fontdb 查询路径）。
fn font_bytes(family: &str, style: &str) -> Option<(Vec<u8>, String)> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let (w, s) = match style {
        "Bold" => (fontdb::Weight::BOLD, fontdb::Style::Normal),
        "Italic" => (fontdb::Weight::NORMAL, fontdb::Style::Italic),
        "Bold Italic" => (fontdb::Weight::BOLD, fontdb::Style::Italic),
        _ => (fontdb::Weight::NORMAL, fontdb::Style::Normal),
    };
    let q = fontdb::Query {
        families: &[fontdb::Family::Name(family), fontdb::Family::Monospace],
        weight: w,
        style: s,
        ..Default::default()
    };
    let id = db.query(&q)?;
    let face = db.face(id)?;
    let (bytes, path) = match &face.source {
        fontdb::Source::File(p) => (std::fs::read(p).ok()?, p.display().to_string()),
        fontdb::Source::Binary(d) => (d.as_ref().as_ref().to_vec(), "<binary>".to_string()),
        fontdb::Source::SharedFile(p, d) => (d.as_ref().as_ref().to_vec(), p.display().to_string()),
    };
    Some((bytes, path))
}

#[test]
fn measure_font_instance_and_clone_cost() {
    // 与产品一致的 FontSettings（只为让编译期常量使用；本测试不用渲染器）
    let _ = FontSettings {
        size: 13.0,
        normal: FontSpec { family: "monospace".into(), style: "Regular".into() },
        bold: FontSpec { family: "monospace".into(), style: "Bold".into() },
        italic: FontSpec { family: "monospace".into(), style: "Italic".into() },
        bold_italic: FontSpec { family: "monospace".into(), style: "Bold Italic".into() },
        fallback: Vec::new(),
    };
    let _ = Rgb { r: 0, g: 0, b: 0 };

    // (显示名, 族名, 样式)
    let cases = [
        ("DejaVu Sans Mono (小字体)", "DejaVu Sans Mono", "Regular"),
        ("Noto Sans CJK (fallback 候选)", "Noto Sans CJK SC", "Regular"),
        ("Noto Sans CJK JP (同一 .ttc 的另一面)", "Noto Sans CJK JP", "Regular"),
    ];

    println!("| 字体面 | 文件字节 | 实例 MB | clone 增量 MB | clone/实例 |");
    println!("|---|---|---|---|---|");
    for (label, family, style) in cases {
        let Some((bytes, path)) = font_bytes(family, style) else {
            println!("| {label} | （本机未安装，SKIP） | - | - | - |");
            continue;
        };
        let before = live_mb();
        let font = match fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default()) {
            Ok(f) => f,
            Err(e) => {
                println!("| {label} | {} | 解析失败: {e} | - | - |", bytes.len());
                continue;
            }
        };
        let after_load = live_mb();
        let cloned = font.clone();
        let after_clone = live_mb();
        println!(
            "| {label} | {} ({} B) | {:.1} | {:.1} | {:.0}% |",
            path,
            bytes.len(),
            after_load - before,
            after_clone - after_load,
            (after_clone - after_load) / (after_load - before).max(0.001) * 100.0
        );
        drop(cloned);
        drop(font);
        // 释放后应回到基线（允许分配器保留误差）
        let after_drop = live_mb();
        println!(
            "|  ↳ 释放后回落到 | | {:.1}（基线 {:.1}） | | |",
            after_drop, before
        );
    }
}
