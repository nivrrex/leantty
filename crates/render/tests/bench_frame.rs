//! CPU 渲染帧耗时基准（阶段 2D 任务 4，`#[ignore]`，手动运行）。
//!
//! 运行：
//! `cargo test --release -p render --test bench_frame -- --ignored --nocapture`
//!
//! 说明：
//! - 只用 render crate 内的 CpuRenderer，不修改 app、不加运行时开关；
//! - 帧缓冲为 `Vec<u32>`（0xFFRRGGBB），与 softbuffer 像素格式一致，
//!   因此"帧缓冲转 softbuffer 像素格式"在 render 内不存在额外转换步骤（拷贝由 app 侧完成，
//!   属于 `buffer[i] = *px` 的逐元素拷贝，本基准不计入，另行在报告中说明）；
//! - 覆盖 120x30 / 200x60 与缩放 1.0 / 2.0，含 ASCII、汉字宽字符、彩色样式、
//!   选区、滚动条；字形缓存冷/热各测一次；
//! - 另测"仅 1 个单元格变化"的帧耗时，用于判断是否存在脏区增量渲染。

use std::time::{Duration, Instant};

use render::{
    Antialiasing, CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer, Subpixel,
};
use vt::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline};

fn settings() -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: "DejaVu Sans Mono".to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 14.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
    }
}

fn opts() -> RenderOptions {
    RenderOptions {
        antialiasing: Antialiasing::Grayscale,
        subpixel: Subpixel::None,
        hinting: false,
        text_gamma: 1.0,
        draw_bold_with_bright: true,
    }
}

/// 合成满屏快照：ASCII 文本为主，每 8 行间隔插入汉字宽字符，混合彩色/样式。
fn synth(cols: u16, rows: u16) -> FrameSnapshot {
    let colors = [
        None,
        Some(Rgb { r: 205, g: 0, b: 0 }),
        Some(Rgb { r: 0, g: 135, b: 0 }),
        Some(Rgb { r: 0, g: 0, b: 238 }),
        Some(Rgb { r: 220, g: 160, b: 0 }),
    ];
    let mut cells: Vec<Vec<Cell>> = Vec::with_capacity(rows as usize);
    let cjk = ['中', '文', '宽', '字'];
    for y in 0..rows {
        let mut line: Vec<Cell> = Vec::with_capacity(cols as usize);
        let mut x = 0u16;
        while x < cols {
            // 每 16 列出现一个宽字符（占 2 格）
            if x % 16 == 8 && x + 1 < cols {
                let ch = cjk[(y as usize + x as usize) % cjk.len()];
                line.push(Cell {
                    chars: ch.to_string(),
                    fg: colors[(x as usize) % colors.len()],
                    bold: x % 32 == 8,
                    wide: CellWide::Wide,
                    ..Default::default()
                });
                line.push(Cell {
                    chars: String::new(),
                    wide: CellWide::SpacerTail,
                    ..Default::default()
                });
                x += 2;
                continue;
            }
            let code = b'!' + ((x as usize + y as usize) % 90) as u8;
            line.push(Cell {
                chars: (code as char).to_string(),
                fg: colors[(x as usize + y as usize) % colors.len()],
                bg: if (x / 10 + y / 5).is_multiple_of(7) {
                    Some(Rgb { r: 40, g: 40, b: 60 })
                } else {
                    None
                },
                bold: x.is_multiple_of(24),
                italic: x % 40 == 1,
                underline: if x % 20 == 3 {
                    Underline::Single
                } else {
                    Underline::None
                },
                strikethrough: x % 60 == 5,
                inverse: x % 45 == 7,
                ..Default::default()
            });
            x += 1;
        }
        cells.push(line);
    }
    FrameSnapshot {
        cols,
        rows,
        cells,
        cursor: Cursor {
            x: cols / 2,
            y: rows - 2,
            visible: true,
            style: CursorStyle::Block,
        },
        scrollback_rows: 500,
        alternate_screen: false,
        wrapped: vec![false; rows as usize],
    }
}

fn stats(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let n = samples.len();
    let median = samples[n / 2].as_secs_f64() * 1000.0;
    let p95 = samples[(n * 95 / 100).min(n - 1)].as_secs_f64() * 1000.0;
    let max = samples[n - 1].as_secs_f64() * 1000.0;
    let min = samples[0].as_secs_f64() * 1000.0;
    println!(
        "{label}: median={median:.2}ms p95={p95:.2}ms max={max:.2}ms min={min:.2}ms (n={n})"
    );
}

/// 单配置基准：冷帧（新 renderer 首帧）+ 热帧（预热 20 后采样 200）。
fn bench_case(cols: u16, rows: u16, scale: f64) {
    let snap = synth(cols, rows);
    let mut renderer = match CpuRenderer::new(&settings(), opts(), Palette::default()) {
        Ok(r) => r,
        Err(e) => {
            println!("skip: CpuRenderer::new 失败（{e}）");
            return;
        }
    };
    renderer.set_scale_factor(scale);
    let (cw, ch) = renderer.cell_metrics(scale);
    let pw = u32::from(cols) * cw;
    let ph = u32::from(rows) * ch;
    renderer.set_selection(Some((1, 1, cols / 2, rows / 2)));
    renderer.set_scrollbar(Some((ph / 3, ph / 6)));

    // 冷帧：字形缓存为空的首帧（真实启动路径）
    let t = Instant::now();
    renderer.render(&snap, pw, ph).expect("render");
    let cold = t.elapsed();

    // 预热 20 帧
    for _ in 0..20 {
        renderer.render(&snap, pw, ph).expect("render");
    }
    // 采样 200 帧（静态画面 = 字形全命中缓存）
    let mut samples = Vec::with_capacity(200);
    for _ in 0..200 {
        let t = Instant::now();
        renderer.render(&snap, pw, ph).expect("render");
        samples.push(t.elapsed());
    }
    stats(
        &format!("{cols}x{rows}@{scale}x full-frame hot"),
        samples,
    );
    println!(
        "  {cols}x{rows}@{scale}x cold first frame: {:.2}ms（px={}x{}，cell={}x{}）",
        cold.as_secs_f64() * 1000.0,
        pw,
        ph,
        cw,
        ch
    );

    // 单格变化：每帧只改一个单元格文本（当前实现全帧重画，用于判断有无脏区增量）
    let mut snap2 = snap.clone();
    let mut samples = Vec::with_capacity(200);
    for i in 0..200usize {
        let x = (i % cols as usize) as u16;
        let y = ((i / cols as usize) % rows as usize) as u16;
        if let Some(row) = snap2.cells.get_mut(y as usize)
            && let Some(c) = row.get_mut(x as usize)
            && c.wide == CellWide::Narrow
        {
            c.chars = ((b'a' + (i % 26) as u8) as char).to_string();
        }
        let t = Instant::now();
        renderer.render(&snap2, pw, ph).expect("render");
        samples.push(t.elapsed());
    }
    stats(
        &format!("{cols}x{rows}@{scale}x single-cell-change"),
        samples,
    );
}

#[test]
#[ignore = "性能基准，手动运行"]
fn bench_frame() {
    for (cols, rows) in [(120u16, 30u16), (200, 60)] {
        for scale in [1.0f64, 2.0] {
            bench_case(cols, rows, scale);
        }
    }
    println!(
        "注：帧缓冲为 Vec<u32>（0xFFRRGGBB），与 softbuffer 像素格式一致，render 内无额外格式转换。"
    );
}
