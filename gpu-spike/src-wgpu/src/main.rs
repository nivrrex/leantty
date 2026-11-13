//! wgpu(Vulkan) 离屏渲染 spike：实例化 quad 背景 + fontdue R8 图集字形，回读 RGBA。
//!
//! 只做验证与测量，不接入主程序。所有 GPU 操作均在离屏纹理上进行，
//! 不创建任何窗口/surface（:99 的 surface 测试由 `--surface` 单独入口覆盖）。

use std::error::Error;
use std::time::Instant;

mod gpu;
mod report;
mod shaders;
mod surface;

use spike_common::atlas::Atlas;
use spike_common::instances::build_instances;
use gpu::Gpu;

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
            "--surface" => {
                cfg.surface = true;
                i += 1;
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
    surface: bool,
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
            surface: false,
            json: false,
            diag: false,
            dump_cell: None,
        }
    }
}

fn run(cfg: &Config) -> Result<(), Box<dyn Error>> {
    if cfg.surface {
        // T4b：:99 下的 X11 surface present（默认 10 帧，沿用 --frames）。
        return surface::run_surface(cfg.frames.min(10));
    }
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
        spike_common::dump_cell(&cpu_fb, &last_rgba, pixel_w, (cw, ch), dcx, dcy);
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
