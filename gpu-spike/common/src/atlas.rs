//! fontdue 字形图集（R8）：字体选择与 `render::CpuRenderer` 相同（fontdb 查询
//! `monospace` + Generic Monospace 兜底，style Normal），保证字形位图来源一致。
//!
//! 宽字符按 `render::scale_bitmap` 的最近邻规则预先缩放到"2 格内居中"，
//! 使 GPU 侧只做 1:1 采样即可与 CPU 输出对齐（阶段 4A 的对比口径）。

use std::collections::HashMap;
use std::error::Error;

pub const ATLAS_W: u32 = 1024;
pub const ATLAS_H: u32 = 1024;
/// 与 `CpuRenderer` 相同的字号（render 侧 fonts.size=16.0）。
pub const FONT_SIZE: f32 = 16.0;

/// 图集中的一条字形记录。
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    /// 位图在图集中的 uv 矩形。
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// 位图尺寸（像素）。
    pub w: u32,
    pub h: u32,
    /// 相对格左上角的像素偏移（含 wide 居中偏移）。
    pub xmin: i32,
    pub ymin: i32,
}

/// 图集键：同一字符在 1 格与 2 格宽度下位图不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub ch: char,
    pub span: u8,
    /// 字体组：0=normal, 1=bold, 2=italic, 3=bold-italic（与 render 的 group_for 一致）。
    pub group: u8,
}

pub struct Atlas {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    glyphs: HashMap<Key, Glyph>,
    default_fg: vt::Rgb,
    default_bg: vt::Rgb,
    /// 进入图集时使用的格子尺寸（与 CPU 侧同函数同字体得到，用于自检）。
    pub cell: (u32, u32),
    /// 复刻 `CpuRenderer` 的四组回退链：每组 = [主档 style, CJK 兜底]。
    #[allow(dead_code)]
    pub groups: Vec<Vec<render::Font>>,
}

impl Atlas {
    pub fn build(snap: &vt::FrameSnapshot, scale: f64) -> Result<Self, Box<dyn Error>> {
        let groups = load_font_groups()?;
        let font = groups
            .first()
            .and_then(|g| g.first())
            .ok_or("字体链为空")?;
        let cell = render::cell_metrics(font, FONT_SIZE, scale);
        let px = FONT_SIZE * scale as f32;

        let mut pixels = vec![0u8; (ATLAS_W * ATLAS_H) as usize];
        let mut glyphs: HashMap<Key, Glyph> = HashMap::new();
        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;
        let mut row_h = 0u32;

        for (y, row) in snap.cells.iter().enumerate() {
            for (x, cell_data) in row.iter().enumerate() {
                let Some(ch) = cell_data.chars.chars().next() else {
                    continue;
                };
                if ch == ' ' {
                    continue;
                }
                // wide 判定与 CPU 一致：Wide 且右侧还有格子 → span 2。
                let span: u8 = if cell_data.wide == vt::CellWide::Wide && x + 1 < row.len() {
                    2
                } else {
                    1
                };
                if matches!(
                    cell_data.wide,
                    vt::CellWide::SpacerTail | vt::CellWide::SpacerHead
                ) {
                    continue;
                }
                let group = group_for(cell_data);
                let key = Key { ch, span, group };
                if glyphs.contains_key(&key) {
                    continue;
                }
                let _ = y;
                // 复刻 font_index_for：在该组链中取第一个含该码点的字体。
                let chain = &groups[group as usize];
                let f = chain
                    .iter()
                    .find(|f| f.has_glyph(ch))
                    .unwrap_or(&chain[0]);
                let (rect, bitmap) = rasterize_for(f, ch, px, cell, span);
                let (w, h) = (rect.w, rect.h);
                if w == 0 || h == 0 {
                    glyphs.insert(key, rect);
                    continue;
                }
                // 每个字形四周留 1px 透明保护边：最近邻采样在 quad 边界可能取到
                // 相邻 texel，无保护边会把邻格字形像素混进来（实测 CJK 段出现这种污染）。
                if cursor_x + w + 2 > ATLAS_W {
                    cursor_x = 0;
                    cursor_y += row_h;
                    row_h = 0;
                }
                if cursor_y + h + 2 > ATLAS_H {
                    return Err(format!(
                        "图集溢出（{ATLAS_W}x{ATLAS_H} 放不下字形，已放 {} 个）",
                        glyphs.len()
                    )
                    .into());
                }
                let x0 = cursor_x + 1;
                let y0 = cursor_y + 1;
                for gy in 0..h {
                    let src = (gy * w) as usize;
                    let dst = ((y0 + gy) * ATLAS_W + x0) as usize;
                    pixels[dst..dst + w as usize].copy_from_slice(&bitmap[src..src + w as usize]);
                }
                let g = Glyph {
                    u0: x0 as f32 / ATLAS_W as f32,
                    v0: y0 as f32 / ATLAS_H as f32,
                    u1: (x0 + w) as f32 / ATLAS_W as f32,
                    v1: (y0 + h) as f32 / ATLAS_H as f32,
                    ..rect
                };
                glyphs.insert(key, g);
                cursor_x = x0 + w + 1;
                row_h = row_h.max(h + 2);
            }
        }

        let palette = render::Palette::default();
        Ok(Self {
            pixels,
            width: ATLAS_W,
            height: ATLAS_H,
            glyphs,
            default_fg: palette.primary_fg,
            default_bg: palette.primary_bg,
            cell,
            groups,
        })
    }

    pub fn get(&self, ch: char, span: u8, group: u8) -> Option<&Glyph> {
        self.glyphs.get(&Key { ch, span, group })
    }

    pub fn default_fg(&self) -> vt::Rgb {
        self.default_fg
    }

    pub fn default_bg(&self) -> vt::Rgb {
        self.default_bg
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// 字体组数与每组链长度（诊断用）。
    #[allow(dead_code)]
    pub fn font_shape(&self) -> (usize, usize) {
        (
            self.groups.len(),
            self.groups.first().map(Vec::len).unwrap_or(0),
        )
    }
}

/// 复刻 `render::CpuRenderer` 的字形放置：单格直接按 metrics 偏移；
/// 宽字符按 `scale_bitmap` 最近邻缩放，水平居中、纵向按 ymin 偏移。
fn rasterize_for(
    font: &render::Font,
    ch: char,
    px: f32,
    cell: (u32, u32),
    span: u8,
) -> (Glyph, Vec<u8>) {
    let (metrics, bitmap) = font.rasterize(ch, px);
    let (cell_w, cell_h) = cell;
    if span != 2 {
        return (
            Glyph {
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                w: metrics.width as u32,
                h: metrics.height as u32,
                xmin: metrics.xmin,
                ymin: metrics.ymin,
            },
            bitmap,
        );
    }
    let area_w = cell_w * 2;
    let bw = metrics.width.max(1) as f32;
    let bh = metrics.height.max(1) as f32;
    let s = (area_w as f32 / bw).min(cell_h as f32 / bh).min(1.0);
    if (s - 1.0).abs() < f32::EPSILON {
        return (
            Glyph {
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                w: metrics.width as u32,
                h: metrics.height as u32,
                xmin: metrics.xmin,
                ymin: metrics.ymin,
            },
            bitmap,
        );
    }
    let sw = (bw * s).ceil() as usize;
    let sh = (bh * s).ceil() as usize;
    let scaled = scale_bitmap(&bitmap, metrics.width, metrics.height, sw, sh);
    let ox = (area_w as i32 - sw as i32) / 2;
    (
        Glyph {
            u0: 0.0,
            v0: 0.0,
            u1: 0.0,
            v1: 0.0,
            w: sw as u32,
            h: sh as u32,
            xmin: (metrics.xmin as f32 * s).ceil() as i32 + ox,
            ymin: (metrics.ymin as f32 * s).ceil() as i32,
        },
        scaled,
    )
}

/// 与 `render::scale_bitmap` 完全相同的最近邻缩放（保持对比口径一致）。
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

/// 复刻 `CpuRenderer` 的四组字体：每组 = [主档 style, 系统 Monospace, 系统 SansSerif]
/// （CJK 由后两者提供；不建组则 bold 段与中文字形都会与 CPU 侧不一致）。
fn load_font_groups() -> Result<Vec<Vec<render::Font>>, Box<dyn Error>> {
    use fontdb::{Database, Family, Query, Source, Style as DbStyle, Weight};
    let mut db = Database::new();
    db.load_system_fonts();
    let read_face = |db: &Database, id| -> Result<render::Font, Box<dyn Error>> {
        let face = db.face(id).ok_or("fontdb face 丢失")?;
        let bytes: Vec<u8> = match &face.source {
            Source::File(path) => std::fs::read(path)?,
            Source::Binary(data) => data.as_ref().as_ref().to_vec(),
            Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
        };
        Ok(render::Font::from_bytes(
            bytes.as_slice(),
            fontdue::FontSettings::default(),
        )?)
    };
    // 与 render::style_query 一致的 (weight, style) 映射
    let styles = [
        (Weight::NORMAL, DbStyle::Normal),
        (Weight::BOLD, DbStyle::Normal),
        (Weight::NORMAL, DbStyle::Italic),
        (Weight::BOLD, DbStyle::Italic),
    ];
    // CJK 兜底只加载一次，再 clone 给四组（Font: Clone）。
    let cjk = crate::cjk_fonts().unwrap_or_default();
    let mut groups = Vec::with_capacity(4);
    for (w, st) in styles {
        let mut chain = Vec::new();
        let q = Query {
            families: &[Family::Name("monospace"), Family::Monospace],
            weight: w,
            style: st,
            ..Default::default()
        };
        if let Some(id) = db.query(&q) {
            chain.push(read_face(&db, id)?);
        }
        // CJK 兜底与 CPU 参考侧同源（crate::cjk_fonts），保证两侧字体一致。
        chain.extend(cjk.iter().cloned());
        if chain.is_empty() {
            return Err("字体组为空".into());
        }
        groups.push(chain);
    }
    Ok(groups)
}

/// 与 `render::group_for` 一致：按 bold/italic 选组。
pub fn group_for(cell: &vt::Cell) -> u8 {
    match (cell.bold, cell.italic) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_contains_cjk_and_ascii() {
        let snap = crate::build_frame(120, 30).expect("frame");
        let atlas = Atlas::build(&snap, 1.0).expect("atlas");
        assert!(atlas.glyph_count() > 20, "字形数应覆盖测试内容");
        // 'H' 出现在普通段（normal 组），'B' 出现在 BOLD 段（bold 组）。
        assert!(atlas.get('H', 1, 0).is_some(), "普通组窄字符应入图集");
        assert!(atlas.get('B', 1, 1).is_some(), "粗体组应按组区分入图集");
        assert!(atlas.get('中', 2, 0).is_some(), "中文字形应以 span=2 入图集");
        let cpu = crate::cpu_reference(&snap, 120 * 8, 30 * 16, 1.0).expect("cpu");
        assert_eq!(atlas.cell, (cpu.3, cpu.4), "cell 尺寸应与 CpuRenderer 一致");
    }
}
