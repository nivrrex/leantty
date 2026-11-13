//! wgpu 设备/管线/回读封装。

use std::error::Error;
use std::sync::mpsc;

use crate::atlas::Atlas;
use crate::shaders;

/// 纯色矩形实例（背景/样式线/光标；布局须与 shader 的 BgInstance 一致）。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BgInstance {
    pub rect: [f32; 4],
    pub color: [f32; 3],
    pub _pad: f32,
}

/// 字形实例（布局必须与 shader 的 GlyphInstance 一致）。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct GlyphInstance {
    pub rect: [f32; 4],
    pub cell: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 3],
    pub backdrop: [f32; 3],
    pub _pad: f32,
}

/// 把实例数组展开为上传字节（安全实现：逐 f32 to_le_bytes，不用 unsafe）。
fn bytes_of_bg(v: &[BgInstance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 32);
    for i in v {
        for f in i.rect.iter().chain(i.color.iter()).chain(std::iter::once(&i._pad)) {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

fn bytes_of_glyph(v: &[GlyphInstance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 80);
    for i in v {
        for f in i
            .rect
            .iter()
            .chain(i.cell.iter())
            .chain(i.uv.iter())
            .chain(i.color.iter())
            .chain(i.backdrop.iter())
            .chain(std::iter::once(&i._pad))
        {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

/// 设备信息（adapter.get_info 的快照）。
#[derive(Debug, Clone)]
pub struct Info {
    pub name: String,
    pub backend: wgpu::Backend,
    pub device_type: wgpu::DeviceType,
    pub driver: String,
    pub driver_info: String,
    pub api_version: String,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: Info,
    pub limits: wgpu::Limits,
}

impl Gpu {
    /// 创建实例并选择适配器；无可用物理设备时返回 Err（不 panic）。
    ///
    /// 判定"真实物理设备"：`device_type` 为 DiscreteGpu / IntegratedGpu / VirtualGpu，
    /// 且不来自 lavapipe（软件 Vulkan）；CPU 类型（lavapipe/llvmpipe）被显式拒绝，
    /// 以保证失败时不会被软件实现掩盖（阶段 4A 规则：不得用 lavapipe 数据代替真实 GPU）。
    pub fn new(index: Option<usize>, name_filter: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::empty(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
        if adapters.is_empty() {
            return Err("Vulkan 适配器枚举为空（无可用 ICD 或无真实设备）".into());
        }
        let mut chosen: Option<(usize, wgpu::Adapter)> = None;
        for (i, a) in adapters.into_iter().enumerate() {
            let info = a.get_info();
            // 跳过软件实现（lavapipe 报 Cpu，Mesa 的 llvmpipe Vulkan ICD 亦然）。
            if info.device_type == wgpu::DeviceType::Cpu {
                eprintln!("[wgpu] 跳过软件适配器: {}", info.name);
                continue;
            }
            if let Some(idx) = index {
                if i == idx {
                    chosen = Some((i, a));
                    break;
                }
                continue;
            }
            if let Some(f) = name_filter {
                if info.name.to_lowercase().contains(&f.to_lowercase()) {
                    chosen = Some((i, a));
                    break;
                }
                continue;
            }
            chosen = Some((i, a));
            break;
        }
        let (idx, adapter) = chosen.ok_or("未找到匹配的真实 GPU 适配器")?;
        eprintln!("[wgpu] 选中适配器 #{idx}");
        let info = adapter.get_info();
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu-spike"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))?;
        Ok(Self {
            device,
            queue,
            info: Info {
                name: info.name,
                backend: info.backend,
                device_type: info.device_type,
                driver: info.driver,
                driver_info: info.driver_info,
                // AdapterInfo 不暴露 apiVersion（wgpu 30）：Vulkan 版本由 T0 的
                // vulkaninfo 记录（1.2.175），此处留空以免臆造。
                api_version: "n/a(见 vulkaninfo)".to_string(),
            },
            limits,
        })
    }
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bg_pipe: wgpu::RenderPipeline,
    glyph_pipe: wgpu::RenderPipeline,
    screen_buf: wgpu::Buffer,
    bg_bind: wgpu::BindGroup,
    glyph_bind: wgpu::BindGroup,
    bg_inst: wgpu::Buffer,
    glyph_inst: wgpu::Buffer,
    bg_cap: u32,
    glyph_cap: u32,
    target: wgpu::Texture,
    readback: wgpu::Buffer,
    padded_row: u32,
    pixel: (u32, u32),
}

impl Renderer {
    pub fn new(
        gpu: Gpu,
        atlas: &Atlas,
        pixel_w: u32,
        pixel_h: u32,
    ) -> Result<Self, Box<dyn Error>> {
        let Gpu {
            device,
            queue,
            limits,
            ..
        } = gpu;
        let _ = limits;

        // 统一缓冲：屏幕尺寸（像素）
        let screen_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &screen_buf,
            0,
            &[pixel_w as f32, pixel_h as f32, 0.0, 0.0]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect::<Vec<u8>>(),
        );

        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bg_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bg"),
            layout: &bg_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buf.as_entire_binding(),
            }],
        });

        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.width),
                rows_per_image: Some(atlas.height),
            },
            wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas-sampler"),
            // 与 CPU 的"像素一一对应"对齐：最近邻、无 mip、clamp。
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let glyph_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let glyph_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph-bg"),
            layout: &glyph_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let bg_pipe = build_pipeline(
            &device,
            &bg_layout,
            shaders::BG_WGSL,
            None,
            false,
            std::mem::size_of::<BgInstance>() as u64,
            "bg",
        );
        // 字形不启用 alpha 混合：shader 直接输出 mix(backdrop, fg, coverage)，
        // 与 CPU 侧 blend_gamma 的语义一致（比 over 混合更贴合，且不受画布内容影响）。
        let glyph_pipe = build_pipeline(
            &device,
            &glyph_layout,
            shaders::GLYPH_WGSL,
            None,
            true,
            std::mem::size_of::<GlyphInstance>() as u64,
            "glyph",
        );

        // 实例缓冲容量：单元格数 + 字形数上限
        let bg_cap = (pixel_w / 4 * 4).max(1024) * 8;
        let glyph_cap = bg_cap;
        let bg_inst = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg-instances"),
            size: u64::from(bg_cap) * std::mem::size_of::<BgInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glyph_inst = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph-instances"),
            size: u64::from(glyph_cap) * std::mem::size_of::<GlyphInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width: pixel_w,
                height: pixel_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // 非 sRGB：与 CPU 帧缓冲的"直接用存储值"语义一致（避免色彩空间转换）。
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let padded_row = align_to(pixel_w * 4, 256);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded_row) * u64::from(pixel_h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self {
            device,
            queue,
            bg_pipe,
            glyph_pipe,
            screen_buf,
            bg_bind,
            glyph_bind,
            bg_inst,
            glyph_inst,
            bg_cap,
            glyph_cap,
            target,
            readback,
            padded_row,
            pixel: (pixel_w, pixel_h),
        })
    }

    /// 提交一帧渲染并等待 GPU 完成（不包含回读）。
    ///
    /// `quads` 顺序：背景 → 样式线 → 光标填充；`glyphs` 顺序：普通字形 → 光标格字符。
    /// `bg_n`/`glyph_n` 为分区边界，绘制顺序复刻 CPU（字形盖背景、样式线与光标盖字形、
    /// 光标字符最后）。
    pub fn submit_render(
        &mut self,
        instances: &crate::Instances,
    ) -> Result<(), Box<dyn Error>> {
        let bgs = &instances.quads;
        let glyphs = &instances.glyphs;
        let bg_n = instances.bg_n.min(bgs.len() as u32);
        let glyph_n = instances.glyph_n.min(glyphs.len() as u32);
        if bgs.len() as u32 > self.bg_cap || glyphs.len() as u32 > self.glyph_cap {
            return Err(format!(
                "实例数超容量：bg {} / cap {}，glyph {} / cap {}",
                bgs.len(),
                self.bg_cap,
                glyphs.len(),
                self.glyph_cap
            )
            .into());
        }
        self.queue.write_buffer(&self.bg_inst, 0, &bytes_of_bg(bgs));
        if !glyphs.is_empty() {
            self.queue
                .write_buffer(&self.glyph_inst, 0, &bytes_of_glyph(glyphs));
        }
        let _ = &self.screen_buf;

        let view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // 绘制顺序复刻 CPU：背景 → 字形 → 样式线/光标填充 → 光标字符。
            // quads 前 bg_n 个是背景，其后是样式线与光标填充；glyphs 前 glyph_n 个是
            // 普通字形，其后是光标格字符。
            pass.set_bind_group(0, &self.bg_bind, &[]);
            pass.set_vertex_buffer(0, self.bg_inst.slice(..));
            pass.set_pipeline(&self.bg_pipe);
            pass.draw(0..6, 0..bg_n);
            if !glyphs.is_empty() {
                pass.set_pipeline(&self.glyph_pipe);
                pass.set_bind_group(0, &self.glyph_bind, &[]);
                pass.set_vertex_buffer(0, self.glyph_inst.slice(..));
                pass.draw(0..6, 0..glyph_n);
                // 样式线与光标填充（在字形之上）
                pass.set_pipeline(&self.bg_pipe);
                pass.set_bind_group(0, &self.bg_bind, &[]);
                pass.set_vertex_buffer(0, self.bg_inst.slice(..));
                pass.draw(0..6, bg_n..bgs.len() as u32);
                if glyphs.len() as u32 > glyph_n {
                    pass.set_pipeline(&self.glyph_pipe);
                    pass.set_bind_group(0, &self.glyph_bind, &[]);
                    pass.set_vertex_buffer(0, self.glyph_inst.slice(..));
                    pass.draw(0..6, glyph_n..glyphs.len() as u32);
                }
            } else if bgs.len() as u32 > bg_n {
                pass.set_pipeline(&self.bg_pipe);
                pass.set_vertex_buffer(0, self.bg_inst.slice(..));
                pass.draw(0..6, bg_n..bgs.len() as u32);
            }
        }
        let buf = enc.finish();
        self.queue.submit([buf]);
        self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;
        Ok(())
    }

    /// 读回最后一帧渲染结果（RGBA8 紧凑排布）。
    pub fn read_back(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let (w, h) = self.pixel;
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let buf = enc.finish();
        self.queue.submit([buf]);

        let (tx, rx) = mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r.is_ok());
            });
        self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;
        match rx.recv() {
            Ok(true) => {}
            Ok(false) => return Err("缓冲区映射失败".into()),
            Err(e) => return Err(format!("映射回调通道断开: {e}").into()),
        }
        let data = self.readback.slice(..).get_mapped_range()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let start = (y * self.padded_row) as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        drop(data);
        self.readback.unmap();
        Ok(out)
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    wgsl: &str,
    blend: Option<wgpu::BlendState>,
    is_glyph: bool,
    stride: u64,
    label: &str,
) -> wgpu::RenderPipeline {
    let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &sm,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            // 属性/布局需 'static：spike 每类管线只建一次，泄漏一块小数组可接受。
            buffers: Box::leak(
                vec![Some(wgpu::VertexBufferLayout {
                    array_stride: stride,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: Box::leak(vertex_attrs(is_glyph).into_boxed_slice()),
                })]
                .into_boxed_slice(),
            ),
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &sm,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn vertex_attrs(glyph: bool) -> Vec<wgpu::VertexAttribute> {
    let mut v = vec![
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x4,
        },
    ];
    if glyph {
        v.push(wgpu::VertexAttribute {
            offset: 16,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x4,
        });
        v.push(wgpu::VertexAttribute {
            offset: 32,
            shader_location: 2,
            format: wgpu::VertexFormat::Float32x4,
        });
        v.push(wgpu::VertexAttribute {
            offset: 48,
            shader_location: 3,
            format: wgpu::VertexFormat::Float32x3,
        });
        v.push(wgpu::VertexAttribute {
            offset: 60,
            shader_location: 4,
            format: wgpu::VertexFormat::Float32x3,
        });
    } else {
        v.push(wgpu::VertexAttribute {
            offset: 16,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x3,
        });
    }
    v
}

fn align_to(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}
