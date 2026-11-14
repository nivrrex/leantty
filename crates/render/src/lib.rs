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

pub mod decide;
pub mod preedit;
#[cfg(target_os = "linux")]
pub mod gpu;

/// 进程启动锚点（阶段 3A 任务 2：启动分段耗时日志；app 在 main 首行调用）。
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 记录渲染侧启动锚点（重复调用以首次为准）。
pub fn mark_start() {
    let _ = START.set(std::time::Instant::now());
}

/// 相对启动锚点的毫秒数（未设置锚点时返 0）。
fn since_start_ms() -> f64 {
    START
        .get()
        .map(|t| t.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

pub use decide::{decide, CellDecision, DecideCtx};
pub use preedit::{layout_end_position, layout_preedit, PreeditLayout, PreeditOverlay};

use std::collections::HashMap;

use fontdb::{Database, Family, Query, Source, Style as DbStyle, Weight};
pub use fontdue::Font;
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

/// 滚动条宽度（像素，右缘竖直条带）：4（阶段 3A）→ 8（4B-fix2）→ **12**（4B-fix3）。
/// 用户两次反馈"太窄、鼠标不容易点中"；12px 在 1x/2x 下都更易命中。
/// CPU 绘制、GPU 绘制、条带备份、app 命中测试共用该常量（唯一来源）。
pub const SCROLLBAR_WIDTH: u32 = 12;

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

    /// 设置 IME 预编辑叠加层（不写入终端网格；None=清除）。
    ///
    /// 默认空实现：Windows 侧共用本 trait，新增方法不破坏既有实现
    /// （DECISIONS 登记 trait 扩展）。
    fn set_preedit(&mut self, _overlay: Option<PreeditOverlay>) {}
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
/// 后台加载 fallback 与系统兜底字体（fallback specs + Monospace/SansSerif generic），
/// 返回按序字体列表。线程安全：仅 owned 数据（FontSpec/Font）经 channel 传递；
/// Database 在调用线程内自建（fontdb Database 非共享设计）。
pub fn load_font_list(fallbacks: &[FontSpec]) -> Vec<Font> {
    let mut db = Database::new();
    db.load_system_fonts();
    let mut fonts = Vec::new();
    for fs in fallbacks {
        match load_font_with_fallback(&db, &fs.family, &fs.style) {
            Ok(f) => fonts.push(f),
            Err(e) => eprintln!(
                "font fallback '{}' '{}' load failed: {}; skipped",
                fs.family, fs.style, e
            ),
        }
    }
    for fam in [Family::Monospace, Family::SansSerif] {
        match load_font_generic(&db, fam) {
            Ok(f) => fonts.push(f),
            Err(e) => eprintln!("system fallback {fam:?} unavailable: {e}"),
        }
    }
    fonts
}

/// 构建四组字体链（CPU 与 GPU 后端共用）：每组 = [主字体, 后台 fallback..., 系统兜底]。
///
/// 主字体同步加载（首帧需渲染 ASCII）；fallback 与系统兜底由 app 层后台加载后
/// 经 `add_background_fonts` 并入。返回顺序固定为 [normal, bold, italic, bold_italic]。
pub(crate) fn build_font_groups(settings: &FontSettings) -> Result<Vec<Vec<Font>>, Error> {
    let mut db = Database::new();
    let t_db = std::time::Instant::now();
    db.load_system_fonts();
    eprintln!(
        "[perf] fontdb_ms={:.1} since_start={:.1}",
        t_db.elapsed().as_secs_f64() * 1000.0,
        since_start_ms()
    );
    let specs = [
        &settings.normal,
        &settings.bold,
        &settings.italic,
        &settings.bold_italic,
    ];
    let mut groups: Vec<Vec<Font>> = Vec::with_capacity(4);
    // 同一 face 只解析一次（四组可能指向同一文件，例如 style 回落）。
    let mut face_cache: std::collections::HashMap<fontdb::ID, Font> =
        std::collections::HashMap::new();
    for (gi, spec) in specs.iter().enumerate() {
        let t_font = std::time::Instant::now();
        // 阶段 4J t4：每组起止点（Windows 上 4 组合计约 400ms，是首帧最大单项）。
        eprintln!(
            "[perf] font_group_begin_ms group={} family='{}' style='{}' since_start={:.1}",
            gi,
            spec.family,
            spec.style,
            since_start_ms()
        );
        // 阶段 4M t3：组内三段（begin→load 调用、load 调用本身、load 返回→组尾），
        // 微秒精度，用于定位 Windows 上 group_begin→seg / seg→cache / cache→end 的未归属耗时。
        let t_before_load = std::time::Instant::now();
        let pre_load_us = t_before_load.duration_since(t_font).as_micros();
        let load_result =
            load_font_with_fallback_cached(&db, &spec.family, &spec.style, &mut face_cache);
        let load_us = t_before_load.elapsed().as_micros();
        let main = match load_result
        {
            Ok((f, cached, used_style)) => {
                // 回退面（style 回落时 used_style != 请求 style）单独标注，便于区分
                // "主字体面" 与 "回退字体面" 的成本。
                let tail_us = t_before_load.elapsed().as_micros().saturating_sub(load_us);
                eprintln!(
                    "[perf] font_group_us group={} family='{}' style='{}' pre_load_us={} load_us={} tail_us={} total_us={} fallback={}",
                    gi,
                    spec.family,
                    spec.style,
                    pre_load_us,
                    load_us,
                    tail_us,
                    t_font.elapsed().as_micros(),
                    used_style != spec.style
                );
                eprintln!(
                    "[perf] font_group_ms group={} family='{}' style='{}' used_style='{}' fallback={} total_ms={:.2} since_start={:.1}",
                    gi,
                    spec.family,
                    spec.style,
                    used_style,
                    used_style != spec.style,
                    t_font.elapsed().as_secs_f64() * 1000.0,
                    since_start_ms()
                );
                eprintln!(
                    "[perf] font_ms={} family='{}' style='{}' used_style='{}' cached={} ms={:.1} since_start={:.1}",
                    gi,
                    spec.family,
                    spec.style,
                    used_style,
                    cached,
                    t_font.elapsed().as_secs_f64() * 1000.0,
                    since_start_ms()
                );
                f
            }
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
        groups.push(vec![main]);
    }
    Ok(groups)
}
pub struct CpuRenderer {
    /// 四组字体 [normal, bold, italic, bold_italic]，每组 = [主字体, fallback..., 系统兜底]。
    ///
    /// 用 `Arc` 共享：auto/gpu 模式下窗口前先建 CPU 渲染器、窗口后再建 GPU 渲染器，
    /// 两者复用同一份字体链，避免重复 fontdb 扫描 + 字体解析（阶段 4E t3 实测
    /// 该重复约占 GPU 首帧的 155ms）。
    groups: std::sync::Arc<Vec<Vec<Font>>>,
    font_size: f32,
    options: RenderOptions,
    palette: Palette,
    scale: f64,
    /// 码点 → 组内字体索引（逐字符回退缓存，规则见模块文档；None=链中暂无字体
    /// 含该码点——后台字体未到位或全链缺失，渲染层统一画 U+25A1 占位）。
    char_font: HashMap<(u8, char), Option<usize>>,
    glyph_cache: HashMap<GlyphKey, CachedGlyph>,
    framebuffer: Vec<u32>,
    fb_width: u32,
    fb_height: u32,
    /// 选区高亮矩形 (min_col, min_row, max_col, max_row)，含端点；None=无选区。
    /// 由 app 层随选区变化调用 set_selection 更新；渲染层只做反色绘制。
    selection: Option<(u16, u16, u16, u16)>,
    /// 滚动条拇指几何 (thumb_y, thumb_h)，轨道内 px；None=无滚动条。
    /// 由 app 层每帧 set_scrollbar 更新；渲染层只画轨道+拇指。
    scrollbar: Option<(u32, u32)>,
    /// IME 预编辑叠加层（不写入网格，绘制在光标处）。
    preedit: Option<PreeditOverlay>,
    /// 强制整屏重绘（缩放/字体集变化等由渲染器自身感知的结构变化）。
    force_full: bool,
    /// 上次绘制的帧缓冲尺寸（用于识别 resize）。
    last_shape: Option<(u32, u32)>,
    /// 上次绘制时的 scale 位表示（识别 HiDPI 变化）。
    last_scale_bits: u64,
    /// 上次绘制的光标状态（位置/可见性/样式），用于叠加层脏判定。
    last_cursor: Option<(u16, u16, bool, u8)>,
    /// 上次绘制的滚动条几何。
    last_scrollbar: Option<(u32, u32)>,
    /// 上次绘制的预编辑布局键（行, 列, 单元数）。
    last_preedit: Option<(u16, u16, u16, usize, u64)>,
    /// 滚动条条带（右缘 4px）在"不含滚动条时"的像素备份。
    band_backup: Option<Vec<u32>>,
    /// 本帧需要提交给窗口系统的矩形区域 (x, y, w, h)，非空。
    damage: Vec<(u32, u32, u32, u32)>,
}

impl CpuRenderer {
    /// 新建渲染器：加载四组字体（normal 组主字体失败整体失败；其余组失败用 normal 组
    /// 字体代替；fallback/系统兜底失败跳过并记日志）。style 缺失依次回落 Bold→Italic→Regular。
    pub fn new(
        settings: &FontSettings,
        options: RenderOptions,
        palette: Palette,
    ) -> Result<Self, Error> {
        let t_ctor = std::time::Instant::now();
        // 阶段 4H t2：CpuRenderer 内部逐段打点（Windows 首帧 781ms 中这段此前无观测）。
        // 入口先报一次 since_start，便于判断"进入 new 之前"是否已耗时。
        eprintln!(
            "[perf] cpu_new_begin_ms={:.1} since_start={:.1}",
            0.0,
            since_start_ms()
        );
        let t_groups = std::time::Instant::now();
        let groups = std::sync::Arc::new(build_font_groups(settings)?);
        eprintln!(
            "[perf] cpu_groups_ms={:.1} since_start={:.1}",
            t_groups.elapsed().as_secs_f64() * 1000.0,
            since_start_ms()
        );
        if options.hinting {
            eprintln!("hinting: fontdue 不支持 hinting，已解析未生效（unsupported）");
        }
        // 阶段 4E t2：字体加载完成 → 字形缓存/帧缓冲等结构就绪的打点
        eprintln!(
            "[perf] cpu_renderer_ready_ms={:.1} since_start={:.1}",
            t_ctor.elapsed().as_secs_f64() * 1000.0,
            since_start_ms()
        );
        let t_state = std::time::Instant::now();
        let me = Ok(Self {
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
            selection: None,
            scrollbar: None,
            preedit: None,
            force_full: true,
            last_shape: None,
            last_scale_bits: 0,
            last_cursor: None,
            last_scrollbar: None,
            last_preedit: None,
            band_backup: None,
            damage: Vec::new(),
        });
        eprintln!(
            "[perf] cpu_state_ms={:.2} since_start={:.1}",
            t_state.elapsed().as_secs_f64() * 1000.0,
            since_start_ms()
        );
        me
    }





    /// 基线相对格顶的偏移（像素）。
    ///
    /// 取 `cell_h - ceil(-descent) - ceil(line_gap)`：既保证下行字形（g/p/y）落在
    /// 格内不被裁剪，又保证字形上缘不越界。等价于"ascent 向上取整后再为 descent
    /// 与 line_gap 让出空间"，与 `cell_metrics` 的
    /// `cell_h = ceil(ascent - descent + line_gap)` 配套。
    fn baseline_px(&self, px: f32, cell_h: u32) -> i32 {
        self.groups
            .first()
            .and_then(|g| g.first())
            .and_then(|f| f.horizontal_line_metrics(px))
            .map(|lm| baseline_from_metrics(cell_h, &lm))
            .unwrap_or(0)
    }

    /// 缩放因子变化：更新 scale 并整体清空字形位图缓存（位图尺寸随 scale 变；
    /// 码点→字体归属缓存与 scale 无关，不清）。
    pub fn set_scale_factor(&mut self, scale: f64) {
        if (scale - self.scale).abs() > f64::EPSILON {
            self.scale = scale;
            self.glyph_cache.clear();
            // 格子尺寸随缩放变化：整屏重绘。
            self.force_full = true;
        }
    }

    /// 设置选区矩形（网格坐标，含端点）；None 清除高亮。
    pub fn set_selection(&mut self, region: Option<(u16, u16, u16, u16)>) {
        self.selection = region;
    }

    /// 本帧需要提交的区域 (x, y, w, h)：整屏帧为一个全屏矩形，
    /// 增量帧为脏行矩形（以及滚动条变化时的右缘条带）。
    pub fn damage(&self) -> &[(u32, u32, u32, u32)] {
        &self.damage
    }

    /// 设置滚动条拇指几何 (thumb_y, thumb_h)；None 清除（无 scrollback）。
    pub fn set_scrollbar(&mut self, geom: Option<(u32, u32)>) {
        self.scrollbar = geom;
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

    /// 渲染器内部缓存规模（阶段 4K t2 观测：字形缓存条目数 + 帧缓冲字节数）。
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.glyph_cache.len(), self.framebuffer.len() * 4)
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

    /// 码点在组内回退链中的字体索引（主字体 → fallback → 系统 Monospace/Sans）；
    /// None=链中暂无字体含该码点（后台字体未到位或全链缺失）。
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

    /// 按索引光栅；None（链中无码点）时用组首字体光栅 U+25A1 占位方块（不 panic）。
    fn rasterize_glyph(
        &mut self,
        group: u8,
        idx: Option<usize>,
        ch: char,
        px: f32,
    ) -> (fontdue::Metrics, Vec<u8>) {
        match idx {
            Some(i) => self.rasterize_at(group, i, ch, px),
            None => self.rasterize_at(group, 0, '\u{25A1}', px),
        }
    }

    /// 后台字体到位：并入各组并清空码点归属与字形缓存（归属与位图可能变化）。
    /// 四组字体链的共享句柄（供 GPU 渲染器复用，避免重复加载；阶段 4E t3）。
    pub fn font_groups(&self) -> std::sync::Arc<Vec<Vec<Font>>> {
        self.groups.clone()
    }

    pub fn add_background_fonts(&mut self, fonts: Vec<Font>) {
        for g in std::sync::Arc::make_mut(&mut self.groups) {
            g.extend(fonts.iter().cloned());
        }
        self.char_font.clear();
        self.glyph_cache.clear();
        // 字形集变化：整屏重绘。
        self.force_full = true;
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

/// 行绘制上下文（全量与增量路径共用；避免重复计算度量与颜色）。
struct RowDrawCtx {
    cell_w: u32,
    cell_h: u32,
    px: f32,
    bg_default: u32,
    gamma: f32,
    sub: Subpixel,
    /// 基线相对格顶的偏移（像素）。字形落点 = cell_y + baseline + gy + ymin。
    ///
    /// 阶段 4B-fix1 任务 A：此前公式直接用 `cell_y + gy + ymin`（把 baseline 当成
    /// 格顶），导致整块字形上移 ascent 像素、'.' 等小字形落在行上部、g/p/y 不下行。
    baseline: i32,
}

/// 光标状态键（位置/可见性/样式），用于叠加层脏判定。
fn cursor_key(frame: &FrameSnapshot) -> (u16, u16, bool, u8) {
    let style = match frame.cursor.style {
        CursorStyle::Block => 0u8,
        CursorStyle::HollowBlock => 1,
        CursorStyle::Underline => 2,
        CursorStyle::Bar => 3,
    };
    (frame.cursor.x, frame.cursor.y, frame.cursor.visible, style)
}

/// 由字体度量与格高算基线（阶段 4B-fix2，CPU/GPU 共用）。
///
/// `baseline = ceil(ascent) + floor(line_gap/2)`：把 line_gap **居中**分配（此前
/// 全放在格底，Consolas 的 line_gap=2.22 使 baseline 比 ceil(ascent) 小 1，高字形
/// 顶边落到格顶之上被裁）。若这样会让下行放不下（`cell_h - baseline < ceil(-descent)`），
/// 按"先保下行、再保上顶"的顺序把 baseline 压到 `cell_h - ceil(-descent)`；
/// 顶部不足由 `glyph_shift()` 做逐字形平移兜底。
pub fn baseline_from_metrics(cell_h: u32, lm: &fontdue::LineMetrics) -> i32 {
    let ascent_px = lm.ascent.ceil().max(0.0);
    let gap_half = (lm.line_gap / 2.0).floor().max(0.0);
    let descent_px = (-lm.descent).ceil().max(0.0);
    let mut baseline = ascent_px + gap_half;
    if cell_h as f32 - baseline < descent_px {
        baseline = cell_h as f32 - descent_px;
        if baseline < 0.0 {
            baseline = 0.0;
        }
    }
    baseline.clamp(0.0, cell_h as f32) as i32
}

/// 字形整体的垂直平移（像素）：让字形完整落入格内，不缩放。
///
/// 用于 CJK 回退字体（ascent 常大于主字体的格内空间）：其字形高度通常 ≤ cell_h，
/// 通过整体平移即可不裁剪；仅当字形高度 > cell_h 时无法两全，此时**保上顶**、
/// 允许底部溢出被裁（DECISIONS 记录该取舍）。
pub fn glyph_shift(baseline: i32, ymin: i32, height: i32, cell_h: u32) -> i32 {
    if height <= 0 {
        return 0;
    }
    let top = baseline - ymin - height;
    let mut shift = 0;
    if top < 0 {
        shift = -top; // 顶部越界 → 整体下移
    }
    if height <= cell_h as i32 {
        let bottom = top + height + shift;
        let overflow = bottom - cell_h as i32;
        if overflow > 0 {
            shift -= overflow; // 底部越界 → 回移（height ≤ cell_h 时必然可行）
        }
    }
    shift
}

/// 预编辑叠加层键（起始行, 起始列, 单元数, 文本哈希）；无预编辑为 None。
///
/// 必须包含文本哈希：组合串在长度/位置相同时内容仍可能变化（如"中"→"a"），
/// 仅比较布局会漏绘叠加层。
fn preedit_key(
    overlay: &Option<PreeditOverlay>,
    frame: &FrameSnapshot,
) -> Option<(u16, u16, u16, usize, u64)> {
    let o = overlay.as_ref()?;
    let layout =
        preedit::layout_preedit(&o.text, frame.cursor.x, frame.cursor.y, frame.cols, frame.rows)?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a 偏移基数
    for b in o.text.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // 叠加层可能跨行：按"起始列 + 总宽度"换算跨越行数，供标脏时整段重绘
    // （只标起始行会在旧布局多行时留下残影，阶段 4B-fix1 任务 C）。
    let cols = u32::from(frame.cols).max(1);
    let width: u32 = layout.cells.iter().map(|c| u32::from(c.width)).sum();
    let spanned = ((u32::from(layout.col) + width) / cols + 1).min(u32::from(frame.rows)) as u16;
    Some((layout.row, spanned, layout.col, layout.cells.len(), hash))
}

impl CpuRenderer {
    /// 是否需要整屏重绘（结构变化：resize/缩放/字体集变化/调用方要求）。
    fn should_full_redraw(
        &self,
        frame: &FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> bool {
        frame.full_redraw
            || self.force_full
            || self.last_shape != Some((pixel_width, pixel_height))
            || self.fb_width != pixel_width
            || self.fb_height != pixel_height
            || self.framebuffer.len() != (pixel_width * pixel_height) as usize
            || self.last_scale_bits != self.scale.to_bits()
    }

    /// 整屏重绘：重建帧缓冲 + 全部行 + 光标 + 采集条带备份 + 滚动条 + 预编辑。
    /// 顺序与增量路径严格一致（cells → cursor → [备份] → scrollbar → preedit）。
    fn render_full(
        &mut self,
        frame: &FrameSnapshot,
        ctx: &RowDrawCtx,
        pixel_width: u32,
        pixel_height: u32,
    ) {
        self.fb_width = pixel_width;
        self.fb_height = pixel_height;
        // 整屏先铺背景色：网格像素尺寸通常不能整除窗口尺寸（cols*cell_w ≤ pw），
        // 右侧/底部余数区必须也是背景色，否则窗口大于字符网格时那里会显示
        // 未初始化像素（0x00000000）或旧内容（阶段 4B-fix1 任务 B）。
        self.framebuffer = vec![ctx.bg_default; (pixel_width * pixel_height) as usize];
        for y in 0..frame.cells.len() {
            self.draw_row(frame, y, ctx);
        }
        self.draw_cursor(frame, ctx.cell_w, ctx.cell_h, ctx.px);
        self.capture_band();
        self.draw_scrollbar(pixel_height);
        self.draw_preedit(frame, ctx.cell_w, ctx.cell_h, ctx.px);
        self.force_full = false;
        self.last_shape = Some((pixel_width, pixel_height));
        self.last_scale_bits = self.scale.to_bits();
        self.last_cursor = Some(cursor_key(frame));
        self.last_scrollbar = self.scrollbar;
        self.last_preedit = preedit_key(&self.preedit, frame);
        self.damage.push((0, 0, pixel_width.max(1), pixel_height.max(1)));
    }

    /// 增量重绘：只画脏行；光标/滚动条/预编辑作为叠加层按变化补齐。
    ///
    /// 叠加层规则：
    /// - 光标：位置/可见性/样式变化时，旧行与新行标脏；任一行被重绘后重画光标。
    /// - 预编辑：布局变化时旧布局行与新布局行标脏（清残影），并在最后重画。
    /// - 滚动条：几何变化时只重绘右缘条带（先恢复"不含滚动条"的备份，再画新条带）。
    /// - 视口滚动/结构变化由 frame.full_redraw / should_full_redraw 走整屏路径。
    fn render_incremental(
        &mut self,
        frame: &FrameSnapshot,
        ctx: &RowDrawCtx,
        _pixel_width: u32,
        pixel_height: u32,
    ) {
        let rows = frame.cells.len();
        // 脏行集合（阶段 3A 任务 3 的行级增量；阶段 3C 任务 6 移除了滚动位移复用，
        // 原因见 DECISIONS：修复后正确但性能收益为负）。
        let mut dirty = vec![false; rows];
        for (y, item) in dirty.iter_mut().enumerate() {
            *item = frame.is_row_dirty(y);
        }
        // 光标叠加层脏
        let cur_cursor = cursor_key(frame);
        let cursor_changed = self.last_cursor != Some(cur_cursor);
        if cursor_changed {
            // 光标变化：旧位置行（清除残影）与新位置行都标脏。
            if let Some(old) = self.last_cursor
                && let Some(d) = dirty.get_mut(old.1 as usize)
            {
                *d = true;
            }
            if let Some(d) = dirty.get_mut(cur_cursor.1 as usize) {
                *d = true;
            }
        }
        // 预编辑叠加层脏（旧布局行清残影 + 新布局行重绘）
        let new_preedit = preedit_key(&self.preedit, frame);
        let preedit_changed = self.last_preedit != new_preedit;
        if preedit_changed {
            let mut mark = |key: Option<(u16, u16, u16, usize, u64)>| {
                if let Some((row, spanned, _, _, _)) = key {
                    for r in row..row.saturating_add(spanned) {
                        if let Some(d) = dirty.get_mut(r as usize) {
                            *d = true;
                        }
                    }
                }
            };
            mark(self.last_preedit);
            mark(new_preedit);
        }
        let mut rows_drawn = false;
        for (y, is_dirty) in dirty.iter().enumerate() {
            if *is_dirty {
                self.draw_row(frame, y, ctx);
                // 该行的条带段刚被内容覆盖（无滚动条）→ 刷新备份。
                self.capture_band_char_row(y, ctx.cell_h);
                let py = (y as u32).saturating_mul(ctx.cell_h);
                self.damage
                    .push((0, py, self.fb_width.max(1), ctx.cell_h.min(self.fb_height.saturating_sub(py)).max(1)));
                rows_drawn = true;
            }
        }
        // 行被重绘会抹掉其上的光标像素：有行重绘或光标变化时重画光标（幂等）。
        if rows_drawn || cursor_changed {
            self.draw_cursor(frame, ctx.cell_w, ctx.cell_h, ctx.px);
            // 光标格若落在右缘条带内（最后一列），其像素也属于"滚动条之下"。
            if frame.cursor.visible && frame.cursor.x + 1 >= frame.cols {
                self.capture_band_char_row(frame.cursor.y as usize, ctx.cell_h);
            }
        }
        let scrollbar_changed = self.last_scrollbar != self.scrollbar;
        if rows_drawn || cursor_changed || scrollbar_changed || preedit_changed {
            if !(rows_drawn || cursor_changed) {
                // 仅叠加层变化：先恢复条带下的内容，再重画滚动条与预编辑。
                self.restore_band();
            }
            self.draw_scrollbar(pixel_height);
            self.draw_preedit(frame, ctx.cell_w, ctx.cell_h, ctx.px);
            // 滚动条条带 / 预编辑所在行需要提交（条带 4px 宽，单独成矩形）。
            if scrollbar_changed || preedit_changed {
                let w = self.fb_width.max(1);
                self.damage
                    .push((w.saturating_sub(4), 0, w.min(4), self.fb_height.max(1)));
            }
        }
        self.last_cursor = Some(cur_cursor);
        self.last_scrollbar = self.scrollbar;
        self.last_preedit = new_preedit;
    }

    /// 采集滚动条条带（右缘 4px 全高）的像素备份（整屏重绘后调用）。
    ///
    /// 备份语义固定为"不含滚动条的条带内容"：整屏路径在此刻尚未绘制滚动条，
    /// 增量路径则在**每次行重绘后按行刷新**（见 capture_band_row），避免把已画的
    /// 滚动条像素带进备份。
    fn capture_band(&mut self) {
        let w = self.fb_width;
        let h = self.fb_height;
        if w < SCROLLBAR_WIDTH || h == 0 {
            self.band_backup = None;
            return;
        }
        let x0 = (w - SCROLLBAR_WIDTH) as usize;
        let mut buf = Vec::with_capacity(SCROLLBAR_WIDTH as usize * h as usize);
        for y in 0..h as usize {
            let base = y * w as usize;
            buf.extend_from_slice(&self.framebuffer[base + x0..base + w as usize]);
        }
        self.band_backup = Some(buf);
    }

    /// 按**字符行**刷新条带备份：该字符行覆盖的所有像素行的右缘 4px。
    ///
    /// 备份按像素行存储（每像素行 4 个 u32），因此这里把字符行转换为
    /// 像素行范围后逐行复制（行刚被重绘或光标刚重画时调用）。
    fn capture_band_char_row(&mut self, char_y: usize, cell_h: u32) {
        let w = self.fb_width;
        let h = self.fb_height;
        if w < SCROLLBAR_WIDTH || h == 0 || cell_h == 0 {
            return;
        }
        let need = (SCROLLBAR_WIDTH * h) as usize;
        if self.band_backup.as_ref().map(|b| b.len()) != Some(need) {
            // 尺寸变化后首次：整条带回落到当前帧缓冲（此时无滚动条覆盖）。
            self.capture_band();
            return;
        }
        let py0 = (char_y as u32).saturating_mul(cell_h).min(h);
        let py1 = ((char_y as u32 + 1).saturating_mul(cell_h)).min(h);
        let x0 = (w - SCROLLBAR_WIDTH) as usize;
        let wu = w as usize;
        if let Some(buf) = self.band_backup.as_mut() {
            let sw = SCROLLBAR_WIDTH as usize;
            for py in py0..py1 {
                let base = py as usize * wu;
                let dst = py as usize * sw;
                buf[dst..dst + sw].copy_from_slice(&self.framebuffer[base + x0..base + wu]);
            }
        }
    }

    /// 把条带恢复为采集时的内容（去掉当前滚动条像素）。
    fn restore_band(&mut self) {
        let Some(buf) = self.band_backup.clone() else { return; };
        let w = self.fb_width;
        let h = self.fb_height;
        if w < SCROLLBAR_WIDTH || h == 0 || buf.len() != (SCROLLBAR_WIDTH * h) as usize {
            return;
        }
        let x0 = (w - SCROLLBAR_WIDTH) as usize;
        let mut i = 0usize;
        for y in 0..h as usize {
            let base = y * w as usize;
            for x in x0..w as usize {
                self.framebuffer[base + x] = buf[i];
                i += 1;
            }
        }
    }

    /// 绘制单行（行内整行重绘）。全量与增量路径共用，保证像素一致。
    fn draw_row(&mut self, frame: &FrameSnapshot, y: usize, ctx: &RowDrawCtx) {
        let row = &frame.cells[y];
        let pixel_width = self.fb_width;
        let cell_w = ctx.cell_w;
        let cell_h = ctx.cell_h;
        let px = ctx.px;
        let bg_default = ctx.bg_default;
        let gamma = ctx.gamma;
        let sub = ctx.sub;
        let baseline = ctx.baseline;
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
            // 选区高亮（反色）：背景取字色、宽字符续格与 Spacer 尾格不高亮。
            // 颜色决策与 GPU 共用 `decide()`（阶段 4C t4：唯一来源，不再各自分叉）
            let d = crate::decide::decide(
                cell,
                &crate::decide::DecideCtx {
                    x,
                    y,
                    row_len: row.len(),
                    bg_default,
                    fg_default: rgb_to_u32(self.palette.primary_fg),
                    selection: self.selection,
                    palette: &self.palette,
                    draw_bold_with_bright: self.options.draw_bold_with_bright,
                },
            );
            let bg = d.bg;
            let _ = (is_tail, span);
            // 宽字符续格（SpacerTail/SpacerHead）不重复填充背景：起始格的区域已按
            // area_w = 2 格铺满，若这里再按自己的单格填一遍，会把宽字形已经画到
            // 续格上的右半部分擦成背景色（阶段 4B t1 实测：'回' 位图非零 alpha
            // 列 1..14，但渲染墨迹只到列 9）。
            if !is_tail {
                fill_rect(&mut self.framebuffer, pixel_width, px0, py0, area_w, cell_h, bg);
            }
            if is_tail || cell.chars.is_empty() || cell.invisible {
                continue;
            }

            let fg = d.fg;

            let ch = cell.chars.chars().next().expect("非空 chars");
            let group = group_for(cell);
            let font_idx = self.font_index_for(group, ch);

            if span == 2 && sub == Subpixel::None {
                // 宽字符：缩放到 2 格宽高内（不放大），水平居中，基线随位图 ymin 对齐。
                let (m, bitmap) = self.rasterize_glyph(group, font_idx, ch, px);
                let bw = m.width.max(1) as f32;
                let bh = m.height.max(1) as f32;
                let s = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
                if (s - 1.0).abs() < f32::EPSILON {
                    blit(
                        &mut self.framebuffer,
                        pixel_width,
                        (px0, py0, area_w, cell_h, baseline),
                        &m,
                        &bitmap,
                        (fg, d.blend_bg),
                        (self.options.antialiasing, gamma),
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
                        (px0, py0, area_w, cell_h, baseline),
                        &sm,
                        &scaled,
                        (fg, d.blend_bg),
                        (self.options.antialiasing, gamma),
                    );
                }
            } else if span == 2 {
                // 宽字符 + 亚像素：3x 水平采样光栅（px*3），缩放到 2 格内。
                let (m, bitmap) = self.rasterize_glyph(group, font_idx, ch, px * 3.0);
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
                    (px0, py0, area_w, cell_h, baseline),
                    &sm,
                    &scaled,
                    fg,
                    (sub, gamma),
                );
            } else {
                let (m, bitmap) = self.rasterize_glyph(group, font_idx, ch, px);
                blit(
                    &mut self.framebuffer,
                    pixel_width,
                    (px0, py0, area_w, cell_h, baseline),
                    &m,
                    &bitmap,
                    (fg, d.blend_bg),
                    (self.options.antialiasing, gamma),
                );
            }

            // 样式线：下划线（Single/Double/Dotted/Dashed；Curly 近似 Single）、删除线、上划线。
            let line_color = d.line;
            draw_style_lines(
                &mut self.framebuffer,
                pixel_width,
                (px0, py0, area_w, cell_h),
                cell,
                line_color,
            );
        }
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
        let cell_h = (pixel_height / rows).max(1);
        let ctx = RowDrawCtx {
            cell_w: (pixel_width / cols).max(1),
            cell_h,
            px,
            bg_default: rgb_to_u32(self.palette.primary_bg),
            gamma: self.options.text_gamma,
            sub: self.options.subpixel,
            // 基线 = ascent（向上取整），限制在格内；与 cell_metrics 的
            // cell_h = ceil(ascent - descent + line_gap) 配套。
            baseline: self.baseline_px(px, cell_h),
        };
        self.damage.clear();
        if self.should_full_redraw(frame, pixel_width, pixel_height) {
            self.render_full(frame, &ctx, pixel_width, pixel_height);
        } else {
            self.render_incremental(frame, &ctx, pixel_width, pixel_height);
        }
        Ok(())
    }

    fn set_preedit(&mut self, overlay: Option<PreeditOverlay>) {
        self.preedit = overlay;
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
        let baseline = self.baseline_px(px, cell_h);
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
                    let (m, bitmap) = self.rasterize_glyph(group, font_idx, ch, px);
                    blit(
                        &mut self.framebuffer,
                        width,
                        (cx, cy, cell_w, cell_h, baseline),
                        &m,
                        &bitmap,
                        (rgb_to_u32(self.palette.cursor_text), color),
                        (self.options.antialiasing, self.options.text_gamma),
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

    /// 滚动条：右缘 4px 轨道（前景/背景通道平均，半透明近似）+拇指（前景色）。
    /// 纯绘制无状态；几何由 set_scrollbar 提供。
    fn draw_scrollbar(&mut self, pixel_height: u32) {
        let Some((thumb_y, thumb_h)) = self.scrollbar else { return; };
        let width = self.fb_width;
        if width < SCROLLBAR_WIDTH || pixel_height == 0 || thumb_h == 0 {
            return;
        }
        let track_x = width - SCROLLBAR_WIDTH;
        let fg = rgb_to_u32(self.palette.primary_fg);
        let bg = rgb_to_u32(self.palette.primary_bg);
        // 轨道色=前景/背景逐通道平均（softbuffer 无 alpha 混合通道，平均近似半透明）。
        let avg = |a: u32, b: u32| (a + b) / 2;
        let track = 0xFF00_0000
            | (avg(fg >> 16 & 0xFF, bg >> 16 & 0xFF) << 16)
            | (avg(fg >> 8 & 0xFF, bg >> 8 & 0xFF) << 8)
            | avg(fg & 0xFF, bg & 0xFF);
        fill_rect(&mut self.framebuffer, width, track_x, 0, SCROLLBAR_WIDTH, pixel_height, track);
        let thumb_h = thumb_h.min(pixel_height);
        let thumb_y = thumb_y.min(pixel_height.saturating_sub(thumb_h));
        fill_rect(&mut self.framebuffer, width, track_x, thumb_y, SCROLLBAR_WIDTH, thumb_h, fg);
    }

    /// IME 预编辑叠加绘制（任务 2C.3）：在终端光标处以叠加层绘制文本
    /// （带下划线，宽字符占 2 格），不写入网格；组合光标以反色标示。
    /// 超出右边界整体换行、贴底平移、超整行截断（布局规则见 preedit::layout_preedit）。
    fn draw_preedit(&mut self, frame: &FrameSnapshot, cell_w: u32, cell_h: u32, px: f32) {
        let Some(overlay) = self.preedit.clone() else { return; };
        let cols = u32::from(frame.cols).max(1);
        let rows = u32::from(frame.rows).max(1);
        let Some(layout) = preedit::layout_preedit(
            &overlay.text,
            frame.cursor.x,
            frame.cursor.y,
            cols as u16,
            rows as u16,
        ) else {
            return;
        };
        // 组合光标所在单元下标（cursor_range 起点所在的字符序号）。
        let cursor_cell_idx = overlay.cursor_range.map(|(begin, _)| {
            overlay
                .text
                .char_indices()
                .position(|(i, _)| i >= begin)
                .unwrap_or(0)
        });
        let bg = rgb_to_u32(self.palette.primary_bg);
        let fg = rgb_to_u32(self.palette.primary_fg);
        let mut x_cells = u32::from(layout.col);
        let y = u32::from(layout.row) * cell_h;
        let gamma = self.options.text_gamma;
        let baseline = self.baseline_px(px, cell_h);
        for (idx, pc) in layout.cells.iter().enumerate() {
            let px0 = x_cells * cell_w;
            let span = u32::from(pc.width).max(1);
            let area_w = cell_w * span;
            if px0 + area_w > self.fb_width {
                break;
            }
            let is_cursor = cursor_cell_idx == Some(idx);
            // 叠加层底色覆盖（遮住原格内容），组合光标格反色。
            let (cell_bg, cell_fg) = if is_cursor { (fg, bg) } else { (bg, fg) };
            fill_rect(&mut self.framebuffer, self.fb_width, px0, y, area_w, cell_h, cell_bg);
            // 字形：宽字符按 2 格内缩放绘制（与终端宽字符规则一致）。
            let group = GROUP_NORMAL as u8;
            let font_idx = self.font_index_for(group, pc.ch);
            let (m, bitmap) = self.rasterize_glyph(group, font_idx, pc.ch, px);
            let bw = m.width.max(1) as f32;
            let bh = m.height.max(1) as f32;
            let s = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
            if (s - 1.0).abs() < f32::EPSILON {
                blit(
                    &mut self.framebuffer,
                    self.fb_width,
                    (px0, y, area_w, cell_h, baseline),
                    &m,
                    &bitmap,
                    (cell_fg, cell_bg),
                    (self.options.antialiasing, gamma),
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
                    self.fb_width,
                    (px0, y, area_w, cell_h, baseline),
                    &sm,
                    &scaled,
                    (cell_fg, cell_bg),
                    (self.options.antialiasing, gamma),
                );
            }
            // 下划线：格底部 2px（组合态统一单线）。
            let uy = y + cell_h.saturating_sub(2);
            fill_rect(&mut self.framebuffer, self.fb_width, px0, uy, area_w, 2, cell_fg);
            x_cells += span;
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
    rect: (u32, u32, u32, u32),
    cell: &Cell,
    color: u32,
) {
    let (x0, y0, w, h) = rect;
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
    cell: (u32, u32, u32, u32, i32),
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    colors: (u32, u32),
    aa_gamma: (Antialiasing, f32),
) {
    let (fg, bg) = colors;
    let (antialiasing, gamma) = aa_gamma;
    let (cell_x, cell_y, cell_w, cell_h, baseline) = cell;
    let bw = metrics.width as i32;
    let bh = metrics.height as i32;
    let shift = glyph_shift(baseline, metrics.ymin, bh, cell_h);
    for gy in 0..bh {
        for gx in 0..bw {
            let alpha = bitmap[(gy as usize) * metrics.width + gx as usize];
            if alpha == 0 {
                continue;
            }
            let px = cell_x as i32 + gx + metrics.xmin;
            // 字形顶边 = 基线 - ymin - height（fontdue 的 ymin 是位图**底边**相对
            // 基线的偏移，向上为负），再叠加逐字形平移（CJK 回退字体兜底）。
            let py = cell_y as i32 + baseline - metrics.ymin - bh + gy + shift;
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
    cell: (u32, u32, u32, u32, i32),
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    fg: u32,
    sp: (Subpixel, f32),
) {
    let (order, gamma) = sp;
    let (cell_x, cell_y, cell_w, cell_h, baseline) = cell;
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
            // 同灰度路径：顶边 = 基线 - ymin - height（3x 采样下全部除以 3），
            // 平移量按缩放后的实际高度计算。
            let shift = glyph_shift(baseline, metrics.ymin / 3, bh3 / 3, cell_h);
            let py = cell_y as i32 + baseline - metrics.ymin / 3 - bh3 / 3 + gy3 / 3 + shift;
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

/// 带缓存的 face 加载（阶段 3A 任务 2）：同一 fontdb face（同一文件/同一 style
/// 回落目标）只读取+解析一次，后续复用已解析 Font。
///
/// 缓存键为 fontdb::ID（同一文件在多组字体间共享，例如 bold 缺失回落到 Regular
/// 时与 normal 指向同一 face）。返回 (Font, 是否命中缓存)。
fn load_font_styled_cached(
    db: &Database,
    family: &str,
    style: &str,
    cache: &mut std::collections::HashMap<fontdb::ID, Font>,
) -> Result<(Font, bool), Error> {
    let (w, s) = style_query(style);
    let query = Query {
        families: &[Family::Name(family), Family::Monospace],
        weight: w,
        style: s,
        ..Default::default()
    };
    // 阶段 4H t2：查找入口（此前从上一段到这里约有 20ms 无标注，实测为 fontdb face 查询）。
    eprintln!(
        "[perf] font_lookup_begin_ms family='{}' style='{}' since_start={:.1}",
        family,
        style,
        since_start_ms()
    );
    let t_query = std::time::Instant::now();
    let id = db
        .query(&query)
        .ok_or_else(|| Error::Font(format!("family '{}' style '{}' not found", family, style)))?;
    let query_us = t_query.elapsed().as_micros();
    if let Some(f) = cache.get(&id) {
        return Ok((f.clone(), true));
    }
    // ---- 阶段 4M t3：语句级微秒打点（只补观测，不改行为）----
    // 把"face 构造 / 元数据 / 读取 / fontdue 参数 / fontdue 调用 / clone / 缓存写入"
    // 逐段量到微秒，并记录来源类型、字节数与是否为 .ttc 集合。
    // 只统计**缓存未命中**（真正解析）的路径；每个 face 只打印一行，避免打印本身
    // 在慢控制台上制造可观测的间隙（Windows 上每行 eprintln 可达数十毫秒，见 3J 数据）。
    let t_face = std::time::Instant::now();
    let face = db
        .face(id)
        .ok_or_else(|| Error::Font("face not found".to_string()))?;
    let face_us = t_face.elapsed().as_micros();

    let (source_kind, path_opt) = match &face.source {
        Source::File(path) => ("file", Some(path.clone())),
        Source::Binary(_) => ("binary", None),
        Source::SharedFile(path, _) => ("shared", Some(path.clone())),
    };
    // 元数据（stat）：区分"文件系统/安全软件扫描"与"解析"——stat 慢通常意味着
    // 打开/属性查询被拦（AV 实时扫描的典型特征），而 read 慢更像顺序读盘/缓存未命中。
    let t_meta = std::time::Instant::now();
    let meta_len = path_opt
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as usize);
    let meta_us = t_meta.elapsed().as_micros();

    let t_read = std::time::Instant::now();
    let bytes: Vec<u8> = match &face.source {
        Source::File(path) => std::fs::read(path)
            .map_err(|e| Error::Font(format!("read font file: {}", e)))?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };
    let read_us = t_read.elapsed().as_micros();

    // .ttc / OpenType 集合：fontdue 需要遍历集合内的 face，Windows 上解析更慢。
    let is_collection = path_opt
        .as_ref()
        .map(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttc")))
        .unwrap_or(false)
        || bytes.starts_with(b"ttcf");

    let t_settings = std::time::Instant::now();
    let fd_settings = fontdue::FontSettings::default();
    let settings_us = t_settings.elapsed().as_micros();

    let t_parse = std::time::Instant::now();
    let font = Font::from_bytes(bytes.as_slice(), fd_settings)
        .map_err(|e| Error::Font(format!("fontdue parse: {}", e)))?;
    let parse_us = t_parse.elapsed().as_micros();

    let t_clone = std::time::Instant::now();
    let cached = font.clone();
    let clone_us = t_clone.elapsed().as_micros();

    let t_insert = std::time::Instant::now();
    cache.insert(id, cached);
    let insert_us = t_insert.elapsed().as_micros();

    let steps_us = query_us + face_us + meta_us + read_us + settings_us + parse_us + clone_us + insert_us;
    eprintln!(
        "[perf] font_steps_us family='{}' style='{}' kind={} bytes={} meta_len={} ttc={} query_us={} face_us={} meta_us={} read_us={} settings_us={} parse_us={} clone_us={} insert_us={} steps_us={} since_start={:.1}",
        family,
        style,
        source_kind,
        bytes.len(),
        meta_len.map(|v| v as i64).unwrap_or(-1),
        is_collection,
        query_us,
        face_us,
        meta_us,
        read_us,
        settings_us,
        parse_us,
        clone_us,
        insert_us,
        steps_us,
        since_start_ms()
    );
    Ok((font, false))
}

/// 带缓存的 family 加载（style 缺失回落顺序同 load_font_with_fallback）。
fn load_font_with_fallback_cached(
    db: &Database,
    family: &str,
    style: &str,
    cache: &mut std::collections::HashMap<fontdb::ID, Font>,
) -> Result<(Font, bool, String), Error> {
    let mut candidates = vec![
        style.to_string(),
        "Bold".to_string(),
        "Italic".to_string(),
        "Regular".to_string(),
    ];
    candidates.dedup();
    let mut last_err = None;
    for s in &candidates {
        match load_font_styled_cached(db, family, s, cache) {
            Ok((f, cached)) => {
                if s != style {
                    eprintln!("font style fallback: '{}' '{}' → '{}'", family, style, s);
                }
                return Ok((f, cached, s.clone()));
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Font(format!("family '{}' not found", family))))
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
            (0, 0, 8, 4, 2),
            &m,
            &bitmap,
            (FG, BG),
            (Antialiasing::Grayscale, 1.0),
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
            (0, 0, 8, 4, 2),
            &m,
            &bitmap,
            (FG, BG),
            (Antialiasing::Grayscale, 1.0),
        );
        let invaded = fb[4 * 8..].iter().filter(|&&p| p != BG).count();
        assert_eq!(invaded, 0, "下邻格应全为背景色");
    }
}
