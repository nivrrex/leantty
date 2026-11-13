//! OpenGL ES 离屏渲染 spike（glow + khronos-egl，无 DISPLAY 的 EGL surfaceless 上下文）。
//!
//! 与 wgpu spike 共用同一份实例数据（`spike_common::instances`）与同一张字形图集，
//! 只替换 GPU 提交路径，便于横向对比。
//!
//! 有界策略：3 条 EGL 上下文途径全部失败即停止并记录"无法自动验证"。

use std::error::Error;
use std::time::Instant;

use glow::HasContext as _;
use khronos_egl as egl;
use spike_common::atlas::Atlas;

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
            "--dump-cell" => {
                let v = args.get(i + 1).ok_or("--dump-cell 需要 cx,cy")?;
                let (a, b) = v.split_once(',').ok_or("--dump-cell 格式 cx,cy")?;
                cfg.dump_cell = Some((a.parse()?, b.parse()?));
                i += 2;
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
    json: bool,
    dump_cell: Option<(u32, u32)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 30,
            scale: 1.0,
            frames: 300,
            json: false,
            dump_cell: None,
        }
    }
}

/// EGL 上下文持有（Drop 顺序：先 GL 资源、再 context、最后 display 不动）。
struct EglCtx {
    egl: egl::DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    context: egl::Context,
    /// 报告用：走的是哪条途径 + 实际 GL_RENDERER。
    path: &'static str,
    renderer: String,
    version: String,
}

fn run(cfg: &Config) -> Result<(), Box<dyn Error>> {
    let t_init = Instant::now();
    let ctx = create_context()?;
    let egl = &ctx.egl;
    let gl = unsafe { glow::Context::from_loader_function(|s| loader(egl, s)) };

    let snap = spike_common::build_frame(cfg.cols, cfg.rows).map_err(|e| e.to_string())?;
    let t_atlas = Instant::now();
    let atlas = Atlas::build(&snap, cfg.scale)?;
    let atlas_ms = t_atlas.elapsed().as_secs_f64() * 1000.0;
    let (cw, ch) = atlas.cell;
    let (pw, ph) = (u32::from(cfg.cols) * cw, u32::from(cfg.rows) * ch);
    let init_ms = t_init.elapsed().as_secs_f64() * 1000.0;

    let t_pipe = Instant::now();
    let mut r = GlRenderer::new(&gl, &atlas, pw, ph)?;
    let pipeline_ms = t_pipe.elapsed().as_secs_f64() * 1000.0;

    let mut build_t = Vec::with_capacity(cfg.frames as usize);
    let mut submit_t = Vec::with_capacity(cfg.frames as usize);
    let mut readback_t = Vec::with_capacity(cfg.frames as usize);
    let mut last: Vec<u8> = Vec::new();
    for frame in 0..cfg.frames {
        let t = Instant::now();
        let inst =
            spike_common::instances::build_instances(&snap, &atlas, cw, ch).map_err(|e| e.to_string())?;
        build_t.push(t.elapsed());

        let t = Instant::now();
        r.draw(&gl, &inst)?;
        submit_t.push(t.elapsed());

        let t = Instant::now();
        let rgba = r.read_back(&gl)?;
        readback_t.push(t.elapsed());
        if frame + 1 == cfg.frames {
            last = rgba;
        }
    }

    let (cpu_fb, cpu_w, cpu_h, _, _) =
        spike_common::cpu_reference(&snap, pw, ph, cfg.scale).map_err(|e| e.to_string())?;
    if (cpu_w, cpu_h) != (pw, ph) {
        return Err(format!("尺寸不一致：CPU {cpu_w}x{cpu_h} vs GL {pw}x{ph}").into());
    }
    let diff = spike_common::compare(&cpu_fb, &last, pw, ph);

    if let Some((dcx, dcy)) = cfg.dump_cell {
        spike_common::dump_cell(&cpu_fb, &last, pw, (cw, ch), dcx, dcy);
    }

    let mut cpu_frames = Vec::with_capacity(cfg.frames.min(60) as usize);
    {
        let mut cr = spike_common::make_cpu_renderer(cfg.scale).map_err(|e| e.to_string())?;
        cr.set_selection(Some(spike_common::SELECTION));
        for _ in 0..cfg.frames.min(60) {
            let t = Instant::now();
            spike_common::cpu_render(&mut cr, &snap, pw, ph).map_err(|e| e.to_string())?;
            cpu_frames.push(t.elapsed());
        }
    }

    let (build, submit, readback, cpu) = (
        spike_common::stats(build_t),
        spike_common::stats(submit_t),
        spike_common::stats(readback_t),
        spike_common::stats(cpu_frames),
    );
    if cfg.json {
        println!(
            concat!(
                "{{\"impl\":\"gl\",\"gpu\":\"{}\",\"path\":\"{}\",\"version\":\"{}\",",
                "\"cols\":{},\"rows\":{},\"scale\":{},\"pw\":{},\"ph\":{},\"cw\":{},\"ch\":{},\"frames\":{},",
                "\"init_total_ms\":{:.3},\"init_atlas_ms\":{:.3},\"init_pipeline_ms\":{:.3},",
                "\"build_med\":{:.3},\"build_p95\":{:.3},\"submit_med\":{:.3},\"submit_p95\":{:.3},",
                "\"readback_med\":{:.3},\"readback_p95\":{:.3},\"cpu_med\":{:.3},\"cpu_p95\":{:.3},",
                "\"diff_pixels\":{},\"bg_diff\":{},\"glyph_diff\":{},\"glyph_total\":{},",
                "\"max_channel_diff\":{},\"glyph_max_channel_diff\":{}}}"
            ),
            ctx.renderer,
            ctx.path,
            ctx.version,
            cfg.cols,
            cfg.rows,
            cfg.scale,
            pw,
            ph,
            cw,
            ch,
            cfg.frames,
            init_ms,
            atlas_ms,
            pipeline_ms,
            build.median_ms,
            build.p95_ms,
            submit.median_ms,
            submit.p95_ms,
            readback.median_ms,
            readback.p95_ms,
            cpu.median_ms,
            cpu.p95_ms,
            diff.diff_pixels,
            diff.bg_diff_pixels,
            diff.glyph_diff_pixels,
            diff.glyph_pixels,
            diff.max_channel_diff,
            diff.glyph_max_channel_diff
        );
    } else {
        println!("== spike-gl (OpenGL ES via EGL) ==");
        println!("path={} GL_VERSION='{}' GL_RENDERER='{}'", ctx.path, ctx.version, ctx.renderer);
        println!("size={}x{} scale={} pixel={pw}x{ph} cell={cw}x{ch} frames={}", cfg.cols, cfg.rows, cfg.scale, cfg.frames);
        println!(
            "init: ctx+atlas={init_ms:.1}ms (atlas {atlas_ms:.1}ms) pipeline+resources={pipeline_ms:.1}ms"
        );
        println!(
            "per-frame: build med={:.3} p95={:.3} | draw+finish med={:.3} p95={:.3} | readback med={:.3} p95={:.3} (ms)",
            build.median_ms, build.p95_ms, submit.median_ms, submit.p95_ms, readback.median_ms, readback.p95_ms
        );
        println!(
            "cpu-render (同快照同尺寸): med={:.3} p95={:.3} ms",
            cpu.median_ms, cpu.p95_ms
        );
        println!(
            "pixel-diff vs CPU: total={} diff={} (bg={} glyph={}/{} glyph_total) max_channel={} glyph_max_channel={}",
            diff.total, diff.diff_pixels, diff.bg_diff_pixels, diff.glyph_diff_pixels,
            diff.glyph_pixels, diff.max_channel_diff, diff.glyph_max_channel_diff
        );
    }
    // 不显式 drop(r)：GL 资源随上下文销毁由驱动回收（spike 进程随即退出）。
    let _ = &r;
    egl.make_current(ctx.display, None, None, None)?;
    egl.destroy_context(ctx.display, ctx.context)?;
    Ok(())
}

/// 三条 EGL 途径：surfaceless-Mesa → device-platform → default-display。
fn create_context() -> Result<EglCtx, Box<dyn Error>> {
    // SAFETY: 动态加载系统 libEGL，失败返回错误。
    let egl = unsafe { egl::DynamicInstance::<egl::EGL1_5>::load_required()? };
    const EGL_PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;
    const EGL_PLATFORM_DEVICE_EXT: egl::Enum = 0x313F;
    let tries: [(&'static str, egl::Display); 3] = [
        (
            "surfaceless-mesa",
            unsafe {
                egl.get_platform_display(
                    EGL_PLATFORM_SURFACELESS_MESA,
                    std::ptr::null_mut(),
                    &[egl::NONE as usize],
                )?
            },
        ),
        (
            "device-platform",
            unsafe {
                egl.get_platform_display(
                    EGL_PLATFORM_DEVICE_EXT,
                    std::ptr::null_mut(),
                    &[egl::NONE as usize],
                )?
            },
        ),
        (
            "default-display",
            unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }.ok_or("eglGetDisplay 返回空")?,
        ),
    ];
    let mut last_err = String::new();
    for (name, display) in tries {
        match init_display(&egl, display, name) {
            Ok((context, renderer, version)) => {
                return Ok(EglCtx {
                    egl,
                    display,
                    context,
                    path: name,
                    renderer,
                    version,
                });
            }
            Err(e) => {
                eprintln!("[gl] 途径 {name} 失败: {e}");
                last_err = format!("{name}: {e}");
            }
        }
    }
    Err(format!("三条 EGL 无表面上下文途径全部失败（最后错误 {last_err}）").into())
}

fn init_display(
    egl: &egl::DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    path: &str,
) -> Result<(egl::Context, String, String), Box<dyn Error>> {
    egl.initialize(display)?;
    let config = egl
        .choose_first_config(display, &config_attribs())?
        .ok_or("无匹配 EGLConfig")?;
    // GLES2：无 DISPLAY 时 Mesa EGL 通常只提供 GLES context（桌面 GL 需 X）。
    let ctx_attribs = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];
    let context = egl
        .create_context(display, config, None, &ctx_attribs)
        .map_err(|e| format!("create_context: {e}"))?;
    if egl
        .make_current(display, None, None, Some(context))
        .is_err()
    {
        // 退回 1x1 pbuffer surface（仍不需要 DISPLAY）。
        let pb = egl.create_pbuffer_surface(
            display,
            config,
            &[
                egl::WIDTH,
                1,
                egl::HEIGHT,
                1,
                egl::NONE,
            ],
        )?;
        egl.make_current(display, Some(pb), Some(pb), Some(context))?;
    }
    let gl = unsafe { glow::Context::from_loader_function(|s| loader(egl, s)) };
    let (version, renderer) = unsafe {
        (
            gl.get_parameter_string(glow::VERSION),
            gl.get_parameter_string(glow::RENDERER),
        )
    };
    eprintln!("[gl] 途径 {path} 成功：{version} / {renderer}");
    Ok((context, renderer, version))
}

fn config_attribs() -> [i32; 15] {
    [
        egl::SURFACE_TYPE,
        egl::PBUFFER_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_ES2_BIT,
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        8,
        egl::NONE,
        egl::NONE,
        egl::NONE,
    ]
}

unsafe fn loader(egl: &egl::DynamicInstance<egl::EGL1_5>, name: &str) -> *const std::ffi::c_void {
    match egl.get_proc_address(name) {
        Some(f) => f as *const std::ffi::c_void,
        None => std::ptr::null(),
    }
}

// ---------------- GL 渲染 ----------------

struct GlRenderer {
    program: glow::Program,
    glyph_program: glow::Program,
    quad_vao: glow::VertexArray,
    quad_vbo: glow::Buffer,
    glyph_vao: glow::VertexArray,
    glyph_vbo: glow::Buffer,
    atlas_tex: glow::Texture,
    fbo: glow::Framebuffer,
    /// 颜色附件纹理（FBO 持有引用；字段保留以便明确生命周期）。
    #[allow(dead_code)]
    color_tex: glow::Texture,
    pixel: (u32, u32),
    quad_cap: usize,
    glyph_cap: usize,
}

const VS_QUAD: &str = r#"#version 300 es
layout(location=0) in vec4 rect;
layout(location=1) in vec3 color;
uniform vec2 screen;
out vec3 v_color;
void main() {
    vec2 c = vec2(float((gl_VertexID & 1) == 1), float((gl_VertexID >> 1) == 1));
    vec2 p = rect.xy + c * rect.zw;
    gl_Position = vec4(p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0, 0.0, 1.0);
    v_color = color;
}
"#;

const FS_QUAD: &str = r#"#version 300 es
precision highp float;
in vec3 v_color;
out vec4 out_color;
void main() { out_color = vec4(v_color, 1.0); }
"#;

const VS_GLYPH: &str = r#"#version 300 es
layout(location=0) in vec4 rect;
layout(location=1) in vec4 cell;
layout(location=2) in vec4 uv;
layout(location=3) in vec3 color;
layout(location=4) in vec3 backdrop;
uniform vec2 screen;
out vec3 v_color;
out vec3 v_backdrop;
out vec2 v_uv;
out vec2 v_page;
out vec2 v_cell_min;
out vec2 v_cell_max;
void main() {
    vec2 c = vec2(float((gl_VertexID & 1) == 1), float((gl_VertexID >> 1) == 1));
    vec2 p = rect.xy + c * rect.zw;
    gl_Position = vec4(p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0, 0.0, 1.0);
    v_color = color;
    v_backdrop = backdrop;
    v_uv = mix(uv.xy, uv.zw, c);
    v_page = p;
    v_cell_min = cell.xy;
    v_cell_max = cell.xy + cell.zw;
}
"#;

const FS_GLYPH: &str = r#"#version 300 es
precision highp float;
in vec3 v_color;
in vec3 v_backdrop;
in vec2 v_uv;
in vec2 v_page;
in vec2 v_cell_min;
in vec2 v_cell_max;
uniform sampler2D atlas;
uniform vec2 screen;
out vec4 out_color;
void main() {
    // 用 gl_FragCoord（像素中心，原点左下）换算成"y 向下"的页面坐标做单元格裁剪，
    // 避免 varying 插值在边缘处的偏差（实测用 varying 会把字形右侧/底部多裁 1-2 行）。
    vec2 page = vec2(gl_FragCoord.x, screen.y - gl_FragCoord.y);
    if (page.x < v_cell_min.x || page.x >= v_cell_max.x ||
        page.y < v_cell_min.y || page.y >= v_cell_max.y) {
        discard;
    }
    float a = texture(atlas, v_uv).r;
    if (a == 0.0) { discard; }
    out_color = vec4(mix(v_backdrop, v_color, a), 1.0);
}
"#;

impl GlRenderer {
    fn new(
        gl: &glow::Context,
        atlas: &Atlas,
        pw: u32,
        ph: u32,
    ) -> Result<Self, Box<dyn Error>> {
        unsafe {
            let quad_program = link(gl, VS_QUAD, FS_QUAD)?;
            let glyph_program = link(gl, VS_GLYPH, FS_GLYPH)?;

            let atlas_tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                atlas.width as i32,
                atlas.height as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&atlas.pixels)),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);

            let color_tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(color_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                pw as i32,
                ph as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            let fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(color_tex),
                0,
            );
            let st = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if st != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("FBO 不完整 0x{st:x}").into());
            }
            gl.viewport(0, 0, pw as i32, ph as i32);

            // quad: [rect vec4][color vec3] stride 28
            let (quad_vao, quad_vbo) = make_vao(gl)?;
            gl.bind_vertex_array(Some(quad_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            set_attr(gl, 0, 4, 28, 0);
            set_attr(gl, 1, 3, 28, 16);
            // glyph: [rect][cell][uv][color vec3][backdrop vec3]
            // = (4+4+4+3+3) 个 f32 = 72 字节 stride（写 68 会让每个实例错位 4 字节）
            let (glyph_vao, glyph_vbo) = make_vao(gl)?;
            gl.bind_vertex_array(Some(glyph_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(glyph_vbo));
            set_attr(gl, 0, 4, 72, 0);
            set_attr(gl, 1, 4, 72, 16);
            set_attr(gl, 2, 4, 72, 32);
            set_attr(gl, 3, 3, 72, 48);
            set_attr(gl, 4, 3, 72, 60);
            Ok(Self {
                program: quad_program,
                glyph_program,
                quad_vao,
                quad_vbo,
                glyph_vao,
                glyph_vbo,
                atlas_tex,
                fbo,
                color_tex,
                pixel: (pw, ph),
                quad_cap: 0,
                glyph_cap: 0,
            })
        }
    }

    fn draw(
        &mut self,
        gl: &glow::Context,
        inst: &spike_common::instances::Instances,
    ) -> Result<(), Box<dyn Error>> {
        let (pw, ph) = self.pixel;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.viewport(0, 0, pw as i32, ph as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(self.program));
            let screen = gl.get_uniform_location(self.program, "screen");
            gl.uniform_2_f32(screen.as_ref(), pw as f32, ph as f32);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));

            // 背景
            self.upload_quads(gl, &inst.quads, 0, inst.bg_n as usize)?;
            draw_instances(gl, self.quad_vao, inst.bg_n as usize);

            // 普通字形
            gl.use_program(Some(self.glyph_program));
            gl.uniform_2_f32(
                gl.get_uniform_location(self.glyph_program, "screen").as_ref(),
                pw as f32,
                ph as f32,
            );
            gl.uniform_1_i32(
                gl.get_uniform_location(self.glyph_program, "atlas").as_ref(),
                0,
            );
            self.upload_glyphs(gl, &inst.glyphs, 0, inst.glyph_n as usize)?;
            draw_instances(gl, self.glyph_vao, inst.glyph_n as usize);

            // 样式线 + 光标填充（回到 quad program）
            gl.use_program(Some(self.program));
            let line_n = inst.quads.len() - inst.bg_n as usize;
            self.upload_quads(gl, &inst.quads, inst.bg_n as usize, line_n)?;
            draw_instances(gl, self.quad_vao, line_n);

            // 光标字符
            let cur_n = inst.glyphs.len() - inst.glyph_n as usize;
            if cur_n > 0 {
                gl.use_program(Some(self.glyph_program));
                self.upload_glyphs(gl, &inst.glyphs, inst.glyph_n as usize, cur_n)?;
                draw_instances(gl, self.glyph_vao, cur_n);
            }
            gl.finish();
        }
        Ok(())
    }

    fn upload_quads(
        &mut self,
        gl: &glow::Context,
        quads: &[spike_common::instances::BgInstance],
        offset: usize,
        count: usize,
    ) -> Result<(), Box<dyn Error>> {
        let mut buf = Vec::with_capacity(count * 7);
        for q in &quads[offset..offset + count] {
            buf.extend_from_slice(&q.rect);
            buf.extend_from_slice(&q.color);
        }
        unsafe {
            gl.bind_vertex_array(Some(self.quad_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.quad_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, f32s(&buf), glow::DYNAMIC_DRAW);
        }
        self.quad_cap = count;
        Ok(())
    }

    fn upload_glyphs(
        &mut self,
        gl: &glow::Context,
        glyphs: &[spike_common::instances::GlyphInstance],
        offset: usize,
        count: usize,
    ) -> Result<(), Box<dyn Error>> {
        let mut buf = Vec::with_capacity(count * 18);
        for g in &glyphs[offset..offset + count] {
            buf.extend_from_slice(&g.rect);
            buf.extend_from_slice(&g.cell);
            buf.extend_from_slice(&g.uv);
            buf.extend_from_slice(&g.color);
            buf.extend_from_slice(&g.backdrop);
        }
        unsafe {
            gl.bind_vertex_array(Some(self.glyph_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.glyph_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, f32s(&buf), glow::DYNAMIC_DRAW);
        }
        if std::env::var("SPIKE_GL_DEBUG").is_ok() && offset == 0 && count > 0 {
            let g = &glyphs[0];
            eprintln!(
                "[gl-debug] glyph0 rect={:?} cell={:?} uv={:?} color={:?}",
                g.rect, g.cell, g.uv, g.color
            );
        }
        self.glyph_cap = count;
        Ok(())
    }

    fn read_back(&mut self, gl: &glow::Context) -> Result<Vec<u8>, Box<dyn Error>> {
        let (w, h) = self.pixel;
        let mut out = vec![0u8; (w * h * 4) as usize];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.read_buffer(glow::COLOR_ATTACHMENT0);
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut out)),
            );
        }
        // GL 的 read_pixels 原点在左下；渲染时 y 已按"向下为 +y"映射到 NDC
        // （`1 - y/h*2`），故数据需要逐行翻转回左上原点。
        // （若翻转方向判断相反，整体会上下颠倒，实测 diff 会从 1.5k 跳到 18k。）
        let stride = (w * 4) as usize;
        if std::env::var("SPIKE_GL_NOFLIP").is_ok() {
            return Ok(out);
        }
        let mut flipped = vec![0u8; out.len()];
        for y in 0..h as usize {
            let src = (h as usize - 1 - y) * stride;
            flipped[y * stride..(y + 1) * stride].copy_from_slice(&out[src..src + stride]);
        }
        Ok(flipped)
    }
}

fn f32s(v: &[f32]) -> &[u8] {
    // SAFETY: f32 无填充、对齐 4；仅用于上传顶点数据。
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

unsafe fn make_vao(gl: &glow::Context) -> Result<(glow::VertexArray, glow::Buffer), Box<dyn Error>> {
    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = gl.create_buffer()?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        Ok((vao, vbo))
    }
}

/// 设置一个实例化顶点属性（divisor=1）。
unsafe fn set_attr(gl: &glow::Context, loc: u32, size: i32, stride: i32, offset: i32) {
    unsafe {
        gl.enable_vertex_attrib_array(loc);
        gl.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, offset);
        gl.vertex_attrib_divisor(loc, 1);
    }
}

fn draw_instances(gl: &glow::Context, vao: glow::VertexArray, n: usize) {
    if n == 0 {
        return;
    }
    unsafe {
        gl.bind_vertex_array(Some(vao));
        gl.draw_arrays_instanced(glow::TRIANGLES, 0, 6, n as i32);
    }
}

unsafe fn link(
    gl: &glow::Context,
    vs: &str,
    fs: &str,
) -> Result<glow::Program, Box<dyn Error>> {
    unsafe {
        let p = gl.create_program()?;
        let mut shaders = Vec::new();
        for (ty, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let sh = gl.create_shader(ty)?;
            gl.shader_source(sh, src);
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(format!("shader 编译失败: {}", gl.get_shader_info_log(sh)).into());
            }
            gl.attach_shader(p, sh);
            shaders.push(sh);
        }
        gl.link_program(p);
        if !gl.get_program_link_status(p) {
            return Err(format!("program 链接失败: {}", gl.get_program_info_log(p)).into());
        }
        for sh in shaders {
            gl.detach_shader(p, sh);
            gl.delete_shader(sh);
        }
        Ok(p)
    }
}
