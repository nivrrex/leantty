//! wgpu(Vulkan) 离屏渲染 spike：实例化 quad 背景 + fontdue R8 图集字形，回读 RGBA。
//!
//! 只做验证与测量，不接入主程序。所有 GPU 操作均在离屏纹理上进行，
//! 不创建任何窗口/surface（:99 的 surface 测试由 `--surface` 单独入口覆盖）。

use std::error::Error;
use std::time::{Duration, Instant};

mod atlas;
mod gpu;
mod report;
mod shaders;

use atlas::Atlas;
use gpu::{BgInstance, GlyphInstance, Gpu};

/// 一帧的实例数据（分区边界见 `gpu::Renderer::submit_render`）。
pub struct Instances {
    pub quads: Vec<BgInstance>,
    pub glyphs: Vec<GlyphInstance>,
    /// quads 中前 `bg_n` 个是单元格背景（其后为样式线与光标填充）。
    pub bg_n: u32,
    /// glyphs 中前 `glyph_n` 个是普通字形（其后为光标格字符）。
    pub glyph_n: u32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cols" => {
                cfg.cols = args.get(i + 1).ok_or("--cols 需要值")?.parse()?;
                i += 2;
            }
            "--rows" => {
                cfg.rows = args.get(i + 1).ok_or("--rows 需要值")?.parse()?;
                i += 2;
            }
            "--scale" => {
                cfg.scale = args.get(i + 1).ok_or("--scale 需要值")?.parse()?;
                i += 2;
            }
            "--frames" => {
                cfg.frames = args.get(i + 1).ok_or("--frames 需要值")?.parse()?;
                i += 2;
            }
            "--gpu-index" => {
                cfg.gpu_index = Some(args.get(i + 1).ok_or("--gpu-index 需要值")?.parse()?);
                i += 2;
            }
            "--gpu-name" => {
                cfg.gpu_name = Some(args.get(i + 1).ok_or("--gpu-name 需要值")?.clone());
                i += 2;
            }
            "--probe-only" => {
                cfg.probe_only = true;
                i += 1;
            }
            "--dump-cell" => {
                let v = args.get(i + 1).ok_or("--dump-cell 需要 cx,cy")?;
                let (a, b) = v.split_once(',').ok_or("--dump-cell 格式 cx,cy")?;
                cfg.dump_cell = Some((a.parse()?, b.parse()?));
                i += 2;
            }
            "--diag" => {
                cfg.diag = true;
                i += 1;
            }
            "--json" => {
                cfg.json = true;
                i += 1;
            }
            other => return Err(format!("未知参数: {other}").into()),
        }
    }
    run(&cfg)
}

#[derive(Debug, Clone)]
struct Config {
    cols: u16,
    rows: u16,
    scale: f64,
    frames: u32,
    gpu_index: Option<usize>,
    gpu_name: Option<String>,
    probe_only: bool,
    json: bool,
    diag: bool,
    dump_cell: Option<(u32, u32)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 30,
            scale: 1.0,
            frames: 300,
            gpu_index: None,
            gpu_name: None,
            probe_only: false,
            json: false,
            diag: false,
            dump_cell: None,
        }
    }
}

fn run(cfg: &Config) -> Result<(), Box<dyn Error>> {
    // 1) 适配器枚举（无真实设备时必须返回 Err 而非 panic —— T4a 的验收点）。
    let t = Instant::now();
    let gpu = Gpu::new(cfg.gpu_index, cfg.gpu_name.as_deref())?;
    let init_ms = t.elapsed().as_secs_f64() * 1000.0;
    let info = gpu.info.clone();
    eprintln!(
        "[wgpu] adapter='{}' backend={:?} type={:?} driver='{}' driver_info='{}'",
        info.name, info.backend, info.device_type, info.driver, info.driver_info
    );
    if cfg.probe_only {
        println!("adapter-ok name={} init_ms={init_ms:.3}", info.name);
        return Ok(());
    }

    // 2) 帧内容
    let snap = spike_common::build_frame(cfg.cols, cfg.rows).map_err(|e| e.to_string())?;

    // 3) 图集（含初始化计时）
    let t = Instant::now();
    let atlas = Atlas::build(&snap, cfg.scale)?;
    let atlas_ms = t.elapsed().as_secs_f64() * 1000.0;
    let (cw, ch) = atlas.cell;
    let pixel_w = u32::from(cfg.cols) * cw;
    let pixel_h = u32::from(cfg.rows) * ch;
    eprintln!(
        "[wgpu] glyphs={} cell={cw}x{ch} pixel={pixel_w}x{pixel_h}",
        atlas.glyph_count()
    );

    // 4) 管线/资源
    let t = Instant::now();
    let mut renderer = gpu::Renderer::new(gpu, &atlas, pixel_w, pixel_h)?;
    let pipeline_ms = t.elapsed().as_secs_f64() * 1000.0;

    let init_report = report::InitReport {
        instance_adapter_device_ms: init_ms,
        atlas_ms,
        pipeline_ms,
    };
    eprintln!(
        "[wgpu] init: instance+adapter+device={:.3}ms atlas={atlas_ms:.3}ms pipeline+resources={pipeline_ms:.3}ms",
        init_report.instance_adapter_device_ms
    );

    // 5) 逐帧测量
    let mut build_t = Vec::with_capacity(cfg.frames as usize);
    let mut submit_t = Vec::with_capacity(cfg.frames as usize);
    let mut readback_t = Vec::with_capacity(cfg.frames as usize);
    let mut last_rgba: Vec<u8> = Vec::new();
    for frame in 0..cfg.frames {
        let t = Instant::now();
        let instances = build_instances(&snap, &atlas, cw, ch)?;
        build_t.push(t.elapsed());

        let t = Instant::now();
        renderer.submit_render(&instances)?;
        submit_t.push(t.elapsed());

        let t = Instant::now();
        let rgba = renderer.read_back()?;
        readback_t.push(t.elapsed());
        if frame + 1 == cfg.frames {
            last_rgba = rgba;
        }
    }

    // 6) 像素对比（CPU 参考 vs GPU 最后一帧）
    let (cpu_fb, cpu_w, cpu_h, _cpu_cw, _cpu_ch) =
        spike_common::cpu_reference(&snap, pixel_w, pixel_h, cfg.scale).map_err(|e| e.to_string())?;
    if (cpu_w, cpu_h) != (pixel_w, pixel_h) {
        return Err(format!(
            "尺寸不一致：CPU {cpu_w}x{cpu_h} vs GPU {pixel_w}x{pixel_h}"
        )
        .into());
    }
    let diff = spike_common::compare(&cpu_fb, &last_rgba, pixel_w, pixel_h);
    if cfg.diag {
        let cells = spike_common::diff_by_cell(&cpu_fb, &last_rgba, pixel_w, pixel_h, (cw, ch), 12);
        eprintln!("[diag] 差异最多的 cell（cx, cy, diff_px, cpu_rgb, gpu_rgb, char）:");
        for (cx, cy, n, c, g) in cells {
            let ch0 = snap
                .cells
                .get(cy as usize)
                .and_then(|r| r.get(cx as usize))
                .map(|c| c.chars.clone())
                .unwrap_or_default();
            eprintln!("[diag]   ({cx:3},{cy:2}) n={n:5} cpu=#{c:06x} gpu=#{g:06x} char={ch0:?}");
        }
    }

    if let Some((dcx, dcy)) = cfg.dump_cell {
        dump_cell(&cpu_fb, &last_rgba, pixel_w, (cw, ch), dcx, dcy);
        dump_row_wide(&snap, dcy as usize);
    }

    // 7) CPU 侧单帧耗时对照（同快照、同尺寸、全量重绘）
    let mut cpu_frames = Vec::with_capacity(cfg.frames.min(60) as usize);
    {
        let mut r = spike_common::make_cpu_renderer(cfg.scale).map_err(|e| e.to_string())?;
        r.set_selection(Some(spike_common::SELECTION));
        for _ in 0..cfg.frames.min(60) {
            let t = Instant::now();
            spike_common::cpu_render(&mut r, &snap, pixel_w, pixel_h)
                .map_err(|e| e.to_string())?;
            cpu_frames.push(t.elapsed());
        }
    }

    let rep = report::RunReport {
        gpu: info,
        cols: cfg.cols,
        rows: cfg.rows,
        scale: cfg.scale,
        pixel: (pixel_w, pixel_h),
        cell: (cw, ch),
        init: init_report,
        frames: cfg.frames,
        build: spike_common::stats(build_t),
        submit: spike_common::stats(submit_t),
        readback: spike_common::stats(readback_t),
        diff,
        cpu_render: spike_common::stats(cpu_frames),
    };
    if cfg.json {
        println!("{}", rep.to_json());
    } else {
        rep.print();
    }
    Ok(())
}

/// 把 CPU 与 GPU 的同一单元格打成 ASCII 灰度图（差异定位用）。
fn dump_cell(cpu: &[u32], gpu: &[u8], w: u32, cell: (u32, u32), cx: u32, cy: u32) {
    let (cw, ch) = cell;
    let ramp = b" .:-=+*#%@";
    let mut a = String::new();
    let mut b = String::new();
    for y in 0..ch {
        for x in 0..cw {
            let px = cx * cw + x;
            let py = cy * ch + y;
            let i = (py * w + px) as usize;
            let c = cpu[i];
            let cg = ((c >> 16) & 0xFF) as usize;
            let gg = usize::from(gpu[i * 4]);
            a.push(ramp[(cg * 9 / 255).min(9)] as char);
            b.push(ramp[(gg * 9 / 255).min(9)] as char);
        }
        a.push('\n');
        b.push('\n');
    }
    eprintln!("[dump] cell ({cx},{cy}) CPU:\n{a}[dump] GPU:\n{b}");
}

/// 打印指定行的 wide/字符标记（诊断 wide 判定是否与 CPU 一致）。
fn dump_row_wide(snap: &vt::FrameSnapshot, y: usize) {
    if let Some(row) = snap.cells.get(y) {
        let items: Vec<String> = row
            .iter()
            .take(24)
            .enumerate()
            .map(|(x, c)| format!("{x}:{:?}[{:?}]", c.wide, c.chars))
            .collect();
        eprintln!("[dump] row {y}: {}", items.join(" "));
    }
}

/// 与 `render::group_for` 一致：按 bold/italic 选字体组。
fn group_for(cell: &vt::Cell) -> u8 {
    match (cell.bold, cell.italic) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 0,
    }
}

fn rgb3(c: vt::Rgb) -> [f32; 3] {
    [
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
    ]
}

fn dim(fg: vt::Rgb, bg: vt::Rgb) -> vt::Rgb {
    vt::Rgb {
        r: fg.r / 2 + bg.r / 2,
        g: fg.g / 2 + bg.g / 2,
        b: fg.b / 2 + bg.b / 2,
    }
}

/// CPU 侧构建实例数据：完整复刻 `CpuRenderer::draw_row` + `draw_style_lines` + `draw_cursor`
/// 的绘制顺序与颜色决策，使像素对比只反映"光栅化/混合实现的差异"，不含语义差异。
fn build_instances(
    snap: &vt::FrameSnapshot,
    atlas: &Atlas,
    cw: u32,
    ch: u32,
) -> Result<Instances, Box<dyn Error>> {
    let bg_default = atlas.default_bg();
    let fg_default = atlas.default_fg();
    let palette = render::Palette::default();
    let sel = spike_common::SELECTION;
    let n_cells: usize = snap.cells.iter().map(Vec::len).sum();
    let mut quads = Vec::with_capacity(n_cells * 2);
    let mut glyphs = Vec::with_capacity(n_cells);

    // --- 逐 cell：背景 → 字形 → 样式线 ---
    for (y, row) in snap.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let px0 = x as u32 * cw;
            let py0 = y as u32 * ch;
            let is_tail = matches!(
                cell.wide,
                vt::CellWide::SpacerTail | vt::CellWide::SpacerHead
            );
            let span = if cell.wide == vt::CellWide::Wide && x + 1 < row.len() {
                2
            } else {
                1
            };
            let area_w = cw * span;
            let in_sel = (x as u16) >= sel.0
                && (x as u16) <= sel.2
                && (y as u16) >= sel.1
                && (y as u16) <= sel.3;
            let sel_here = in_sel && !is_tail;

            let orig_bg = cell.bg.unwrap_or(bg_default);
            let orig_fg = cell.fg.unwrap_or(fg_default);
            let bg_fill = if sel_here { orig_fg } else { orig_bg };
            quads.push(BgInstance {
                rect: [px0 as f32, py0 as f32, area_w as f32, ch as f32],
                color: rgb3(bg_fill),
                _pad: 0.0,
            });

            if is_tail || cell.chars.is_empty() || cell.invisible {
                continue;
            }

            let mut fg = orig_fg;
            let mut bgc = bg_fill;
            if cell.inverse {
                std::mem::swap(&mut fg, &mut bgc);
            }
            if cell.faint {
                fg = dim(fg, bgc);
            }
            if sel_here {
                fg = orig_bg;
                bgc = orig_fg;
            }

            let ch0 = cell.chars.chars().next().ok_or("空字符意外")?;
            if let Some(g) = atlas
                .get(ch0, span as u8, group_for(cell))
                .filter(|g| g.w > 0 && g.h > 0)
            {
                glyphs.push(GlyphInstance {
                    rect: [
                        (px0 as i32 + g.xmin) as f32,
                        (py0 as i32 + g.ymin) as f32,
                        g.w as f32,
                        g.h as f32,
                    ],
                    // 裁剪到**起始格**而不是宽字符的 2 格宽度：CPU 侧 blit 的实际
                    // 可见范围就是单格（实测把裁剪放宽到 2 格会让续格多出 5-10 列，
                    // 与 CPU 输出不一致）。这是对齐参照实现的选择，不是物理正确性。
                    cell: [px0 as f32, py0 as f32, cw as f32, ch as f32],
                    uv: [g.u0, g.v0, g.u1, g.v1],
                    color: rgb3(fg),
                    backdrop: rgb3(bgc),
                    _pad: 0.0,
                });
            }

            // 样式线：颜色 = inverse ? bgc : fg（与 CPU 一致）
            let line = if cell.inverse { bgc } else { fg };
            push_style_lines(&mut quads, cell, px0, py0, area_w, ch, line);
        }
    }
    let bg_n = quads.len() as u32;
    let glyph_n = glyphs.len() as u32;

    // --- 光标（覆盖在全部内容之上）---
    let cur = &snap.cursor;
    if cur.visible {
        let cx = u32::from(cur.x) * cw;
        let cy = u32::from(cur.y) * ch;
        let color = rgb3(palette.cursor);
        let push = |quads: &mut Vec<BgInstance>, x: u32, y: u32, w: u32, h: u32, c: [f32; 3]| {
            quads.push(BgInstance {
                rect: [x as f32, y as f32, w as f32, h as f32],
                color: c,
                _pad: 0.0,
            });
        };
        match cur.style {
            vt::CursorStyle::Block => {
                push(&mut quads, cx, cy, cw, ch, color);
                // 光标格字符以 cursor_text 色重画（底=cursor 色）
                if let Some(cell) = snap
                    .cells
                    .get(cur.y as usize)
                    .and_then(|r| r.get(cur.x as usize))
                    && !cell.chars.is_empty()
                {
                    let ch0 = cell.chars.chars().next().ok_or("空字符意外")?;
                    if let Some(g) = atlas.get(ch0, 1, 0).filter(|g| g.w > 0 && g.h > 0) {
                        glyphs.push(GlyphInstance {
                            rect: [
                                (cx as i32 + g.xmin) as f32,
                                (cy as i32 + g.ymin) as f32,
                                g.w as f32,
                                g.h as f32,
                            ],
                            cell: [cx as f32, cy as f32, cw as f32, ch as f32],
                            uv: [g.u0, g.v0, g.u1, g.v1],
                            color: rgb3(palette.cursor_text),
                            backdrop: color,
                            _pad: 0.0,
                        });
                    }
                }
            }
            vt::CursorStyle::HollowBlock => {
                push(&mut quads, cx, cy, cw, 1, color);
                push(&mut quads, cx, cy + ch - 1, cw, 1, color);
                push(&mut quads, cx, cy, 1, ch, color);
                push(&mut quads, cx + cw - 1, cy, 1, ch, color);
            }
            vt::CursorStyle::Underline => {
                push(&mut quads, cx, cy + ch - 1, cw, 1, color);
                push(&mut quads, cx, cy + ch - 2, cw, 1, color);
            }
            vt::CursorStyle::Bar => {
                push(&mut quads, cx, cy, 1, ch, color);
                push(&mut quads, cx + 1, cy, 1, ch, color);
            }
        }
    }

    Ok(Instances {
        quads,
        glyphs,
        bg_n,
        glyph_n,
    })
}

/// 复刻 `render::draw_style_lines`：下划线（Single/Double/Dotted/Dashed/Curly→Single）、
/// 删除线（h/2）、上划线（y=1）。虚线按下标取模（i % gap == 0）逐像素点。
fn push_style_lines(
    quads: &mut Vec<BgInstance>,
    cell: &vt::Cell,
    px0: u32,
    py0: u32,
    w: u32,
    h: u32,
    color: vt::Rgb,
) {
    let c = rgb3(color);
    let hline = |quads: &mut Vec<BgInstance>, y: i32, gap: u32| {
        if y < 0 || y >= h as i32 {
            return;
        }
        let mut i = 0u32;
        while i < w {
            quads.push(BgInstance {
                rect: [(px0 + i) as f32, (py0 + y as u32) as f32, 1.0, 1.0],
                color: c,
                _pad: 0.0,
            });
            i += gap;
        }
    };
    match cell.underline {
        vt::Underline::None => {}
        vt::Underline::Single => hline(quads, h as i32 - 2, 1),
        vt::Underline::Double => {
            hline(quads, h as i32 - 2, 1);
            hline(quads, h as i32 - 4, 1);
        }
        vt::Underline::Dotted => hline(quads, h as i32 - 2, 2),
        vt::Underline::Dashed => hline(quads, h as i32 - 2, 4),
        vt::Underline::Curly => hline(quads, h as i32 - 2, 1),
    }
    if cell.strikethrough {
        hline(quads, h as i32 / 2, 1);
    }
    if cell.overline {
        hline(quads, 1, 1);
    }
}

#[allow(dead_code)]
fn secs(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
