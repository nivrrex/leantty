//! GPU 渲染后端（wgpu/Vulkan，仅 Linux 目标）。
//!
//! 设计要点（阶段 4B t3）：
//! - 与 `CpuRenderer` 共享**纯决策逻辑**：字体组由 `crate::build_font_groups` 提供，
//!   颜色/反色/faint/选区/宽字符/样式线的判定由 [`decide`] 统一给出，两条路径
//!   只差"如何把这些决策画成像素"。
//! - 字形图集**动态增长**：键 =（字体组，码点，像素字号，缩放）。初始 2048×2048，
//!   放不下时按设备 `max_texture_dimension_2d` 翻倍重建；达到上限仍满则清空并
//!   重新上传当前帧所需字形（保证画面正确性，不 panic）。
//! - 本模块 t3 阶段只做离屏渲染 + 回读（供对拍门禁使用）；t4 再接入 surface。

#![cfg(any(target_os = "linux", target_os = "windows"))]

use crate::eperf;
use std::collections::HashMap;

use vt::{Cell, FrameSnapshot, Underline};

use crate::{Error, Font, FontSettings, Palette, PreeditOverlay, RenderOptions, Renderer};

mod atlas;
mod instances;
mod shaders;

pub use atlas::AtlasStats;
use atlas::{AtlasKey, GlyphAtlas};
use instances::{BgInstance, GlyphInstance};

/// 一帧的实例数据（分区边界：quads 前 `bg_n` 个是单元格背景，glyphs 前 `glyph_n`
/// 个是普通字形；其后依次是样式线/光标填充与光标字符）。
pub struct FrameInstances {
    pub quads: Vec<BgInstance>,
    pub glyphs: Vec<GlyphInstance>,
    pub bg_n: u32,
    pub glyph_n: u32,
    /// quads 末尾 `tail_n` 个在**所有字形之后**绘制（Preedit 下划线：CPU 侧每个
    /// 字符是"底色 → 字形 → 下划线"，若下划线排在中段会被后画的字形盖住）。
    pub tail_n: u32,
}

pub use crate::decide::{decide, CellDecision, DecideCtx};
pub(crate) use crate::decide::rgb_to_u32;



/// 字体组选择（与 `crate::group_for` 一致）。
pub(crate) fn group_for(cell: &Cell) -> u8 {
    match (cell.bold, cell.italic) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 0,
    }
}

/// wgpu 设备/队列与静态资源。
struct Gpu {
    /// 保留 instance：surface 需要用它创建（Lost 时重建）。
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    adapter_type: wgpu::DeviceType,
    max_texture: u32,
}

/// GPU 渲染器（离屏；t4 起支持 surface）。
pub struct GpuRenderer {
    gpu: Gpu,
    pipelines: Pipelines,
    atlas: GlyphAtlas,
    /// 四组字体链（与 CPU 后端同源，`Arc` 共享以避免重复加载）。
    groups: std::sync::Arc<Vec<Vec<std::sync::Arc<Font>>>>,
    font_size: f32,
    scale: f64,
    options: RenderOptions,
    palette: Palette,
    selection: Option<(u16, u16, u16, u16)>,
    scrollbar: Option<(u32, u32)>,
    preedit: Option<PreeditOverlay>,
    /// 渲染目标：离屏（对拍/测试）或窗口 surface（正式路径）。
    target: Target,
    /// surface acquire 结果计数（阶段 4Q t3 观测）。
    acquire: AcquireStats,
    /// 上一帧回读的 RGBA 数据（t3 对拍用）。
    rgba: Vec<u8>,
    /// 字形 → 组内字体索引缓存（与 CPU 同规则）。
    char_font: HashMap<(u8, char), Option<usize>>,
    /// 上次渲染尺寸（尺寸变化时重建离屏目标）。
    pixel: (u32, u32),
}

enum Target {
    /// 离屏纹理 + 回读缓冲（对拍门禁与无窗口场景）。
    Offscreen(OffscreenTarget),
    /// 窗口 surface（正式路径，不回读）。
    Surface(SurfaceTarget),
}

struct SurfaceTarget {
    surface: wgpu::Surface<'static>,
    /// 原始窗口/显示句柄（Lost 时重建 surface 用；由调用方保证存活）。
    raw_window: raw_window_handle::RawWindowHandle,
    raw_display: raw_window_handle::RawDisplayHandle,
    config: wgpu::SurfaceConfiguration,
    /// device lost 后已重建的次数（最多一次，再失败即报错）。
    rebuilt: u8,
    /// 窗口尺寸变化等需要 reconfigure 的标记。
    needs_configure: bool,
}

struct OffscreenTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
}

struct Pipelines {
    bg: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
    bg_layout: wgpu::BindGroupLayout,
    bg_bind: wgpu::BindGroup,
    glyph_layout: wgpu::BindGroupLayout,
    glyph_bind: wgpu::BindGroup,
    screen: wgpu::Buffer,
    bg_inst: wgpu::Buffer,
    glyph_inst: wgpu::Buffer,
    bg_cap: u32,
    glyph_cap: u32,
}

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// 窗口 surface 的初始化参数（打包以控制函数参数个数）。
pub struct SurfaceInit {
    pub raw_window: raw_window_handle::RawWindowHandle,
    pub raw_display: raw_window_handle::RawDisplayHandle,
    pub width: u32,
    pub height: u32,
}

impl GpuRenderer {
    /// 创建离屏 GPU 渲染器；无可用真实适配器时返回 `Error::Backend`（不 panic）。
    pub fn new_offscreen(
        settings: &FontSettings,
        options: RenderOptions,
        palette: Palette,
    ) -> Result<Self, Error> {
        let gpu = Gpu::new()?;
        let groups = std::sync::Arc::new(crate::build_font_groups(settings)?);
        let atlas = GlyphAtlas::new(
            &gpu.device,
            &gpu.queue,
            gpu.max_texture,
            atlas::INITIAL_SIZE,
        );
        let pipelines = Pipelines::new(&gpu.device, &atlas, TARGET_FORMAT);
        let target = Target::Offscreen(OffscreenTarget::new(&gpu.device, 1, 1));
        let acquire = AcquireStats::default();
        Ok(Self {
            gpu,
            pipelines,
            atlas,
            groups,
            acquire,
            font_size: settings.size,
            scale: 1.0,
            options,
            palette,
            selection: None,
            scrollbar: None,
            preedit: None,
            target,
            rgba: Vec::new(),
            char_font: HashMap::new(),
            pixel: (1, 1),
        })
    }

    /// 适配器名（日志/诊断）。
    pub fn adapter_name(&self) -> &str {
        &self.gpu.adapter_name
    }

    /// 适配器类型。
    pub fn adapter_type(&self) -> wgpu::DeviceType {
        self.gpu.adapter_type
    }

    /// 图集统计（压力测试断言用）：当前尺寸、字形数、重建次数。
    pub fn atlas_stats(&self) -> AtlasStats {
        self.atlas.stats()
    }

    /// 当前字体的格子尺寸（物理像素）。
    pub fn cell_metrics(&self, scale: f64) -> (u32, u32) {
        match self.groups.first().and_then(|g| g.first()) {
            Some(f) => crate::cell_metrics(f, self.font_size, scale),
            None => (8, 16),
        }
    }

    /// 更新缩放因子（字形缓存键含缩放，无需清空）。
    pub fn set_scale_factor(&mut self, scale: f64) {
        self.scale = scale;
    }

    /// 并入后台字体（CJK fallback）。
    pub fn add_background_fonts(&mut self, fonts: Vec<Font>) {
        // 阶段 4N t3：与 CPU 渲染器一致——每个回退字体只包一次 Arc，四组共享。
        let t_merge = std::time::Instant::now();
        let shared: Vec<std::sync::Arc<Font>> = fonts.into_iter().map(std::sync::Arc::new).collect();
        for g in std::sync::Arc::make_mut(&mut self.groups) {
            g.extend(shared.iter().cloned());
        }
        crate::eperf!(
            "[perf] bg_fonts_merge_us families={} groups={} strong_refs={:?} merge_us={}",
            shared.len(),
            self.groups.len(),
            shared.iter().map(std::sync::Arc::strong_count).collect::<Vec<_>>(),
            t_merge.elapsed().as_micros()
        );
        self.char_font.clear();
    }

    /// 选区（内容绝对坐标，含端点）。
    pub fn set_selection(&mut self, region: Option<(u16, u16, u16, u16)>) {
        self.selection = region;
    }

    /// 滚动条拇指几何。
    pub fn set_scrollbar(&mut self, geom: Option<(u32, u32)>) {
        self.scrollbar = geom;
    }

    /// IME 预编辑叠加层。
    pub fn set_preedit(&mut self, overlay: Option<PreeditOverlay>) {
        self.preedit = overlay;
    }

    /// 整窗清屏色 = 调色板主背景色。
    fn clear_color(&self) -> wgpu::Color {
        let c = self.palette.primary_bg;
        wgpu::Color {
            r: f64::from(c.r) / 255.0,
            g: f64::from(c.g) / 255.0,
            b: f64::from(c.b) / 255.0,
            a: 1.0,
        }
    }

    /// 当前渲染尺寸（像素）。
    pub fn pixel_size(&self) -> (u32, u32) {
        self.pixel
    }

    /// 最近一帧回读的 RGBA8 数据（左上原点，紧凑排布）。
    pub fn framebuffer_rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// 基线相对格顶的偏移（像素），与 `CpuRenderer::baseline_px` 同公式。
    fn baseline_px(&self, px: f32, cell_h: u32) -> i32 {
        self.groups
            .first()
            .and_then(|g| g.first())
            .and_then(|f| f.horizontal_line_metrics(px))
            .map(|lm| crate::baseline_from_metrics(cell_h, &lm))
            .unwrap_or(0)
    }

    /// 组内字体索引（第一个含该码点的字体；全无则 None → 用组首字体画 U+25A1）。
    fn font_index_for(&mut self, group: u8, ch: char) -> Option<usize> {
        if let Some(&i) = self.char_font.get(&(group, ch)) {
            return i;
        }
        let idx = self.groups[group as usize]
            .iter()
            .position(|f| f.has_glyph(ch));
        self.char_font.insert((group, ch), idx);
        idx
    }

    /// 光栅化（None → 组首字体画 U+25A1 占位方块，与 CPU 一致）。
    fn rasterize(&self, group: u8, idx: Option<usize>, ch: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        let chain = &self.groups[group as usize];
        match idx {
            Some(i) => chain[i].rasterize(ch, px),
            None => chain[0].rasterize('\u{25A1}', px),
        }
    }

    /// 构建一帧的实例数据（调用方负责最终提交）。
    fn build_instances(
        &mut self,
        frame: &FrameSnapshot,
        pixel_w: u32,
        pixel_h: u32,
    ) -> Result<FrameInstances, Error> {
        let cols = u32::from(frame.cols).max(1);
        let rows = u32::from(frame.rows).max(1);
        let cell_w = (pixel_w / cols).max(1);
        let cell_h = (pixel_h / rows).max(1);
        let px = (self.font_size * self.scale as f32).max(0.1);
        let bg_default = rgb_to_u32(self.palette.primary_bg);
        let fg_default = rgb_to_u32(self.palette.primary_fg);
        let baseline = self.baseline_px(px, cell_h);
        let n_cells: usize = frame.cells.iter().map(Vec::len).sum();
        let mut quads = Vec::with_capacity(n_cells * 2);
        // 样式线单独收集：它们是"字形之上"的层，必须排在 bg_n 之后提交，
        // 否则会被字形覆盖（CPU 的绘制顺序是 bg → 字形 → 样式线）。
        let mut lines: Vec<BgInstance> = Vec::new();
        let mut glyphs = Vec::with_capacity(n_cells);

        for (y, row) in frame.cells.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                let px0 = x as u32 * cell_w;
                let py0 = y as u32 * cell_h;
                let d = decide(
                    cell,
                    &DecideCtx {
                        x,
                        y,
                        row_len: row.len(),
                        bg_default,
                        fg_default,
                        selection: self.selection,
                        palette: &self.palette,
                        draw_bold_with_bright: self.options.draw_bold_with_bright,
                    },
                );
                // 背景：宽字符起始格按 2 格宽铺满；续格**不画**（CPU 修复后同样
                // 跳过续格的 fill_rect，否则会把宽字形右半擦掉、覆盖成背景色）。
                if !d.is_tail {
                    quads.push(BgInstance {
                        rect: [
                            px0 as f32,
                            py0 as f32,
                            (cell_w * d.span) as f32,
                            cell_h as f32,
                        ],
                        color: unpack3(d.bg),
                        _pad: 0.0,
                    });
                }
                if d.is_tail || cell.chars.is_empty() || cell.invisible {
                    continue;
                }
                let ch = cell.chars.chars().next().ok_or_else(|| {
                    Error::Backend("空字符单元意外进入字形构建".to_string())
                })?;
                let group = group_for(cell);
                let font_idx = self.font_index_for(group, ch);
                let (m, bitmap) = self.rasterize(group, font_idx, ch, px);
                // 宽字符按 2 格内缩放居中（与 CPU 的 scale_bitmap 规则一致）。
                let (gx, gy, gw, gh, data) = if d.span == 2 {
                    let area_w = cell_w * 2;
                    let bw = m.width.max(1) as f32;
                    let bh = m.height.max(1) as f32;
                    let s = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
                    if (s - 1.0).abs() < f32::EPSILON {
                        (m.xmin, m.ymin, m.width as u32, m.height as u32, bitmap)
                    } else {
                        let sw = (bw * s).ceil() as usize;
                        let sh = (bh * s).ceil() as usize;
                        let scaled = crate::scale_bitmap(&bitmap, m.width, m.height, sw, sh);
                        let ox = (area_w as i32 - sw as i32) / 2;
                        (
                            (m.xmin as f32 * s).ceil() as i32 + ox,
                            (m.ymin as f32 * s).ceil() as i32,
                            sw as u32,
                            sh as u32,
                            scaled,
                        )
                    }
                } else {
                    (m.xmin, m.ymin, m.width as u32, m.height as u32, bitmap)
                };
                if gw == 0 || gh == 0 {
                    continue;
                }
                let key = AtlasKey {
                    group,
                    ch,
                    px: px.ceil() as u32,
                    scale_bits: self.scale.to_bits(),
                };
                // 图集可能增长/重建：返回该字形的 uv。
                let Some(uv) = self
                    .atlas
                    .get_or_insert(&self.gpu.device, &self.gpu.queue, key, gw, gh, &data)
                else {
                    continue;
                };
                let shift = crate::glyph_shift(baseline, gy, gh as i32, cell_h);
                glyphs.push(GlyphInstance {
                    // 顶边 = 格顶 + 基线 - ymin - height + 平移（与 CPU 的 blit 同公式）。
                    rect: [
                        (px0 as i32 + gx) as f32,
                        (py0 as i32 + baseline - gy - gh as i32 + shift) as f32,
                        gw as f32,
                        gh as f32,
                    ],
                    // 裁剪到起始单格之外的宽字符区域（与 CPU blit 的实际可见范围一致，
                    // 但 CPU 已修好跨格；这里按 2 格裁剪）。
                    cell: [
                        px0 as f32,
                        py0 as f32,
                        (cell_w * d.span) as f32,
                        cell_h as f32,
                    ],
                    uv,
                    color: unpack3(d.fg),
                    backdrop: unpack3(d.blend_bg),
                    _pad: 0.0,
                });
                // 样式线（延后到 bg_n 之后追加）
                push_style_lines(&mut lines, cell, px0, py0, cell_w * d.span, cell_h, d.line);
            }
        }
        let bg_n = quads.len() as u32;
        quads.append(&mut lines);
        let glyph_n = glyphs.len() as u32;

        // 光标（覆盖在内容之上）
        let cur = &frame.cursor;
        if cur.visible {
            let cx = u32::from(cur.x) * cell_w;
            let cy = u32::from(cur.y) * cell_h;
            let color = unpack3(rgb_to_u32(self.palette.cursor));
            let mut push = |x: u32, y: u32, w: u32, h: u32, c: [f32; 3]| {
                quads.push(BgInstance {
                    rect: [x as f32, y as f32, w as f32, h as f32],
                    color: c,
                    _pad: 0.0,
                });
            };
            match cur.style {
                vt::CursorStyle::Block => {
                    push(cx, cy, cell_w, cell_h, color);
                    if let Some(cell) = frame
                        .cells
                        .get(cur.y as usize)
                        .and_then(|r| r.get(cur.x as usize))
                        && !cell.chars.is_empty()
                    {
                        let ch = cell.chars.chars().next().ok_or_else(|| {
                            Error::Backend("光标格字符为空".to_string())
                        })?;
                        let group = group_for(cell);
                        let font_idx = self.font_index_for(group, ch);
                        let (m, bitmap) = self.rasterize(group, font_idx, ch, px);
                        let gw = m.width as u32;
                        let gh = m.height as u32;
                        if gw > 0 && gh > 0 {
                            let key = AtlasKey {
                                group,
                                ch,
                                px: px.ceil() as u32,
                                scale_bits: self.scale.to_bits(),
                            };
                            if let Some(uv) = self.atlas.get_or_insert(
                                &self.gpu.device,
                                &self.gpu.queue,
                                key,
                                gw,
                                gh,
                                &bitmap,
                            ) {
                                let cshift = crate::glyph_shift(
                                    baseline,
                                    m.ymin,
                                    m.height as i32,
                                    cell_h,
                                );
                                glyphs.push(GlyphInstance {
                                    rect: [
                                        (cx as i32 + m.xmin) as f32,
                                        (cy as i32 + baseline - m.ymin
                                            - m.height as i32
                                            + cshift) as f32,
                                        gw as f32,
                                        gh as f32,
                                    ],
                                    cell: [cx as f32, cy as f32, cell_w as f32, cell_h as f32],
                                    uv,
                                    color: unpack3(rgb_to_u32(self.palette.cursor_text)),
                                    backdrop: color,
                                    _pad: 0.0,
                                });
                            }
                        }
                    }
                }
                vt::CursorStyle::HollowBlock => {
                    push(cx, cy, cell_w, 1, color);
                    push(cx, cy + cell_h - 1, cell_w, 1, color);
                    push(cx, cy, 1, cell_h, color);
                    push(cx + cell_w - 1, cy, 1, cell_h, color);
                }
                vt::CursorStyle::Underline => {
                    push(cx, cy + cell_h - 1, cell_w, 1, color);
                    push(cx, cy + cell_h - 2, cell_w, 1, color);
                }
                vt::CursorStyle::Bar => {
                    push(cx, cy, 1, cell_h, color);
                    push(cx + 1, cy, 1, cell_h, color);
                }
            }
        }

        // 滚动条（t5 补：轨道 + 拇指）
        if let Some((thumb_y, thumb_h)) = self.scrollbar
            && thumb_h > 0
            && pixel_w >= crate::SCROLLBAR_WIDTH
        {
                let track_x = pixel_w - crate::SCROLLBAR_WIDTH;
                let fg = rgb_to_u32(self.palette.primary_fg);
                let bg = rgb_to_u32(self.palette.primary_bg);
                let avg = |a: u32, b: u32| (a + b) / 2;
                let track = (avg((fg >> 16) & 0xFF, (bg >> 16) & 0xFF) << 16)
                    | (avg((fg >> 8) & 0xFF, (bg >> 8) & 0xFF) << 8)
                    | avg(fg & 0xFF, bg & 0xFF);
                quads.push(BgInstance {
                    rect: [track_x as f32, 0.0, crate::SCROLLBAR_WIDTH as f32, pixel_h as f32],
                    color: unpack3(track),
                    _pad: 0.0,
                });
            let th = thumb_h.min(pixel_h);
            let ty = thumb_y.min(pixel_h.saturating_sub(th));
            quads.push(BgInstance {
                rect: [track_x as f32, ty as f32, crate::SCROLLBAR_WIDTH as f32, th as f32],
                color: unpack3(fg),
                _pad: 0.0,
            });
        }

        // Preedit 下划线单独收集：必须在字形之后画（见 FrameInstances::tail_n）。
        let mut tail_quads: Vec<BgInstance> = Vec::new();
        // IME 预编辑叠加层：复刻 CpuRenderer::draw_preedit 的视觉规则
        // （每格先盖底色遮住原内容、组合光标格反色、字形按 2 格内缩放、格底 2px 下划线）。
        if let Some(overlay) = self.preedit.clone() {
            let cells = preedit_layout(&overlay, frame);
            if let Some((layout, cursor_cell_idx)) = cells {
                let bg = rgb_to_u32(self.palette.primary_bg);
                let fg = rgb_to_u32(self.palette.primary_fg);
                let mut x_cells = u32::from(layout.col);
                let y = u32::from(layout.row) * cell_h;
                for (idx, pc) in layout.cells.iter().enumerate() {
                    let px0 = x_cells * cell_w;
                    let span = u32::from(pc.width).max(1);
                    let area_w = cell_w * span;
                    if px0 + area_w > pixel_w {
                        break;
                    }
                    let is_cursor = cursor_cell_idx == Some(idx);
                    let (cell_bg, cell_fg) = if is_cursor { (fg, bg) } else { (bg, fg) };
                    quads.push(BgInstance {
                        rect: [px0 as f32, y as f32, area_w as f32, cell_h as f32],
                        color: unpack3(cell_bg),
                        _pad: 0.0,
                    });
                    // 字形：与终端宽字符相同的缩放规则。
                    let group = 0u8;
                    let font_idx = self.font_index_for(group, pc.ch);
                    let (m, bitmap) = self.rasterize(group, font_idx, pc.ch, px);
                    let bw = m.width.max(1) as f32;
                    let bh = m.height.max(1) as f32;
                    let sc = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
                    let (gx, gy, gw, gh, data) = if (sc - 1.0).abs() < f32::EPSILON {
                        (m.xmin, m.ymin, m.width as u32, m.height as u32, bitmap)
                    } else {
                        let sw = (bw * sc).ceil() as usize;
                        let sh = (bh * sc).ceil() as usize;
                        let scaled = crate::scale_bitmap(&bitmap, m.width, m.height, sw, sh);
                        let ox = (area_w as i32 - sw as i32) / 2;
                        (
                            (m.xmin as f32 * sc).ceil() as i32 + ox,
                            (m.ymin as f32 * sc).ceil() as i32,
                            sw as u32,
                            sh as u32,
                            scaled,
                        )
                    };
                    if gw > 0 && gh > 0 {
                        let key = AtlasKey {
                            group,
                            ch: pc.ch,
                            px: px.ceil() as u32,
                            scale_bits: self.scale.to_bits(),
                        };
                        if let Some(uv) = self.atlas.get_or_insert(
                            &self.gpu.device,
                            &self.gpu.queue,
                            key,
                            gw,
                            gh,
                            &data,
                        ) {
                            let pshift = crate::glyph_shift(baseline, gy, gh as i32, cell_h);
                            glyphs.push(GlyphInstance {
                                rect: [
                                    (px0 as i32 + gx) as f32,
                                    (y as i32 + baseline - gy - gh as i32 + pshift) as f32,
                                    gw as f32,
                                    gh as f32,
                                ],
                                cell: [px0 as f32, y as f32, area_w as f32, cell_h as f32],
                                uv,
                                color: unpack3(cell_fg),
                                backdrop: unpack3(cell_bg),
                                _pad: 0.0,
                            });
                        }
                    }
                    // 下划线：格底 2px（延后到所有字形之后绘制）。
                    tail_quads.push(BgInstance {
                        rect: [
                            px0 as f32,
                            (y + cell_h.saturating_sub(2)) as f32,
                            area_w as f32,
                            2.0,
                        ],
                        color: unpack3(cell_fg),
                        _pad: 0.0,
                    });
                    x_cells += span;
                }
            }
        }

        let tail_n = tail_quads.len() as u32;
        quads.append(&mut tail_quads);
        Ok(FrameInstances {
            quads,
            glyphs,
            bg_n,
            glyph_n,
            tail_n,
        })
    }

    /// 创建面朝窗口的 GPU 渲染器（正式路径）。
    ///
    /// `raw_window`/`raw_display` 来自 winit 的 `HasWindowHandle`/`HasDisplayHandle`；
    /// 调用方保证句柄在渲染器存活期间有效（窗口由事件循环持有）。
    pub fn new_surface(
        settings: &FontSettings,
        options: RenderOptions,
        palette: Palette,
        surface: SurfaceInit,
    ) -> Result<Self, Error> {
        Self::new_surface_with_groups(settings, options, palette, None, surface)
    }

    /// 同 `new_surface`，但可复用**已构建的**字体组（`Arc` 共享，零拷贝）。
    ///
    /// stage 4E t3：auto/gpu 模式下窗口前已为估算格子尺寸建过 CPU 渲染器，
    /// 其字体链与 GPU 渲染器完全相同；复用可省掉一次 fontdb 扫描 + 四组字体解析
    /// （实测约 155ms）。
    pub fn new_surface_with_groups(
        settings: &FontSettings,
        options: RenderOptions,
        palette: Palette,
        reuse_groups: Option<std::sync::Arc<Vec<Vec<std::sync::Arc<Font>>>>>,
        surface: SurfaceInit,
    ) -> Result<Self, Error> {
        let SurfaceInit {
            raw_window,
            raw_display,
            width,
            height,
        } = surface;
        // 阶段 4H t2：GPU 侧入口点（内部实例/适配器/设备三段已有打点，不重复）。
        eperf!(
            "[perf] gpu_begin_ms=0.0 since_start={:.1} reuse_groups={}",
            crate::since_start_ms(),
            reuse_groups.is_some()
        );
        let gpu = Gpu::new()?;
        let groups = match reuse_groups {
            Some(g) => g,
            None => std::sync::Arc::new(crate::build_font_groups(settings)?),
        };
        let t_atlas = std::time::Instant::now();
        let atlas = GlyphAtlas::new(&gpu.device, &gpu.queue, gpu.max_texture, atlas::INITIAL_SIZE);
        eperf!(
            "[perf] gpu_atlas_ms={:.1} since_start={:.1}",
            t_atlas.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms()
        );
        let instance = gpu.instance.clone();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display),
                raw_window_handle: raw_window,
            })
        }
        .map_err(|e| Error::Backend(format!("create_surface: {e}")))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| Error::Backend("surface 无可用格式".to_string()))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        eperf!(
            "[perf] gpu_surface_ms={:.1} since_start={:.1} fmt={:?} present={:?} alpha={:?} size={}x{}",
            t_atlas.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms(),
            config.format,
            config.present_mode,
            config.alpha_mode,
            config.width,
            config.height
        );
        let t_pipe = std::time::Instant::now();
        let pipelines = Pipelines::new(&gpu.device, &atlas, config.format);
        eperf!(
            "[perf] gpu_pipeline_ms={:.1} since_start={:.1}",
            t_pipe.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms()
        );
        let t_state = std::time::Instant::now();
        let me = Ok(Self {
            gpu,
            pipelines,
            atlas,
            groups,
            font_size: settings.size,
            scale: 1.0,
            options,
            palette,
            selection: None,
            scrollbar: None,
            preedit: None,
            acquire: AcquireStats::default(),
            target: Target::Surface(SurfaceTarget {
                surface,
                raw_window,
                raw_display,
                config,
                rebuilt: 0,
                needs_configure: false,
            }),
            rgba: Vec::new(),
            char_font: HashMap::new(),
            pixel: (width.max(1), height.max(1)),
        });
        eperf!(
            "[perf] gpu_state_ms={:.2} since_start={:.1}",
            t_state.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms()
        );
        me
    }

    /// 窗口尺寸变化（含 HiDPI）：标记需要 reconfigure，下一帧生效。
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if self.pixel == (w, h) {
            return;
        }
        self.pixel = (w, h);
        if let Target::Surface(t) = &mut self.target {
            t.config.width = w;
            t.config.height = h;
            t.needs_configure = true;
        }
    }

    /// 把已取得的 surface 纹理绘制并 present（Success 与 Suboptimal 共用）。
    fn present_acquired(
        &mut self,
        tex: wgpu::SurfaceTexture,
        inst: &crate::gpu::FrameInstances,
        pixel_w: u32,
        pixel_h: u32,
    ) -> Result<(), Error> {
        let view = tex
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.pipelines.write_screen(&self.gpu.queue, pixel_w, pixel_h);
        self.pipelines
            .ensure_capacity(&self.gpu.device, inst.quads.len(), inst.glyphs.len().max(1));
        let clear = self.clear_color();
        self.pipelines.submit(
            &self.gpu.device,
            &self.gpu.queue,
            &self.atlas,
            &view,
            inst,
            clear,
        )?;
        self.gpu.queue.present(tex);
        Ok(())
    }

    /// surface acquire 结果计数（阶段 4Q t3 观测）。
    pub fn acquire_stats(&self) -> AcquireStats {
        self.acquire
    }

    /// 渲染一帧到 surface 并 present（整帧重绘，不回读）。
    ///
    /// surface Lost/Outdated：reconfigure 后重试本帧；device lost：重建一次；
    /// 仍失败则返回 Err（app 据此非零退出），不做中途回退。
    pub fn render_surface(
        &mut self,
        frame: &FrameSnapshot,
        pixel_w: u32,
        pixel_h: u32,
    ) -> Result<(), Error> {
        self.resize_surface(pixel_w, pixel_h);
        let inst = self.build_instances(frame, pixel_w, pixel_h)?;
        let Target::Surface(t) = &mut self.target else {
            return Err(Error::Backend(
                "render_surface 只能用于 surface 目标".to_string(),
            ));
        };
        if t.needs_configure {
            t.surface.configure(&self.gpu.device, &t.config);
            t.needs_configure = false;
        }
        for attempt in 0..2u8 {
            let acquired = t.surface.get_current_texture();
            match acquired {
                // 阶段 4Q t3：Suboptimal 与 Success 走同一条渲染路径，但单独计数——
                // 若空闲时 suboptimal 持续增长，说明交换链一直在"次优"状态被反复渲染。
                wgpu::CurrentSurfaceTexture::Suboptimal(tex) => {
                    self.acquire.suboptimal += 1;
                    self.present_acquired(tex, &inst, pixel_w, pixel_h)?;
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Success(tex) => {
                    self.acquire.success += 1;
                    self.present_acquired(tex, &inst, pixel_w, pixel_h)?;
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Outdated if attempt == 0 => {
                    self.acquire.outdated += 1;
                    // 旧交换链/尺寸过期：reconfigure 后重试一次本帧。
                    t.surface.configure(&self.gpu.device, &t.config);
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Lost if attempt == 0 => {
                    self.acquire.lost += 1;
                    // 重建 surface（保留 device）后重试一次本帧。
                    let surface = unsafe {
                        self.gpu.instance.create_surface_unsafe(
                            wgpu::SurfaceTargetUnsafe::RawHandle {
                                raw_display_handle: Some(t.raw_display),
                                raw_window_handle: t.raw_window,
                            },
                        )
                    }
                    .map_err(|e| Error::Backend(format!("重建 surface: {e}")))?;
                    surface.configure(&self.gpu.device, &t.config);
                    t.surface = surface;
                    t.rebuilt += 1;
                    continue;
                }
                other => {
                    self.acquire.other += 1;
                    return Err(Error::Backend(format!(
                        "get_current_texture: {}",
                        surface_status_desc(&other)
                    )));
                }
            }
        }
        Err(Error::Backend("surface 获取纹理失败（重试耗尽）".to_string()))
    }

    /// 渲染一帧到离屏纹理并回读（t3；t4 改为 surface present）。
    pub fn render_offscreen(
        &mut self,
        frame: &FrameSnapshot,
        pixel_w: u32,
        pixel_h: u32,
    ) -> Result<(), Error> {
        if self.pixel != (pixel_w, pixel_h) {
            self.target = Target::Offscreen(OffscreenTarget::new(
                &self.gpu.device,
                pixel_w,
                pixel_h,
            ));
            self.pixel = (pixel_w, pixel_h);
        }
        let inst = self.build_instances(frame, pixel_w, pixel_h)?;
        let Target::Offscreen(target) = &self.target else {
            return Err(Error::Backend(
                "render_offscreen 只能用于离屏目标（当前为 surface）".to_string(),
            ));
        };
        self.pipelines
            .ensure_capacity(&self.gpu.device, inst.quads.len(), inst.glyphs.len().max(1));
        self.pipelines.write_screen(&self.gpu.queue, pixel_w, pixel_h);
        let clear = self.clear_color();
        self.pipelines.submit(
            &self.gpu.device,
            &self.gpu.queue,
            &self.atlas,
            &target.view,
            &inst,
            clear,
        )?;
        self.rgba = target.read_back(&self.gpu.device, &self.gpu.queue)?;
        Ok(())
    }

}

impl Renderer for GpuRenderer {
    fn render(
        &mut self,
        frame: &FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<(), Error> {
        self.render_offscreen(frame, pixel_width, pixel_height)
    }

    fn set_preedit(&mut self, overlay: Option<PreeditOverlay>) {
        self.preedit = overlay;
    }
}

/// 预编辑布局与组合光标单元下标（与 `CpuRenderer::draw_preedit` 同规则）。
fn preedit_layout(
    overlay: &PreeditOverlay,
    frame: &FrameSnapshot,
) -> Option<(crate::preedit::PreeditLayout, Option<usize>)> {
    let cols = u32::from(frame.cols).max(1);
    let rows = u32::from(frame.rows).max(1);
    let layout = crate::preedit::layout_preedit(
        &overlay.text,
        frame.cursor.x,
        frame.cursor.y,
        cols as u16,
        rows as u16,
    )?;
    let cursor_cell_idx = overlay.cursor_range.map(|(begin, _)| {
        overlay
            .text
            .char_indices()
            .position(|(i, _)| i >= begin)
            .unwrap_or(0)
    });
    Some((layout, cursor_cell_idx))
}

/// u32 颜色（0x00RRGGBB）→ 归一化 RGB。
pub(crate) fn unpack3(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 0xFF) as f32 / 255.0,
        ((c >> 8) & 0xFF) as f32 / 255.0,
        (c & 0xFF) as f32 / 255.0,
    ]
}

/// 复刻 `crate::draw_style_lines`：下划线/删除线/上划线，虚线按下标取模。
fn push_style_lines(
    quads: &mut Vec<BgInstance>,
    cell: &Cell,
    px0: u32,
    py0: u32,
    w: u32,
    h: u32,
    color: u32,
) {
    let c = unpack3(color);
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
        Underline::None => {}
        Underline::Single => hline(quads, h as i32 - 2, 1),
        Underline::Double => {
            hline(quads, h as i32 - 2, 1);
            hline(quads, h as i32 - 4, 1);
        }
        Underline::Dotted => hline(quads, h as i32 - 2, 2),
        Underline::Dashed => hline(quads, h as i32 - 2, 4),
        Underline::Curly => hline(quads, h as i32 - 2, 1),
    }
    if cell.strikethrough {
        hline(quads, h as i32 / 2, 1);
    }
    if cell.overline {
        hline(quads, 1, 1);
    }
}

/// surface acquire 结果计数（阶段 4Q t3 观测：空闲时是否出现异常分支）。
#[derive(Debug, Clone, Copy, Default)]
pub struct AcquireStats {
    /// 直接成功（含 Suboptimal，后者按成功处理且不重配置）。
    pub success: u64,
    /// Suboptimal（按成功渲染，仅计数以便发现"每帧都 suboptimal"）。
    pub suboptimal: u64,
    /// Outdated（reconfigure 后重试）。
    pub outdated: u64,
    /// Lost（重建 surface 后重试）。
    pub lost: u64,
    /// 其它（Timeout/Occluded/Validation 等，直接返回错误）。
    pub other: u64,
}

/// surface 获取失败的状态描述（CurrentSurfaceTexture 未实现 Display）。
fn surface_status_desc(s: &wgpu::CurrentSurfaceTexture) -> &'static str {
    match s {
        wgpu::CurrentSurfaceTexture::Success(_) => "Success",
        wgpu::CurrentSurfaceTexture::Suboptimal(_) => "Suboptimal",
        wgpu::CurrentSurfaceTexture::Timeout => "Timeout",
        wgpu::CurrentSurfaceTexture::Occluded => "Occluded",
        wgpu::CurrentSurfaceTexture::Outdated => "Outdated",
        wgpu::CurrentSurfaceTexture::Lost => "Lost",
        wgpu::CurrentSurfaceTexture::Validation => "Validation",
    }
}

/// 解析 `WGPU_BACKEND` 的逗号列表（阶段 4Q t2）。
///
/// 与 wgpu 自带 `Backends::from_comma_list` 的差别：**无法识别的记号返回 Err**
/// （携带原始值与出错记号），而不是静默丢弃；同时显式 trim 空白、大小写不敏感
/// （wgpu 内部也 trim，但丢弃未知记号后只剩空集合，用户无法得知自己写错了）。
///
/// 支持的记号（大小写不敏感，等价于 wgpu）：`vulkan`/`vk`、`dx12`/`d3d12`、
/// `metal`/`mtl`、`gl`/`gles`/`opengl`、`webgpu`。
pub fn parse_backends_spec(spec: &str) -> Result<wgpu::Backends, String> {
    let mut out = wgpu::Backends::empty();
    let mut unknown: Vec<&str> = Vec::new();
    let mut any = false;
    for tok in spec.split(',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        any = true;
        let lower = t.to_ascii_lowercase();
        out |= match lower.as_str() {
            "vulkan" | "vk" => wgpu::Backends::VULKAN,
            "dx12" | "d3d12" => wgpu::Backends::DX12,
            "metal" | "mtl" => wgpu::Backends::METAL,
            "gl" | "gles" | "opengl" => wgpu::Backends::GL,
            "webgpu" => wgpu::Backends::BROWSER_WEBGPU,
            _ => {
                unknown.push(t);
                wgpu::Backends::empty()
            }
        };
    }
    if !unknown.is_empty() {
        return Err(format!(
            "WGPU_BACKEND='{spec}' 含无法识别的后端记号 {unknown:?}；支持：vulkan|vk, dx12|d3d12, metal|mtl, gl|gles|opengl, webgpu"
        ));
    }
    if !any || out.is_empty() {
        return Err(format!("WGPU_BACKEND='{spec}' 未包含任何后端名"));
    }
    Ok(out)
}

impl Gpu {
    /// 默认后端集合（未设置 `WGPU_BACKEND` 时）：Linux 仅 Vulkan，Windows 为 Vulkan|DX12。
    ///
    /// 阶段 4P：可用标准环境变量 `WGPU_BACKEND`（如 `dx12`、`vulkan`、`vulkan,dx12`）覆盖，
    /// 不新增任何配置项。
    fn default_backends() -> wgpu::Backends {
        if cfg!(target_os = "windows") {
            wgpu::Backends::VULKAN | wgpu::Backends::DX12
        } else {
            wgpu::Backends::VULKAN
        }
    }

    /// 适配器类型优先级：独显 > 集显 > 其他（软件适配器在更早就被排除）。
    fn type_rank(t: wgpu::DeviceType) -> u8 {
        match t {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            _ => 2,
        }
    }

    /// 创建实例 + 适配器 + 设备；无真实适配器（或仅软件实现）时返回 Err。
    fn new() -> Result<Self, Error> {
        let t_instance = std::time::Instant::now();
        // 阶段 4Q t2：自己解析（严格），并把"原始值 / 生效值"都打出来，用引号包住原始值
        // 以暴露尾随空格这类问题；无法识别时明确报错而不是回落到默认值。
        let backends_env = std::env::var("WGPU_BACKEND").ok();
        let backends = match backends_env.as_deref() {
            Some(raw) => parse_backends_spec(raw).map_err(Error::Backend)?,
            None => Self::default_backends(),
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::empty(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let adapters = pollster::block_on(instance.enumerate_adapters(backends));
        eperf!(
            "[perf] gpu_instance_ms={:.1} since_start={:.1} backends_env='{}' backends_effective={:?} count={}",
            t_instance.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms(),
            backends_env.as_deref().unwrap_or("<unset>"),
            backends,
            adapters.len()
        );
        if adapters.is_empty() {
            let hint = if let Some(raw) = backends_env.as_deref() {
                format!(
                    "请求的后端（WGPU_BACKEND='{}' → {backends:?}）在本平台/本构建不可用或没有对应驱动",
                    raw
                )
            } else {
                format!("默认后端 {backends:?} 没有可用适配器（无 ICD/驱动）")
            };
            return Err(Error::Backend(format!("适配器枚举为空：{hint}")));
        }
        // 适配器清单（诊断）：每个一行，含 name / backend / type / driver
        for a in &adapters {
            let i = a.get_info();
            eperf!(
                "[perf] gpu_adapters name='{}' backend={:?} type={:?} driver='{}' driver_info='{}' vendor=0x{:04x} device=0x{:04x}",
                i.name,
                i.backend,
                i.device_type,
                i.driver,
                i.driver_info,
                i.vendor,
                i.device
            );
        }
        // 过滤掉软件适配器（含 Windows 的 WARP / Basic Render Driver）
        let mut real: Vec<_> = Vec::new();
        for a in adapters {
            let info = a.get_info();
            if info.device_type == wgpu::DeviceType::Cpu {
                eperf!("[perf] gpu_skip_software name='{}'", info.name);
                continue;
            }
            real.push(a);
        }
        if real.is_empty() {
            return Err(Error::Backend(
                "未找到真实 GPU 适配器（仅软件实现可用）".to_string(),
            ));
        }
        // 选择：WGPU_ADAPTER_NAME 子串匹配（不区分大小写）优先；否则按类型优先级，
        // 同级保持枚举顺序（稳定排序）。
        let adapter = match std::env::var("WGPU_ADAPTER_NAME") {
            Ok(want) if !want.is_empty() => {
                let want_lc = want.to_lowercase();
                let idx = real.iter().position(|a| {
                    a.get_info().name.to_lowercase().contains(&want_lc)
                });
                match idx {
                    Some(i) => real.swap_remove(i),
                    None => {
                        return Err(Error::Backend(format!(
                            "WGPU_ADAPTER_NAME='{want}' 未匹配到任何非软件适配器（可用：{}）",
                            real.iter()
                                .map(|a| a.get_info().name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )));
                    }
                }
            }
            _ => {
                let mut best = 0usize;
                for (i, a) in real.iter().enumerate() {
                    if Self::type_rank(a.get_info().device_type)
                        < Self::type_rank(real[best].get_info().device_type)
                    {
                        best = i;
                    }
                }
                real.swap_remove(best)
            }
        };
        let info = adapter.get_info();
        eperf!(
            "[perf] gpu_adapter_ms={:.1} since_start={:.1} name='{}' backend={:?} type={:?} driver='{}'",
            t_instance.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms(),
            info.name,
            info.backend,
            info.device_type,
            info.driver
        );
        let limits = adapter.limits();
        // 不要直接拿 adapter.limits() 当 required_limits：部分驱动会把
        // max_buffer_size/max_memory_allocation_size 报得极大，request_device
        // 会以 "Not enough memory left" 失败（实测）。按需收敛到本项目实际用量。
        let mut required = wgpu::Limits::downlevel_defaults();
        required.max_texture_dimension_2d = limits.max_texture_dimension_2d;
        // 回读缓冲在 4000x1920 下约 31MB，留足余量到 64MB。
        required.max_buffer_size = (64u64 << 20).min(limits.max_buffer_size);
        required.max_storage_buffer_binding_size =
            required.max_storage_buffer_binding_size.min(limits.max_storage_buffer_binding_size);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("tty-gpu-renderer"),
                required_features: wgpu::Features::empty(),
                required_limits: required,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))
            .map_err(|e| Error::Backend(format!("request_device: {e}")))?;
        eperf!(
            "[perf] gpu_device_ms={:.1} since_start={:.1}",
            t_instance.elapsed().as_secs_f64() * 1000.0,
            crate::since_start_ms()
        );
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            adapter_name: info.name,
            adapter_type: info.device_type,
            max_texture: limits.max_texture_dimension_2d,
        })
    }
}

impl OffscreenTarget {
    fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_row = (w * 4).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded_row) * u64::from(h.max(1)),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            readback,
            padded_row,
        }
    }

    fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<u8>, Error> {
        let w = self.texture.width();
        let h = self.texture.height();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
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
        queue.submit([enc.finish()]);
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r.is_ok());
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| Error::Backend(format!("poll: {e}")))?;
        match rx.recv() {
            Ok(true) => {}
            Ok(false) => return Err(Error::Backend("缓冲区映射失败".to_string())),
            Err(e) => return Err(Error::Backend(format!("映射回调断开: {e}"))),
        }
        let data = slice
            .get_mapped_range()
            .map_err(|e| Error::Backend(format!("get_mapped_range: {e}")))?;
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

impl Pipelines {
    fn new(device: &wgpu::Device, atlas: &GlyphAtlas, format: wgpu::TextureFormat) -> Self {
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg-bgl"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let bg_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bg"),
            layout: &bg_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        let glyph_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph-bgl"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX),
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
                    resource: screen.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });
        let bg = build_pipeline(
            device,
            &bg_layout,
            shaders::BG_WGSL,
            false,
            std::mem::size_of::<BgInstance>() as u64,
            format,
        );
        let glyph = build_pipeline(
            device,
            &glyph_layout,
            shaders::GLYPH_WGSL,
            true,
            std::mem::size_of::<GlyphInstance>() as u64,
            format,
        );
        const CAP: u32 = 1 << 16;
        let bg_inst = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg-inst"),
            size: u64::from(CAP) * std::mem::size_of::<BgInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glyph_inst = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph-inst"),
            size: u64::from(CAP) * std::mem::size_of::<GlyphInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            bg,
            glyph,
            bg_layout,
            bg_bind,
            glyph_layout,
            glyph_bind,
            screen,
            bg_inst,
            glyph_inst,
            bg_cap: CAP,
            glyph_cap: CAP,
        }
    }

    fn ensure_capacity(&mut self, device: &wgpu::Device, quads: usize, glyphs: usize) {
        if quads as u32 > self.bg_cap {
            self.bg_cap = (quads as u32).next_power_of_two();
            self.bg_inst = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bg-inst"),
                size: u64::from(self.bg_cap) * std::mem::size_of::<BgInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if glyphs as u32 > self.glyph_cap {
            self.glyph_cap = (glyphs as u32).next_power_of_two();
            self.glyph_inst = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glyph-inst"),
                size: u64::from(self.glyph_cap) * std::mem::size_of::<GlyphInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let _ = (&self.bg_layout, &self.glyph_layout);
    }

    fn write_screen(&self, queue: &wgpu::Queue, w: u32, h: u32) {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&(w as f32).to_le_bytes());
        bytes[4..8].copy_from_slice(&(h as f32).to_le_bytes());
        queue.write_buffer(&self.screen, 0, &bytes);
    }

    fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &GlyphAtlas,
        view: &wgpu::TextureView,
        inst: &FrameInstances,
        clear: wgpu::Color,
    ) -> Result<(), Error> {
        if inst.quads.len() as u32 > self.bg_cap || inst.glyphs.len() as u32 > self.glyph_cap {
            return Err(Error::Backend("实例数超过缓冲容量".to_string()));
        }
        queue.write_buffer(&self.bg_inst, 0, &bg_bytes(&inst.quads));
        if !inst.glyphs.is_empty() {
            queue.write_buffer(&self.glyph_inst, 0, &glyph_bytes(&inst.glyphs));
        }
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // 整窗先铺背景色（同 CPU）：网格不能整除窗口时余数区也是背景。
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bg_bind, &[]);
            pass.set_vertex_buffer(0, self.bg_inst.slice(..));
            pass.set_pipeline(&self.bg);
            pass.draw(0..6, 0..inst.bg_n);
            if !inst.glyphs.is_empty() {
                pass.set_pipeline(&self.glyph);
                pass.set_bind_group(0, &self.glyph_bind, &[]);
                pass.set_vertex_buffer(0, self.glyph_inst.slice(..));
                pass.draw(0..6, 0..inst.glyph_n);
                pass.set_pipeline(&self.bg);
                pass.set_bind_group(0, &self.bg_bind, &[]);
                pass.set_vertex_buffer(0, self.bg_inst.slice(..));
                let mid_end = (inst.quads.len() as u32).saturating_sub(inst.tail_n);
                pass.draw(0..6, inst.bg_n..mid_end);
                if inst.glyphs.len() as u32 > inst.glyph_n {
                    pass.set_pipeline(&self.glyph);
                    pass.set_bind_group(0, &self.glyph_bind, &[]);
                    pass.set_vertex_buffer(0, self.glyph_inst.slice(..));
                    pass.draw(0..6, inst.glyph_n..inst.glyphs.len() as u32);
                }
                // 最后画 quads 尾段（Preedit 下划线），保证不被字形覆盖。
                let tail_start = (inst.quads.len() as u32).saturating_sub(inst.tail_n);
                if inst.tail_n > 0 {
                    pass.set_pipeline(&self.bg);
                    pass.set_bind_group(0, &self.bg_bind, &[]);
                    pass.set_vertex_buffer(0, self.bg_inst.slice(..));
                    pass.draw(0..6, tail_start..inst.quads.len() as u32);
                }
            } else if inst.quads.len() as u32 > inst.bg_n {
                pass.set_pipeline(&self.bg);
                pass.set_vertex_buffer(0, self.bg_inst.slice(..));
                let mid_end = (inst.quads.len() as u32).saturating_sub(inst.tail_n);
                pass.draw(0..6, inst.bg_n..mid_end);
            }
        }
        queue.submit([enc.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| Error::Backend(format!("poll: {e}")))?;
        let _ = atlas;
        Ok(())
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    wgsl: &str,
    is_glyph: bool,
    stride: u64,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(if is_glyph { "glyph" } else { "bg" }),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layout"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if is_glyph { "glyph" } else { "bg" }),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &sm,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
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
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn vertex_attrs(glyph: bool) -> Vec<wgpu::VertexAttribute> {
    let mut v = vec![wgpu::VertexAttribute {
        offset: 0,
        shader_location: 0,
        format: wgpu::VertexFormat::Float32x4,
    }];
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

fn bg_bytes(v: &[BgInstance]) -> Vec<u8> {
    // 必须补 _pad：pipeline 的 array_stride 是 size_of::<BgInstance>() = 32 字节，
    // 只写 28 字节会让每个实例错位 4 字节（实测表现为整屏错乱）。
    let mut out = Vec::with_capacity(v.len() * 32);
    for i in v {
        for f in i
            .rect
            .iter()
            .chain(i.color.iter())
            .chain(std::iter::once(&i._pad))
        {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

fn glyph_bytes(v: &[GlyphInstance]) -> Vec<u8> {
    // 同上：stride 为 size_of::<GlyphInstance>() = 76 字节，必须含 _pad。
    let mut out = Vec::with_capacity(v.len() * 76);
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

#[cfg(test)]
mod backends_spec_tests {
    use super::parse_backends_spec;

    #[test]
    fn parses_single_names_case_insensitively() {
        assert_eq!(parse_backends_spec("dx12").unwrap(), wgpu::Backends::DX12);
        assert_eq!(parse_backends_spec("DX12").unwrap(), wgpu::Backends::DX12);
        assert_eq!(parse_backends_spec("vulkan").unwrap(), wgpu::Backends::VULKAN);
        assert_eq!(parse_backends_spec("vK").unwrap(), wgpu::Backends::VULKAN);
        assert_eq!(parse_backends_spec("d3d12").unwrap(), wgpu::Backends::DX12);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        // 这正是 Windows 上 `set X=dx12 && app` 会产生的尾随空格
        assert_eq!(parse_backends_spec("dx12 ").unwrap(), wgpu::Backends::DX12);
        assert_eq!(parse_backends_spec(" vulkan").unwrap(), wgpu::Backends::VULKAN);
        assert_eq!(parse_backends_spec("\tdx12\n").unwrap(), wgpu::Backends::DX12);
    }

    #[test]
    fn parses_comma_list_with_spaces() {
        assert_eq!(
            parse_backends_spec("vulkan,dx12").unwrap(),
            wgpu::Backends::VULKAN | wgpu::Backends::DX12
        );
        assert_eq!(
            parse_backends_spec(" vulkan , dx12 ").unwrap(),
            wgpu::Backends::VULKAN | wgpu::Backends::DX12
        );
        // 重复项不报错
        assert_eq!(
            parse_backends_spec("vulkan,vulkan").unwrap(),
            wgpu::Backends::VULKAN
        );
    }

    #[test]
    fn rejects_unknown_token_with_original_value() {
        for bad in ["dx", "vk12", "d3d11", "gpu", "乱码"] {
            let err = parse_backends_spec(bad).expect_err("无法识别的记号必须报错");
            assert!(err.contains(bad), "错误信息应包含原始值：{err}");
            assert!(err.contains("WGPU_BACKEND="), "错误信息应带变量名：{err}");
        }
        // 列表里混入未知项同样报错（不静默丢弃）
        let err = parse_backends_spec("vulkan,dx").expect_err("混入未知项必须报错");
        assert!(err.contains("dx"), "{err}");
    }

    #[test]
    fn rejects_empty_or_blank() {
        for bad in ["", "   ", ",", " , "] {
            assert!(
                parse_backends_spec(bad).is_err(),
                "空值/仅分隔符必须报错（实测 {bad:?}）"
            );
        }
    }

    #[test]
    fn accepts_all_supported_names() {
        for ok in ["metal", "mtl", "gl", "gles", "opengl", "webgpu"] {
            assert!(parse_backends_spec(ok).is_ok(), "{ok} 应可解析");
        }
    }
}
