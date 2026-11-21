//! 字体集合（阶段 0.2.8 / 3a）：crossfont 后端。
//!
//! 之前用 fontdb 做字体发现、fontdue 做光栅化：fontdue 是**整文件解析**（CJK 面 ≈0.34 s），
//! 只能把回退字体放到后台线程加载，于是首帧先画占位方块。crossfont 与 alacritty 同构：
//! Linux 走 fontconfig + FreeType、Windows 走 DirectWrite、macOS 走 CoreText，都是
//! **惰性**打开（mmap）→ 启动时同步加载全部字体只需毫秒级，首帧就有真实字形。
//!
//! 本模块把 crossfont 的 `Rasterizer`/`FontKey` 包成一个可跨线程共享的 `FontSet`
//! （内部 `Mutex`，仅渲染线程访问），并对外暴露与旧实现等价的语义：
//! - `groups`：四个样式组，每组 = [主字体, fallback..., 内置兜底...]；
//! - 逐字符回退：先查缓存，再按组内顺序找"该 face 含此码点"的第一个；
//! - 字形位图：8 位覆盖率 + 与 fontdue `Metrics` 兼容的度量（xmin/ymin/width/height/advance）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crossfont::{BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Style};

use crate::{Error, FontSettings, FontSpec, px_from_pt};

/// 字形度量：字段语义与 fontdue::Metrics 对齐（ymin 自基线向上为正）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlyphMetrics {
    /// 水平步进（像素）。
    pub advance_width: f32,
    /// 位图左边界相对原点的偏移。
    pub xmin: i32,
    /// 位图下边界相对基线的偏移（向上为正，通常为负）。
    pub ymin: i32,
    /// 位图宽高（像素）。
    pub width: usize,
    pub height: usize,
}

/// face 级行度量。
#[derive(Clone, Copy, Debug, Default)]
pub struct LineMetrics {
    /// 基线距格顶（像素，向上取整）。
    pub ascent: f32,
    /// 基线以下高度（负值或正值，取自后端；用 `descent_abs` 归一）。
    pub descent: f32,
    /// 行高（ascent + |descent| + line_gap）。
    pub line_height: f32,
}

/// 一个渲染器共享的字体集合（CPU/GPU 渲染器复用同一份，避免重复加载）。
pub struct FontSet {
    rasterizer: Rasterizer,
    /// 四组字体键 [normal, bold, italic, bold_italic]。
    groups: Vec<Vec<FontKey>>,
    /// 码点 → 组内索引缓存（None = 全链缺字）。
    char_font: HashMap<(u8, char), Option<usize>>,
    /// face 度量缓存（与字号无关的只缓存一次；字号变换用 px 现算）。
    line_metrics: HashMap<FontKey, LineMetrics>,
    /// 按加载顺序记录的字体规格：[normal, bold, italic, bold_italic, fallback...]。
    specs: Vec<FontSpec>,
    /// 组内索引 → `specs` 下标（组 0 是 [normal, fallback...]，组 1..3 是 [对应样式, fallback...]）。
    spec_map: Vec<Vec<usize>>,
    /// 覆盖判断用的 fontdb（懒加载：只有出现主字体缺字时才会扫系统字体）。
    db: Option<fontdb::Database>,
    /// 规格下标 → fontdb face id（懒加载，查不到为 None）。
    face_ids: Vec<Option<fontdb::ID>>,
    /// face_ids 是否已解析。
    ids_ready: bool,
}

/// 组序号（与 FontSettings 字段顺序一致）。
pub const GROUP_NORMAL: usize = 0;
pub const GROUP_BOLD: usize = 1;
pub const GROUP_ITALIC: usize = 2;
pub const GROUP_BOLD_ITALIC: usize = 3;

fn style_from_name(name: &str) -> Style {
    // crossfont 的 Style 只接受"具体样式名"或 slant/weight 描述；配置里的
    // "Regular"/"Bold"/"Italic"/"Bold Italic" 正好是族内常见样式名，直接透传即可
    // （fontconfig/DirectWrite 会按名字匹配）。
    Style::Specific(name.to_string())
}

impl FontSet {
    /// 同步加载主字体与全部回退字体（crossfont 打开 face 是毫秒级，无需后台线程）。
    pub fn load(settings: &FontSettings) -> Result<Rc<RefCell<FontSet>>, Error> {
        let t = std::time::Instant::now();
        let mut rasterizer = Rasterizer::new()
            .map_err(|e| Error::Font(format!("crossfont Rasterizer::new: {e}")))?;
        let size = Size::new(settings.size.max(1.0));

        let specs = [
            &settings.normal,
            &settings.bold,
            &settings.italic,
            &settings.bold_italic,
        ];
        let mut groups: Vec<Vec<FontKey>> = Vec::with_capacity(4);
        for (i, spec) in specs.iter().enumerate() {
            let key = load_one(&mut rasterizer, spec, size);
            let key = match key {
                Some(k) => k,
                None if i == GROUP_NORMAL => {
                    return Err(Error::Font(format!(
                        "主字体加载失败：family={:?} style={:?}",
                        spec.family, spec.style
                    )));
                }
                // 其余样式组失败时回落到 normal 组主字体（与旧行为一致）。
                None => groups[GROUP_NORMAL][0],
            };
            groups.push(vec![key]);
        }
        // 回退链：用户配置在前，内置符号兜底在后（去重；缺失的 face 跳过并记一行提示）。
        for spec in &settings.fallback {
            let key = load_one(&mut rasterizer, spec, size);
            match key {
                Some(k) => {
                    for g in groups.iter_mut() {
                        g.push(k);
                    }
                }
                None => eprintln!(
                    "fallback 字体不可用，已跳过：family={:?} style={:?}",
                    spec.family, spec.style
                ),
            }
        }

        let mut specs: Vec<FontSpec> = Vec::new();
        for spec in [
            &settings.normal,
            &settings.bold,
            &settings.italic,
            &settings.bold_italic,
        ] {
            specs.push(spec.clone());
        }
        for spec in &settings.fallback {
            specs.push(spec.clone());
        }
        // 组内索引 → specs 下标（groups 里每组都是 [该样式主字体, fallback...]）。
        let fallback_spec_base = 4usize;
        let spec_map: Vec<Vec<usize>> = (0..groups.len())
            .map(|g| {
                let mut m = vec![g];
                m.extend((0..settings.fallback.len()).map(|i| fallback_spec_base + i));
                m
            })
            .collect();
        let mut set = FontSet {
            rasterizer,
            groups,
            spec_map,
            char_font: HashMap::new(),
            line_metrics: HashMap::new(),
            specs,
            db: None,
            face_ids: Vec::new(),
            ids_ready: false,
        };
        set.line_metrics.insert(
            set.groups[GROUP_NORMAL][0],
            set.read_line_metrics(set.groups[GROUP_NORMAL][0], size),
        );
        crate::eperf!(
            "[perf] fontset_load_ms={:.2} families={} groups={}",
            t.elapsed().as_secs_f64() * 1000.0,
            settings.fallback.len() + 4,
            set.groups.len()
        );
        Ok(Rc::new(RefCell::new(set)))
    }

    fn read_line_metrics(&self, key: FontKey, size: Size) -> LineMetrics {
        match self.rasterizer.metrics(key, size) {
            Ok(m) => LineMetrics {
                ascent: 0.0,
                descent: m.descent,
                line_height: m.line_height as f32,
            },
            Err(_) => LineMetrics::default(),
        }
    }

    /// 组内首个含该码点的 face 索引（逐字符回退缓存）。
    pub fn index_for(&mut self, group: u8, ch: char) -> Option<usize> {
        if let Some(v) = self.char_font.get(&(group, ch)) {
            return *v;
        }
        let mut found = None;
        let count = self.groups[group as usize].len();
        for i in 0..count {
            if self.face_has(group, i, ch) {
                found = Some(i);
                break;
            }
        }
        self.char_font.insert((group, ch), found);
        found
    }

    /// face 是否真的含该码点（阶段 0.2.9）。
    ///
    /// **不能用 crossfont 的 `get_glyph` 判断**：它在字形缺失时会通过 fontconfig 换一个
    /// 含该字的 face 渲染并返回 `Ok`（`crossfont/src/ft/mod.rs::face_for_glyph`），
    /// 于是"第一个字体永远含所有字"，配置里的 `font.fallback` 会被完全绕过，
    /// 逐字符字形改由 fontconfig 决定（不同字符来自不同字重/字号，表现为字形粗细不一）。
    ///
    /// 这里改为查 face 的真实 cmap：fontdb 负责"家族名 → 字体文件"，ttf-parser 负责
    /// 解析 cmap 查 `glyph_index`。fontdb 扫描只在该函数首次真正需要时执行（懒加载）。
    fn face_has(&mut self, group: u8, index: usize, ch: char) -> bool {
        let Some(&spec_index) = self
            .spec_map
            .get(group as usize)
            .and_then(|m| m.get(index))
        else {
            return false;
        };
        if !self.ids_ready {
            let db = self.db.get_or_insert_with(|| {
                let mut db = fontdb::Database::new();
                db.load_system_fonts();
                db
            });
            self.face_ids = self
                .specs
                .iter()
                .map(|spec| resolve_face(db, spec))
                .collect();
            self.ids_ready = true;
            if std::env::var_os("LEANTTY_FONT_COV_DEBUG").is_some() {
                for (i, (spec, id)) in self.specs.iter().zip(self.face_ids.iter()).enumerate() {
                    eprintln!("[cov] {i} family={:?} style={:?} id={:?}", spec.family, spec.style, id);
                }
            }
            crate::eperf!("[perf] font_coverage_db_ready faces={}", self.face_ids.len());
        }
        let Some(id) = self.face_ids.get(spec_index).copied().flatten() else {
            return false;
        };
        let db = self.db.as_ref().expect("db 已在上面初始化");
        let has = db
            .with_face_data(id, |data, face_index| {
                ttf_parser::Face::parse(data, face_index)
                    .ok()
                    .is_some_and(|face| face.glyph_index(ch).is_some())
            })
            .unwrap_or(false);
        if std::env::var_os("LEANTTY_FONT_COV_DEBUG").is_some() {
            let spec = &self.specs[spec_index];
            eprintln!("[cov] ch={ch:?} spec={} family={:?} has={has}", spec_index, spec.family);
        }
        has
    }

    /// 光栅化一个码点；链中无该字形时返回 `None`（由调用方决定画不画占位）。
    pub fn rasterize(
        &mut self,
        group: u8,
        idx: usize,
        ch: char,
        px: f32,
    ) -> Option<(GlyphMetrics, Vec<u8>)> {
        let key = *self.groups[group as usize].get(idx)?;
        let size = Self::size_for_px(px);
        let glyph = self
            .rasterizer
            .get_glyph(GlyphKey {
                font_key: key,
                character: ch,
                size,
            })
            .ok()?;
        let (w, h) = (glyph.width.max(0) as usize, glyph.height.max(0) as usize);
        let mut bitmap = Vec::with_capacity(w * h);
        match &glyph.buffer {
            BitmapBuffer::Rgb(buf) => {
                for px in buf.as_chunks::<3>().0 {
                    bitmap.push(px[0]);
                }
            }
            BitmapBuffer::Rgba(buf) => {
                // 彩色字形暂按 alpha 通道画（单色近似；彩色渲染未实现）。
                for px in buf.as_chunks::<4>().0 {
                    bitmap.push(px[3]);
                }
            }
        }
        bitmap.truncate(w * h);
        Some((
            GlyphMetrics {
                advance_width: glyph.advance.0 as f32,
                xmin: glyph.left,
                ymin: glyph.top - glyph.height,
                width: w,
                height: h,
            },
            bitmap,
        ))
    }

    /// 像素 → crossfont 的 `Size`（其内部以点存储，`as_px()` 乘 96/72）。
    fn size_for_px(px: f32) -> Size {
        Size::new((px * 72.0 / 96.0).max(0.5))
    }

    /// FreeType 的 face 尺寸只有在 `get_glyph` 后才会被 `set_char_size` 更新，
    /// 否则 `metrics()` 返回的是未设置尺寸的陈旧值（实测 line_height=0、advance 恒定）。
    /// 这里先光栅化一个 '0' 强制应用尺寸，再读度量。
    fn ensure_sized(&mut self, key: FontKey, size: Size) {
        let _ = self.rasterizer.get_glyph(GlyphKey {
            font_key: key,
            character: '0',
            size,
        });
    }

    /// 格子尺寸：宽 = 平均 advance（四舍五入），高 = 行高（四舍五入）。
    pub fn cell_metrics(&mut self, size_pt: f32, scale: f64) -> (u32, u32) {
        let px = px_from_pt(size_pt, scale);
        let size = Self::size_for_px(px);
        let normal = self.groups[GROUP_NORMAL][0];
        self.ensure_sized(normal, size);
        let advance = match self.rasterizer.metrics(normal, size) {
            Ok(m) => m.average_advance as f32,
            Err(_) => px * 0.6,
        };
        let line = match self.rasterizer.metrics(normal, size) {
            Ok(m) => m.line_height as f32,
            Err(_) => px * 1.2,
        };
        (
            advance.round().max(1.0) as u32,
            line.round().max(1.0) as u32,
        )
    }

    /// 基线相对格顶的偏移：ascent = 行高 - |descent|。
    pub fn baseline(&mut self, size_pt: f32, scale: f64, cell_h: u32) -> i32 {
        let size = Self::size_for_px(px_from_pt(size_pt, scale));
        let normal = self.groups[GROUP_NORMAL][0];
        self.ensure_sized(normal, size);
        let m = match self.rasterizer.metrics(normal, size) {
            Ok(m) => m,
            Err(_) => return (cell_h as i32) - 2,
        };
        let ascent = (m.line_height as f32 + m.descent).round();
        ascent.clamp(0.0, (cell_h as i32 - 1) as f32) as i32
    }
}

fn load_one(rasterizer: &mut Rasterizer, spec: &FontSpec, size: Size) -> Option<FontKey> {
    let desc = FontDesc::new(spec.family.as_str(), style_from_name(&spec.style));
    rasterizer.load_font(&desc, size).ok()
}

/// 家族名 + 样式名 → fontdb face id。
///
/// 只在**家族名精确匹配**时返回：fontdb 的 `query` 可能模糊匹配到别的家族
/// （例如缺 `Consolas` 时给一个替代品），那样"覆盖判断"就会落到错误的字体上。
fn resolve_face(db: &fontdb::Database, spec: &FontSpec) -> Option<fontdb::ID> {
    let style = spec.style.to_ascii_lowercase();
    let (weight, slant) = if style.contains("bold") {
        (fontdb::Weight::BOLD, style.contains("italic") || style.contains("oblique"))
    } else {
        (fontdb::Weight::NORMAL, style.contains("italic") || style.contains("oblique"))
    };
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(spec.family.as_str())],
        weight,
        style: if slant {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
        ..Default::default()
    };
    let id = db.query(&query)?;
    // 精确家族名校验：避免记录到 fontdb 的替代字体。
    let face = db.face(id)?;
    let exact = face
        .families
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(&spec.family));
    if exact { Some(id) } else { None }
}
