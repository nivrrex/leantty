//! 动态字形图集（R8）：键 =（字体组，码点，像素字号，缩放）。
//!
//! 增长策略（阶段 4B t3）：
//! - 初始 `INITIAL_SIZE`，放不下时按 2 倍增长，直到设备 `max_texture_dimension_2d`；
//! - 达到上限仍满 → 清空（全部字形 + 像素缓冲）并重新上传当前帧所需字形，
//!   保证画面正确（不 panic、不丢字形）。
//! - 像素缓冲常驻内存，重建纹理时直接整块重传（避免重新光栅化）。

use std::collections::HashMap;

/// 图集初始边长（阶段 4B 任务要求）。
pub const INITIAL_SIZE: u32 = 2048;
/// 每个字形四周留 1px 透明保护边（最近邻采样可能取到相邻 texel）。
const PAD: u32 = 1;

/// 图集键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtlasKey {
    /// 字体组（0=normal, 1=bold, 2=italic, 3=bold-italic）。
    pub group: u8,
    /// 码点。
    pub ch: char,
    /// 像素字号（向上取整；与 CPU 的 glyph_cache 键一致）。
    pub px: u32,
    /// 缩放因子位模式（同一字号在 1x/2x 下位图不同）。
    pub scale_bits: u64,
}

/// 图集中的一个字形槽位。
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// 图集统计（压力测试断言用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasStats {
    pub size: u32,
    pub glyphs: usize,
    /// 因放不下而增长/重建的次数。
    pub grows: u32,
    /// 达到上限后清空重建的次数。
    pub resets: u32,
}

pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    size: u32,
    max_size: u32,
    pixels: Vec<u8>,
    map: HashMap<AtlasKey, Slot>,
    shelf_x: u32,
    shelf_y: u32,
    row_h: u32,
    grows: u32,
    resets: u32,
}

impl GlyphAtlas {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        max_texture: u32,
        initial: u32,
    ) -> Self {
        let size = initial.min(max_texture).max(64);
        let (texture, view) = create_texture(device, size);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let _ = queue;
        Self {
            texture,
            view,
            sampler,
            size,
            max_size: max_texture.max(size),
            pixels: vec![0u8; (size * size) as usize],
            map: HashMap::new(),
            shelf_x: 0,
            shelf_y: 0,
            row_h: 0,
            grows: 0,
            resets: 0,
        }
    }

    pub fn stats(&self) -> AtlasStats {
        AtlasStats {
            size: self.size,
            glyphs: self.map.len(),
            grows: self.grows,
            resets: self.resets,
        }
    }

    /// 取得字形 uv；不存在则分配并上传。空间不足时增长或清空重建。
    ///
    /// 返回 None 表示尺寸为 0（调用方跳过该字形）。
    pub fn get_or_insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: AtlasKey,
        w: u32,
        h: u32,
        data: &[u8],
    ) -> Option<[f32; 4]> {
        if w == 0 || h == 0 {
            return None;
        }
        if let Some(s) = self.map.get(&key) {
            return Some([s.u0, s.v0, s.u1, s.v1]);
        }
        loop {
            match self.allocate(w, h) {
                Some((x0, y0)) => {
                    // 写入像素（含保护边留空）。
                    for gy in 0..h {
                        let src = (gy * w) as usize;
                        let dst = ((y0 + gy) * self.size + x0) as usize;
                        self.pixels[dst..dst + w as usize]
                            .copy_from_slice(&data[src..src + w as usize]);
                    }
                    self.upload_region(queue, x0, y0, w, h);
                    let slot = Slot {
                        u0: x0 as f32 / self.size as f32,
                        v0: y0 as f32 / self.size as f32,
                        u1: (x0 + w) as f32 / self.size as f32,
                        v1: (y0 + h) as f32 / self.size as f32,
                    };
                    self.map.insert(key, slot);
                    return Some([slot.u0, slot.v0, slot.u1, slot.v1]);
                }
                None => {
                    // 放不下：先尝试增长，增长到上限仍不行则清空重建。
                    if self.size < self.max_size {
                        self.grow(device, queue);
                        self.grows += 1;
                    } else {
                        self.reset(device, queue);
                        self.resets += 1;
                    }
                }
            }
        }
    }

    /// shelf 分配：返回内容左上角（含保护边偏移）。
    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + 2 * PAD > self.size {
            return None;
        }
        if self.shelf_x + w + 2 * PAD > self.size {
            // 换行
            self.shelf_x = 0;
            self.shelf_y += self.row_h;
            self.row_h = 0;
        }
        if self.shelf_y + h + 2 * PAD > self.size {
            return None;
        }
        let x0 = self.shelf_x + PAD;
        let y0 = self.shelf_y + PAD;
        self.shelf_x = x0 + w + PAD;
        self.row_h = self.row_h.max(h + 2 * PAD);
        Some((x0, y0))
    }

    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let new_size = (self.size * 2).min(self.max_size);
        if new_size == self.size {
            return;
        }
        // 简单策略：增长后重排全部已缓存字形（像素缓冲扩容后整块写入）。
        let mut old = std::mem::take(&mut self.pixels);
        let mut map = std::mem::take(&mut self.map);
        let old_size = self.size;
        self.size = new_size;
        self.pixels = vec![0u8; (new_size * new_size) as usize];
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.row_h = 0;
        // 记录每个槽位的尺寸（由 uv 反推），逐个重排。
        let entries: Vec<(AtlasKey, u32, u32, Vec<u8>)> = map
            .drain()
            .map(|(k, s)| {
                let w = ((s.u1 - s.u0) * old_size as f32).round() as u32;
                let h = ((s.v1 - s.v0) * old_size as f32).round() as u32;
                let x0 = (s.u0 * old_size as f32).round() as u32;
                let y0 = (s.v0 * old_size as f32).round() as u32;
                let mut px = vec![0u8; (w * h) as usize];
                for gy in 0..h {
                    let src = ((y0 + gy) * old_size + x0) as usize;
                    px[(gy * w) as usize..((gy + 1) * w) as usize]
                        .copy_from_slice(&old[src..src + w as usize]);
                }
                (k, w, h, px)
            })
            .collect();
        old.clear();
        let (texture, view) = create_texture(device, self.size);
        self.texture = texture;
        self.view = view;
        for (k, w, h, px) in entries {
            if let Some((x0, y0)) = self.allocate(w, h) {
                for gy in 0..h {
                    let src = (gy * w) as usize;
                    let dst = ((y0 + gy) * self.size + x0) as usize;
                    self.pixels[dst..dst + w as usize]
                        .copy_from_slice(&px[src..src + w as usize]);
                }
                self.map.insert(
                    k,
                    Slot {
                        u0: x0 as f32 / self.size as f32,
                        v0: y0 as f32 / self.size as f32,
                        u1: (x0 + w) as f32 / self.size as f32,
                        v1: (y0 + h) as f32 / self.size as f32,
                    },
                );
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size),
                rows_per_image: Some(self.size),
            },
            wgpu::Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
        );
    }

    /// 达到上限后清空：丢弃全部字形，调用方在本帧重新插入所需的。
    fn reset(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.map.clear();
        self.pixels.fill(0);
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.row_h = 0;
        let (texture, view) = create_texture(device, self.size);
        self.texture = texture;
        self.view = view;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size),
                rows_per_image: Some(self.size),
            },
            wgpu::Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
        );
    }

    fn upload_region(&self, queue: &wgpu::Queue, x0: u32, y0: u32, w: u32, h: u32) {
        let mut region = Vec::with_capacity((w * h) as usize);
        for gy in 0..h {
            let src = ((y0 + gy) * self.size + x0) as usize;
            region.extend_from_slice(&self.pixels[src..src + w as usize]);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: x0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &region,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn create_texture(device: &wgpu::Device, size: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
