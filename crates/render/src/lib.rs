//! render crate：渲染抽象。
//!
//! `Renderer` 只接收自有快照与像素尺寸，不暴露任何后端类型。
//!
//! 实现说明（阶段 2A 任务 3）：`CpuRenderer` 基于 fontdb（字体查找/回退链）+ fontdue
//! （位图光栅化），帧缓冲为自有 `Vec<u32>`；softbuffer 作为窗口 surface
//! 后端在 app 接线阶段接入。字体回退链、逐字符回退缓存、字形缓存、渲染选项
//! （抗锯齿/亚像素/hinting/text_gamma/粗体亮色）、样式属性与光标样式均在此实现。
//!
//! 缓存规则（文档注释，规格任务 3.2/3.4）：
//! - `char_font`：码点 → 组内字体索引。键为 (组索引, 码点)；字体列表与渲染选项在
//!   renderer 生命周期内不变，归属判定与缩放因子无关，故无需随 scale 失效；
//!   整体失效仅随 renderer 重建发生。
//! - `glyph_cache`：位图缓存，键 (组索引, 码点, px=字号*缩放, 选项指纹, gamma 位)；
//!   `set_scale_factor` 变化时整体清空（位图尺寸随 scale 变）。
//! - 逐字符回退顺序：组内 [主字体, font.fallback..., 系统 Monospace generic, Sans]
//!   取首个含该码点字形的字体；全无则用组首字体光栅化（fontdue 对缺字形返 default
//!   glyph，不 panic）。

use std::collections::HashMap;

use fontdb::{Database, Family, Query, Source, Style as DbStyle, Weight};
use fontdue::Font;
use vt::{Cell, CellWide, CursorStyle, FrameSnapshot, Rgb, Underline};

/// 渲染错误。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 字体系统错误。
    #[error("font error: {0}")]
    Font(String),
    /// 后端错误。
    #[error("backend error: {0}")]
    Backend(String),
}

/// Linux 默认等宽字体 family。
#[cfg(target_os = "linux")]
pub const DEFAULT_FONT_FAMILY: &str = "monospace";
/// Linux 默认字号（pt）。
#[cfg(target_os = "linux")]
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
/// Windows 默认等宽字体 family。
#[cfg(target_os = "windows")]
pub const DEFAULT_FONT_FAMILY: &str = "Consolas";
/// Windows 默认字号（pt）。
#[cfg(target_os = "windows")]
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// 渲染器输入：自有快照 + 像素尺寸。
pub trait Renderer {
    /// 渲染一帧。
    fn render(
        &mut self,
        frame: &FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<(), Error>;
}

/// 抗锯齿模式（font.antialiasing）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Antialiasing {
    /// 灰度抗锯齿（默认）。
    Grayscale,
    /// 无抗锯齿（二值化）。
    None,
}

/// 亚像素模式（font.subpixel）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subpixel {
    /// 无亚像素（默认）。
    None,
    /// LCD RGB 排列。
    Rgb,
    /// LCD BGR 排列。
    Bgr,
}

/// 字体渲染选项（font 段）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    pub antialiasing: Antialiasing,
    pub subpixel: Subpixel,
    /// fontdue 不支持 hinting：解析后不生效，new() 时打印一次 unsupported 日志。
    pub hinting: bool,
    /// 文字对比度（抗锯齿覆盖率 → alpha 映射）。
    pub text_gamma: f32,
    /// 粗体单元格使用 bright 色。
    pub draw_bold_with_bright: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            antialiasing: Antialiasing::Grayscale,
            subpixel: Subpixel::None,
            hinting: true,
            text_gamma: 1.0,
            draw_bold_with_bright: false,
        }
    }
}

impl RenderOptions {
    /// 选项指纹（入字形缓存键；任一选项变化即不中）。
    fn fingerprint(&self) -> u32 {
        let aa = match self.antialiasing {
            Antialiasing::Grayscale => 0u32,
            Antialiasing::None => 1,
        };
        let sp = match self.subpixel {
            Subpixel::None => 0u32,
            Subpixel::Rgb => 1,
            Subpixel::Bgr => 2,
        } << 1;
        let hint = u32::from(self.hinting) << 3;
        aa | sp | hint
    }
}

/// 颜色表（config::Colors → render 自有转换层，避免 render 依赖 config）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub primary_bg: Rgb,
    pub primary_fg: Rgb,
    pub cursor_text: Rgb,
    pub cursor: Rgb,
    pub normal: [Rgb; 8],
    pub bright: [Rgb; 8],
}

impl Default for Palette {
    fn default() -> Self {
        const fn c(r: u8, g: u8, b: u8) -> Rgb {
            Rgb { r, g, b }
        }
        Self {
            primary_bg: c(0, 0, 0),
            primary_fg: c(255, 255, 255),
            cursor_text: c(255, 255, 255),
            cursor: c(0, 0, 0),
            normal: [
                c(0, 0, 0),
                c(205, 0, 0),
                c(0, 205, 0),
                c(205, 205, 0),
                c(0, 0, 238),
                c(215, 0, 159),
                c(0, 205, 205),
                c(229, 229, 229),
            ],
            bright: [
                c(127, 127, 127),
                c(255, 0, 0),
                c(0, 255, 0),
                c(255, 255, 0),
                c(92, 92, 255),
                c(255, 0, 255),
                c(0, 255, 255),
                c(255, 255, 255),
            ],
        }
    }
}

/// 字体描述（family + style）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSpec {
    pub family: String,
    pub style: String,
}

/// 字体设置（config::FontSettings → render 自有转换层）。
#[derive(Debug, Clone)]
pub struct FontSettings {
    pub size: f32,
    pub normal: FontSpec,
    pub bold: FontSpec,
    pub italic: FontSpec,
    pub bold_italic: FontSpec,
    /// 有序回退列表（config font.fallback）。
    pub fallback: Vec<FontSpec>,
}

/// 由字体度量算格子尺寸（纯函数）：宽取 '0' 的 advance，高取
/// ascent+descent+line_gap（fontdue LineMetrics::new_line_size = ascent - descent + line_gap），
/// 均向上取整到整像素。scale 为 winit scale_factor（HiDPI 缩放）。
pub fn cell_metrics(font: &Font, size: f32, scale: f64) -> (u32, u32) {
    let px = size * scale as f32;
    let advance_w = font.metrics('0', px).advance_width;
    let w = advance_w.ceil() as u32;
    let h = match font.horizontal_line_metrics(px) {
        Some(lm) => lm.new_line_size.ceil() as u32,
        None => {
            // 字体缺行度量表：用 'M' 的轮廓高兜底，再兜底 px 向上取整。
            let m = font.metrics('M', px);
            let bounds_h = m.bounds.height.ceil() as u32;
            bounds_h.max(px.ceil() as u32)
        }
    };
    (w.max(1), h.max(1))
}

/// 字形缓存键：(组索引, 码点, px, 选项指纹, gamma 位)。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    group: u8,
    ch: char,
    px: u32,
    opts: u32,
    gamma_bits: u32,
}

struct CachedGlyph {
    metrics: fontdue::Metrics,
    bitmap: Vec<u8>,
}

/// 字体组序号（与 FontSettings 字段顺序一致）。
const GROUP_NORMAL: usize = 0;
const GROUP_BOLD: usize = 1;
const GROUP_ITALIC: usize = 2;
const GROUP_BOLD_ITALIC: usize = 3;

/// 基于 fontdb/fontdue 的 CPU 光栅化渲染器，多字体回退链 + 选项。
pub struct CpuRenderer {
    /// 四组字体 [normal, bold, italic, bold_italic]，每组 = [主字体, fallback..., 系统兜底]。
    groups: Vec<Vec<Font>>,
    font_size: f32,
    options: RenderOptions,
    palette: Palette,
    scale: f64,
    /// 码点 → 组内字体索引（逐字符回退缓存，规则见模块文档）。
    char_font: HashMap<(u8, char), usize>,
    glyph_cache: HashMap<GlyphKey, CachedGlyph>,
    framebuffer: Vec<u32>,
    fb_width: u32,
    fb_height: u32,
}

impl CpuRenderer {
    /// 新建渲染器：加载四组字体（normal 组主字体失败整体失败；其余组失败用 normal 组
    /// 字体代替；fallback/系统兜底失败跳过并记日志）。style 缺失依次回落 Bold→Italic→Regular。
    pub fn new(
        settings: &FontSettings,
        options: RenderOptions,
        palette: Palette,
    ) -> Result<Self, Error> {
        let mut db = Database::new();
        db.load_system_fonts();
        let specs = [
            &settings.normal,
            &settings.bold,
            &settings.italic,
            &settings.bold_italic,
        ];
        let mut groups: Vec<Vec<Font>> = Vec::with_capacity(4);
        for (gi, spec) in specs.iter().enumerate() {
            let main = match load_font_with_fallback(&db, &spec.family, &spec.style) {
                Ok(f) => f,
                Err(e) => {
                    if gi == GROUP_NORMAL {
                        return Err(e);
                    }
                    eprintln!(
                        "font group {} ('{}' '{}') load failed: {}; using normal font",
                        gi, spec.family, spec.style, e
                    );
                    groups[GROUP_NORMAL][0].clone()
                }
            };
            let mut fonts = vec![main];
            for fs in &settings.fallback {
                match load_font_with_fallback(&db, &fs.family, &fs.style) {
                    Ok(f) => fonts.push(f),
                    Err(e) => eprintln!(
                        "font fallback '{}' '{}' load failed: {}; skipped",
                        fs.family, fs.style, e
                    ),
                }
            }
            // 系统兜底：Monospace generic → SansSerif（fontdb generic 链，规格任务 3.2）。
            for fam in [Family::Monospace, Family::SansSerif] {
                match load_font_generic(&db, fam) {
                    Ok(f) => fonts.push(f),
                    Err(e) => eprintln!("system fallback {fam:?} unavailable: {e}"),
                }
            }
            groups.push(fonts);
        }
        if options.hinting {
            eprintln!("hinting: fontdue 不支持 hinting，已解析未生效（unsupported）");
        }
        Ok(Self {
            groups,
            font_size: settings.size,
            options,
            palette,
            scale: 1.0,
            char_font: HashMap::new(),
            glyph_cache: HashMap::new(),
            framebuffer: Vec::new(),
            fb_width: 0,
            fb_height: 0,
        })
    }

    /// 缩放因子变化：更新 scale 并整体清空字形位图缓存（位图尺寸随 scale 变；
    /// 码点→字体归属缓存与 scale 无关，不清）。
    pub fn set_scale_factor(&mut self, scale: f64) {
        if (scale - self.scale).abs() > f64::EPSILON {
            self.scale = scale;
            self.glyph_cache.clear();
        }
    }

    /// 每格组内字体索引（dump 钩子用；&self 不写缓存，避免与 render 热路径借用冲突）。
    pub fn font_indices(&self, frame: &FrameSnapshot) -> Vec<Vec<usize>> {
        let mut out = Vec::with_capacity(frame.rows as usize);
        for row in &frame.cells {
            let mut line = Vec::with_capacity(row.len());
            for cell in row {
                let ch = cell.chars.chars().next().unwrap_or(' ');
                let group = group_for(cell);
                let idx = self.groups[group as usize]
                    .iter()
                    .position(|f| f.has_glyph(ch))
                    .unwrap_or(0);
                line.push(idx);
            }
            out.push(line);
        }
        out
    }

    /// 当前帧缓冲（ARGB 打包为 u32，低 24 位为 RGB）。
    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    /// 帧缓冲尺寸。
    pub fn framebuffer_size(&self) -> (u32, u32) {
        (self.fb_width, self.fb_height)
    }

    /// 当前字体在 scale 下的格子尺寸（物理像素）；字体未加载时回落阶段1 常量 8x16。
    pub fn cell_metrics(&self, scale: f64) -> (u32, u32) {
        match self.groups.first().and_then(|g| g.first()) {
            Some(f) => cell_metrics(f, self.font_size, scale),
            None => (8, 16),
        }
    }

    /// 码点在组内的字体索引（回退链：主字体 → fallback → 系统 Monospace/Sans）。
    fn font_index_for(&mut self, group: u8, ch: char) -> usize {
        if let Some(&i) = self.char_font.get(&(group, ch)) {
            return i;
        }
        let idx = self.groups[group as usize]
            .iter()
            .position(|f| f.has_glyph(ch))
            .unwrap_or(0);
        self.char_font.insert((group, ch), idx);
        idx
    }

    /// 光栅化（带缓存）：px = 字号 * 缩放因子。
    fn rasterize_at(&mut self, group: u8, font_idx: usize, ch: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        let key = GlyphKey {
            group,
            ch,
            px: px.ceil() as u32,
            opts: self.options.fingerprint(),
            gamma_bits: self.options.text_gamma.to_bits(),
        };
        if let Some(c) = self.glyph_cache.get(&key) {
            return (c.metrics, c.bitmap.clone());
        }
        let font = &self.groups[group as usize][font_idx];
        let (m, b) = font.rasterize(ch, px);
        self.glyph_cache.insert(
            key,
            CachedGlyph {
                metrics: m,
                bitmap: b.clone(),
            },
        );
        (m, b)
    }
}

/// 单元格属性 → 字体组序号（bold/italic 组合映射四组）。
fn group_for(cell: &Cell) -> u8 {
    match (cell.bold, cell.italic) {
        (true, true) => GROUP_BOLD_ITALIC as u8,
        (true, false) => GROUP_BOLD as u8,
        (false, true) => GROUP_ITALIC as u8,
        (false, false) => GROUP_NORMAL as u8,
    }
}

impl Renderer for CpuRenderer {
    fn render(
        &mut self,
        frame: &FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<(), Error> {
        let px = (self.font_size * self.scale as f32).max(0.1);
        let cols = u32::from(frame.cols).max(1);
        let rows = u32::from(frame.rows).max(1);
        let cell_w = (pixel_width / cols).max(1);
        let cell_h = (pixel_height / rows).max(1);
        self.fb_width = pixel_width;
        self.fb_height = pixel_height;
        self.framebuffer = vec![0; (pixel_width * pixel_height) as usize];
        let bg_default = rgb_to_u32(self.palette.primary_bg);
        let gamma = self.options.text_gamma;
        let sub = self.options.subpixel;

        for (y, row) in frame.cells.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                let px0 = x as u32 * cell_w;
                let py0 = y as u32 * cell_h;
                // SpacerTail/SpacerHead 不绘制字形；背景仍填充（宽字符背景跨 2 格）。
                let is_tail = matches!(
                    cell.wide,
                    CellWide::SpacerTail | CellWide::SpacerHead
                );
                let span = if cell.wide == CellWide::Wide && x + 1 < row.len() {
                    2
                } else {
                    1
                };
                let area_w = cell_w * span;
                let bg = cell.bg.map(rgb_to_u32).unwrap_or(bg_default);
                fill_rect(&mut self.framebuffer, pixel_width, px0, py0, area_w, cell_h, bg);
                if is_tail || cell.chars.is_empty() || cell.invisible {
                    continue;
                }

                // 颜色：默认前景/反显交换/粗体亮色/faint 降不透明度。
                let mut fg = cell
                    .fg
                    .map(rgb_to_u32)
                    .unwrap_or(rgb_to_u32(self.palette.primary_fg));
                let mut bgc = bg;
                if cell.inverse {
                    std::mem::swap(&mut fg, &mut bgc);
                }
                if cell.bold && self.options.draw_bold_with_bright {
                    fg = map_to_bright(fg, &self.palette);
                }
                if cell.faint {
                    fg = dim(fg, bgc);
                }

                let ch = cell.chars.chars().next().expect("非空 chars");
                let group = group_for(cell);
                let font_idx = self.font_index_for(group, ch);

                if span == 2 && sub == Subpixel::None {
                    // 宽字符：缩放到 2 格宽高内（不放大），水平居中，基线随位图 ymin 对齐。
                    let (m, bitmap) = self.rasterize_at(group, font_idx, ch, px);
                    let bw = m.width.max(1) as f32;
                    let bh = m.height.max(1) as f32;
                    let s = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
                    if (s - 1.0).abs() < f32::EPSILON {
                        blit(
                            &mut self.framebuffer,
                            pixel_width,
                            (px0, py0, area_w, cell_h),
                            &m,
                            &bitmap,
                            fg,
                            bg,
                            self.options.antialiasing,
                            gamma,
                        );
                    } else {
                        let sw = (bw * s).ceil() as usize;
                        let sh = (bh * s).ceil() as usize;
                        let scaled = scale_bitmap(&bitmap, m.width, m.height, sw, sh);
                        let ox = (area_w as i32 - sw as i32) / 2;
                        let sm = fontdue::Metrics {
                            xmin: (m.xmin as f32 * s).ceil() as i32 + ox,
                            ymin: (m.ymin as f32 * s).ceil() as i32,
                            width: sw,
                            height: sh,
                            advance_width: m.advance_width * s,
                            advance_height: m.advance_height * s,
                            bounds: m.bounds,
                        };
                        blit(
                            &mut self.framebuffer,
                            pixel_width,
                            (px0, py0, area_w, cell_h),
                            &sm,
                            &scaled,
                            fg,
                            bg,
                            self.options.antialiasing,
                            gamma,
                        );
                    }
                } else if span == 2 {
                    // 宽字符 + 亚像素：3x 水平采样光栅（px*3），缩放到 2 格内。
                    let (m, bitmap) = self.rasterize_at(group, font_idx, ch, px * 3.0);
                    let bw = m.width.max(1) as f32 / 3.0;
                    let bh = m.height.max(1) as f32 / 3.0;
                    let s = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
                    let sw = (bw * s).ceil() as usize;
                    let sh = (bh * s).ceil() as usize;
                    let scaled = scale_bitmap(&bitmap, m.width, m.height, sw * 3, sh * 3);
                    let ox = (area_w as i32 - sw as i32) / 2;
                    let sm = fontdue::Metrics {
                        xmin: (m.xmin as f32 / 3.0 * s).ceil() as i32 + ox * 3,
                        ymin: (m.ymin as f32 / 3.0 * s).ceil() as i32 * 3,
                        width: sw * 3,
                        height: sh * 3,
                        advance_width: m.advance_width * s,
                        advance_height: m.advance_height * s,
                        bounds: m.bounds,
                    };
                    blit_subpixel(
                        &mut self.framebuffer,
                        pixel_width,
                        (px0, py0, area_w, cell_h),
                        &sm,
                        &scaled,
                        fg,
                        sub,
                        gamma,
                    );
                } else {
                    let (m, bitmap) = self.rasterize_at(group, font_idx, ch, px);
                    blit(
                        &mut self.framebuffer,
                        pixel_width,
                        (px0, py0, area_w, cell_h),
                        &m,
                        &bitmap,
                        fg,
                        bg,
                        self.options.antialiasing,
                        gamma,
                    );
                }

                // 样式线：下划线（Single/Double/Dotted/Dashed；Curly 近似 Single）、删除线、上划线。
                let line_color = if cell.inverse { bgc } else { fg };
                draw_style_lines(
                    &mut self.framebuffer,
                    pixel_width,
                    px0,
                    py0,
                    area_w,
                    cell_h,
                    cell,
                    line_color,
                );
            }
        }

        self.draw_cursor(frame, cell_w, cell_h, px);
        Ok(())
    }
}

impl CpuRenderer {
    /// 光标样式（块/空心块/下划线/竖线）按快照光标样式与配置颜色绘制。
    fn draw_cursor(&mut self, frame: &FrameSnapshot, cell_w: u32, cell_h: u32, px: f32) {
        let cur = &frame.cursor;
        if !cur.visible {
            return;
        }
        let cx = u32::from(cur.x) * cell_w;
        let cy = u32::from(cur.y) * cell_h;
        let width = self.fb_width;
        let color = rgb_to_u32(self.palette.cursor);
        match cur.style {
            CursorStyle::Block => {
                fill_rect(&mut self.framebuffer, width, cx, cy, cell_w, cell_h, color);
                // 光标格字符以 cursor.text 色重画（glyph_cache 命中同键位图）。
                if let Some(row) = frame.cells.get(cur.y as usize)
                    && let Some(cell) = row.get(cur.x as usize)
                    && !cell.chars.is_empty()
                {
                    let ch = cell.chars.chars().next().expect("非空 chars");
                    let group = group_for(cell);
                    let font_idx = self.font_index_for(group, ch);
                    let (m, bitmap) = self.rasterize_at(group, font_idx, ch, px);
                    blit(
                        &mut self.framebuffer,
                        width,
                        (cx, cy, cell_w, cell_h),
                        &m,
                        &bitmap,
                        rgb_to_u32(self.palette.cursor_text),
                        color,
                        self.options.antialiasing,
                        self.options.text_gamma,
                    );
                }
            }
            CursorStyle::HollowBlock => {
                // 边框 1px：上下横线 + 左右竖线。
                for i in 0..cell_w {
                    self.plot(cx + i, cy, color);
                    self.plot(cx + i, cy + cell_h - 1, color);
                }
                for j in 0..cell_h {
                    self.plot(cx, cy + j, color);
                    self.plot(cx + cell_w - 1, cy + j, color);
                }
            }
            CursorStyle::Underline => {
                let y = cy + cell_h - 1;
                for i in 0..cell_w {
                    self.plot(cx + i, y, color);
                    self.plot(cx + i, y.saturating_sub(1), color);
                }
            }
            CursorStyle::Bar => {
                for j in 0..cell_h {
                    self.plot(cx, cy + j, color);
                    self.plot(cx + 1, cy + j, color);
                }
            }
        }
    }

    /// 单像素写（裁剪到帧缓冲）。
    fn plot(&mut self, x: u32, y: u32, color: u32) {
        if x < self.fb_width && y < self.fb_height {
            let idx = (y * self.fb_width + x) as usize;
            if idx < self.framebuffer.len() {
                self.framebuffer[idx] = color;
            }
        }
    }
}

/// 样式线：下划线/删除线/上划线。
fn draw_style_lines(
    fb: &mut [u32],
    width: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    cell: &Cell,
    color: u32,
) {
    let mut hline = |y: i32, gap: u32| {
        if y < 0 || y >= h as i32 {
            return;
        }
        for i in 0..w {
            if i % gap == 0 {
                plot_fb(fb, width, x0 + i, y0 + y as u32, color);
            }
        }
    };
    match cell.underline {
        Underline::None => {}
        Underline::Single => hline(h as i32 - 2, 1),
        Underline::Double => {
            hline(h as i32 - 2, 1);
            hline(h as i32 - 4, 1);
        }
        Underline::Dotted => hline(h as i32 - 2, 2),
        Underline::Dashed => hline(h as i32 - 2, 4),
        // Curly 无波状近似实现，按 Single 处理（DECISIONS 登记）。
        Underline::Curly => hline(h as i32 - 2, 1),
    }
    if cell.strikethrough {
        hline(h as i32 / 2, 1);
    }
    if cell.overline {
        hline(1, 1);
    }
}

fn plot_fb(fb: &mut [u32], width: u32, x: u32, y: u32, color: u32) {
    if x < width {
        let idx = (y * width + x) as usize;
        if idx < fb.len() {
            fb[idx] = color;
        }
    }
}

fn rgb_to_u32(c: Rgb) -> u32 {
    0xFF00_0000 | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

fn fill_rect(fb: &mut [u32], width: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    for row in y..y + h {
        for col in x..x + w {
            if col < width && row < fb.len() as u32 / width.max(1) {
                fb[(row * width + col) as usize] = color;
            }
        }
    }
}

/// 灰度/二值化路径的 blit：裁剪到单元格矩形，背景混合。
fn blit(
    fb: &mut [u32],
    width: u32,
    cell: (u32, u32, u32, u32),
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    fg: u32,
    bg: u32,
    antialiasing: Antialiasing,
    gamma: f32,
) {
    let (cell_x, cell_y, cell_w, cell_h) = cell;
    let bw = metrics.width as i32;
    let bh = metrics.height as i32;
    for gy in 0..bh {
        for gx in 0..bw {
            let alpha = bitmap[(gy as usize) * metrics.width + gx as usize];
            if alpha == 0 {
                continue;
            }
            let px = cell_x as i32 + gx + metrics.xmin;
            let py = cell_y as i32 + gy + metrics.ymin;
            // 裁剪：只绘制落在当前单元格矩形 [cell_x, cell_x+cell_w) x [cell_y, cell_y+cell_h) 内的像素；
            // 负 xmin/ymin 或超宽超高位图不得侵入邻格。
            if px < cell_x as i32
                || px >= (cell_x + cell_w) as i32
                || py < cell_y as i32
                || py >= (cell_y + cell_h) as i32
            {
                continue;
            }
            let idx = (py as u32 * width + px as u32) as usize;
            if idx >= fb.len() {
                continue;
            }
            let a = match antialiasing {
                Antialiasing::Grayscale => alpha,
                Antialiasing::None => {
                    if alpha > 127 {
                        255
                    } else {
                        0
                    }
                }
            };
            if a == 0 {
                continue;
            }
            fb[idx] = blend_gamma(fg, bg, a, gamma);
        }
    }
}

/// 亚像素路径（3x 水平采样）：位图宽/高为输出的 3 倍，每输出像素取连续 3 列作
/// R/G/B 覆盖（垂直方向取 3 行中间行），三通道独立混合。
fn blit_subpixel(
    fb: &mut [u32],
    width: u32,
    cell: (u32, u32, u32, u32),
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    fg: u32,
    order: Subpixel,
    gamma: f32,
) {
    let (cell_x, cell_y, cell_w, cell_h) = cell;
    let bw3 = metrics.width as i32;
    let bh3 = metrics.height as i32;
    let stride3 = metrics.width;
    for gy3 in 0..bh3 {
        if gy3 % 3 != 1 {
            continue; // 垂直方向取 3 行中间行
        }
        for gx3 in 0..bw3 {
            let alpha = bitmap[(gy3 as usize) * stride3 + gx3 as usize];
            if alpha == 0 {
                continue;
            }
            let sx = gx3 / 3;
            let sub = gx3 % 3; // 0=R,1=G,2=B（位图列序）
            let px = cell_x as i32 + sx + metrics.xmin / 3;
            let py = cell_y as i32 + gy3 / 3 + metrics.ymin / 3;
            if px < cell_x as i32
                || px >= (cell_x + cell_w) as i32
                || py < cell_y as i32
                || py >= (cell_y + cell_h) as i32
            {
                continue;
            }
            let idx = (py as u32 * width + px as u32) as usize;
            if idx >= fb.len() {
                continue;
            }
            // LCD 排列：Bgr 时位图列序与通道序互换。
            let channel = match (order, sub) {
                (Subpixel::Rgb, 0) | (Subpixel::Bgr, 2) => 0,
                (Subpixel::Rgb, 1) | (Subpixel::Bgr, 1) => 1,
                _ => 2,
            };
            fb[idx] = blend_channel(fb[idx], fg, alpha, channel, gamma);
        }
    }
}

/// 按 alpha 混合前景色到背景（fg*a + bg*(1-a)，gamma 映射覆盖率）。
fn blend_gamma(fg: u32, bg: u32, alpha: u8, gamma: f32) -> u32 {
    let t = (u32::from(alpha) as f32 / 255.0).powf(1.0 / gamma.max(0.01));
    let a = (t * 255.0).round() as u32;
    let ia = 255 - a;
    let mix = |f: u32, b: u32| (f * a + b * ia) / 255;
    0xFF00_0000
        | (mix((fg >> 16) & 0xFF, (bg >> 16) & 0xFF) << 16)
        | (mix((fg >> 8) & 0xFF, (bg >> 8) & 0xFF) << 8)
        | mix(fg & 0xFF, bg & 0xFF)
}

/// 亚像素单通道混合：仅 channel 通道按 alpha 混合，其余通道保持背景。
fn blend_channel(pixel: u32, fg: u32, alpha: u8, channel: u32, gamma: f32) -> u32 {
    let t = (u32::from(alpha) as f32 / 255.0).powf(1.0 / gamma.max(0.01));
    let a = (t * 255.0).round() as u32;
    let shift = match channel {
        0 => 16,
        1 => 8,
        _ => 0,
    };
    let f = (fg >> shift) & 0xFF;
    let b = (pixel >> shift) & 0xFF;
    let c = (f * a + b * (255 - a)) / 255;
    (pixel & !(0xFF << shift)) | (c << shift)
}

/// faint：前景与背景 50% 混合。
fn dim(fg: u32, bg: u32) -> u32 {
    let r = ((fg >> 16) & 0xFF) / 2 + ((bg >> 16) & 0xFF) / 2;
    let g = ((fg >> 8) & 0xFF) / 2 + ((bg >> 8) & 0xFF) / 2;
    let b = (fg & 0xFF) / 2 + (bg & 0xFF) / 2;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

/// 粗体亮色映射：fg 若等于 normal[i] 则替换为 bright[i]，否则不变。
fn map_to_bright(fg: u32, palette: &Palette) -> u32 {
    for i in 0..8 {
        if fg == rgb_to_u32(palette.normal[i]) {
            return rgb_to_u32(palette.bright[i]);
        }
    }
    fg
}

/// 位图最近邻缩放（宽字符缩放到 2 格宽高内）。
fn scale_bitmap(bitmap: &[u8], w: usize, h: usize, tw: usize, th: usize) -> Vec<u8> {
    if tw == 0 || th == 0 || w == 0 || h == 0 {
        return Vec::new();
    }
    let mut out = vec![0u8; tw * th];
    for y in 0..th {
        for x in 0..tw {
            let sx = (x * w / tw).min(w - 1);
            let sy = (y * h / th).min(h - 1);
            out[y * tw + x] = bitmap[sy * w + sx];
        }
    }
    out
}

/// style 字符串 → fontdb 查询用 (weight, style)。
fn style_query(style: &str) -> (Weight, DbStyle) {
    match style {
        "Bold" => (Weight::BOLD, DbStyle::Normal),
        "Italic" => (Weight::NORMAL, DbStyle::Italic),
        "Bold Italic" => (Weight::BOLD, DbStyle::Italic),
        _ => (Weight::NORMAL, DbStyle::Normal),
    }
}

/// 按 family + style 精确加载字体（Name 精确优先，Monospace generic 兜底；阶段1 决策：family 精确查找，generic 链仅兜底）。
fn load_font_styled(db: &Database, family: &str, style: &str) -> Result<Font, Error> {
    let (w, s) = style_query(style);
    let query = Query {
        families: &[Family::Name(family), Family::Monospace],
        weight: w,
        style: s,
        ..Default::default()
    };
    let id = db
        .query(&query)
        .ok_or_else(|| Error::Font(format!("family '{}' style '{}' not found", family, style)))?;
    let face = db
        .face(id)
        .ok_or_else(|| Error::Font("face not found".to_string()))?;
    let bytes: Vec<u8> = match &face.source {
        Source::File(path) => std::fs::read(path)
            .map_err(|e| Error::Font(format!("read font file: {}", e)))?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };
    Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
        .map_err(|e| Error::Font(format!("fontdue parse: {}", e)))
}

/// 按 family 加载字体，style 缺失依次回落 Bold→Italic→Regular（规格任务 3.1），
/// 回落时记日志；不合成粗体/斜体。
fn load_font_with_fallback(db: &Database, family: &str, style: &str) -> Result<Font, Error> {
    let mut candidates = vec![
        style.to_string(),
        "Bold".to_string(),
        "Italic".to_string(),
        "Regular".to_string(),
    ];
    candidates.dedup();
    let mut last_err = None;
    for s in &candidates {
        match load_font_styled(db, family, s) {
            Ok(f) => {
                if s != style {
                    eprintln!("font style fallback: '{}' '{}' → '{}'", family, style, s);
                }
                return Ok(f);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Font(format!("family '{}' not found", family))))
}

/// 系统 generic 字体加载（回退链末档）。
fn load_font_generic(db: &Database, family: Family) -> Result<Font, Error> {
    let query = Query {
        families: &[family],
        ..Default::default()
    };
    let id = db
        .query(&query)
        .ok_or_else(|| Error::Font(format!("generic '{family:?}' not found")))?;
    let face = db
        .face(id)
        .ok_or_else(|| Error::Font("face not found".to_string()))?;
    let bytes: Vec<u8> = match &face.source {
        Source::File(path) => std::fs::read(path)
            .map_err(|e| Error::Font(format!("read font file: {}", e)))?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };
    Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
        .map_err(|e| Error::Font(format!("fontdue parse: {}", e)))
}

#[cfg(test)]
mod blit_tests {
    use super::blit;
    use super::Antialiasing;

    const BG: u32 = 0xFF00_0000;
    const FG: u32 = 0xFF00_00FF;

    fn metrics(xmin: i32, ymin: i32, width: usize, height: usize) -> fontdue::Metrics {
        fontdue::Metrics {
            xmin,
            ymin,
            width,
            height,
            ..Default::default()
        }
    }

    /// 负 xmin + 超宽位图：只画本格，右邻格像素不变。
    #[test]
    fn blit_clips_negative_offset_and_oversize_width_to_cell() {
        let width = 16u32; // 两格各 8 宽，高 4
        let mut fb = vec![BG; (width * 4) as usize];
        let m = metrics(-3, 0, 20, 2); // 负偏移 + 超宽
        let bitmap = vec![255u8; 20 * 2];
        blit(
            &mut fb,
            width,
            (0, 0, 8, 4),
            &m,
            &bitmap,
            FG,
            BG,
            Antialiasing::Grayscale,
            1.0,
        );
        // 本格 [0,8) 列有着色
        assert!(fb[0..8].contains(&FG), "本格内应有前景像素");
        // 右邻格 [8,16) 列全背景
        let invaded = fb[8..16].iter().filter(|&&p| p != BG).count();
        assert_eq!(
            invaded, 0,
            "右邻格应全为背景色，被侵入像素 {}",
            invaded
        );
    }

    /// 超高位图：只画本格，下邻格像素不变。
    #[test]
    fn blit_clips_oversize_height_to_cell() {
        let width = 8u32;
        let mut fb = vec![BG; (width * 8) as usize];
        let m = metrics(0, 0, 2, 20); // 超高
        let bitmap = vec![255u8; 2 * 20];
        blit(
            &mut fb,
            width,
            (0, 0, 8, 4),
            &m,
            &bitmap,
            FG,
            BG,
            Antialiasing::Grayscale,
            1.0,
        );
        let invaded = fb[4 * 8..].iter().filter(|&&p| p != BG).count();
        assert_eq!(invaded, 0, "下邻格应全为背景色");
    }
}
