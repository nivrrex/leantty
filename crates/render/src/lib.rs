//! render crate：渲染抽象。
//!
//! `Renderer` 只接收自有快照与像素尺寸，不暴露任何后端类型。
//! 字体 family 与字号用按平台 cfg 的默认常量 + 命令行 `--font`/`--font-size`
//! 覆盖，不引入配置文件。
//!
//! 实现说明：`CpuRenderer` 的光栅化核心基于 fontdb（字体查找）+ fontdue
//! （位图光栅化），帧缓冲为自有 `Vec<u32>`；softbuffer 作为窗口 surface
//! 后端在 app 接线阶段接入（需要 winit 窗口），见 docs/DECISIONS.md。

use fontdb::{Database, Family, Source};
use fontdue::Font;
use vt::{FrameSnapshot, Rgb};

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

/// 基于 fontdb/fontdue 的 CPU 光栅化渲染器，单字体等宽网格。
pub struct CpuRenderer {
    font_family: String,
    font_size: f32,
    font: Option<Font>,
    framebuffer: Vec<u32>,
    fb_width: u32,
    fb_height: u32,
}

impl CpuRenderer {
    /// 新建渲染器，字体 family 与字号用默认常量或命令行覆盖值。
    pub fn new(font_family: Option<String>, font_size: Option<f32>) -> Result<Self, Error> {
        let font_family = font_family.unwrap_or_else(|| DEFAULT_FONT_FAMILY.to_string());
        let font_size = font_size.unwrap_or(DEFAULT_FONT_SIZE);
        let font = load_font(&font_family, font_size)?;
        Ok(Self {
            font_family,
            font_size,
            font: Some(font),
            framebuffer: Vec::new(),
            fb_width: 0,
            fb_height: 0,
        })
    }

    /// 字体 family。
    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    /// 字号（pt）。
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// 当前帧缓冲（ARGB 打包为 u32，低 24 位为 RGB）。
    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    /// 帧缓冲尺寸。
    pub fn framebuffer_size(&self) -> (u32, u32) {
        (self.fb_width, self.fb_height)
    }
}

impl Renderer for CpuRenderer {
    fn render(
        &mut self,
        frame: &FrameSnapshot,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<(), Error> {
        let font = self
            .font
            .as_ref()
            .ok_or_else(|| Error::Font("font not loaded".to_string()))?;
        let cols = u32::from(frame.cols).max(1);
        let rows = u32::from(frame.rows).max(1);
        let cell_w = pixel_width / cols;
        let cell_h = pixel_height / rows;
        self.fb_width = pixel_width;
        self.fb_height = pixel_height;
        self.framebuffer = vec![0; (pixel_width * pixel_height) as usize];

        for (y, row) in frame.cells.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                let px = x as u32 * cell_w;
                let py = y as u32 * cell_h;
                let bg = cell.bg.map(rgb_to_u32).unwrap_or(0xFF000000);
                fill_rect(&mut self.framebuffer, pixel_width, px, py, cell_w, cell_h, bg);
                if cell.chars.is_empty() {
                    continue;
                }
                let fg = cell.fg.map(rgb_to_u32).unwrap_or(0xFFFFFFFF);
                // 单字体等宽网格：每格光栅化首个字符。
                let ch = cell.chars.chars().next().expect("非空 chars");
                let (metrics, bitmap) = font.rasterize(ch, self.font_size);
                blit(
                    &mut self.framebuffer,
                    pixel_width,
                    (px, py, cell_w, cell_h),
                    &metrics,
                    &bitmap,
                    fg,
                );
            }
        }
        Ok(())
    }
}

fn rgb_to_u32(c: Rgb) -> u32 {
    0xFF000000 | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
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

fn blit(
    fb: &mut [u32],
    width: u32,
    cell: (u32, u32, u32, u32),
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    fg: u32,
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
            fb[idx] = blend(fg, alpha);
        }
    }
}

/// 按 alpha 混合前景色到背景（帧缓冲此处已填背景色，直接覆盖写前景）。
fn blend(fg: u32, alpha: u8) -> u32 {
    let a = u32::from(alpha);
    ((fg & 0x00FF_FFFF) * a / 255) | 0xFF00_0000
}

fn load_font(family: &str, size: f32) -> Result<Font, Error> {
    let mut db = Database::new();
    db.load_system_fonts();
    let query = fontdb::Query {
        families: &[Family::Name(family)],
        ..Default::default()
    };
    let id = db
        .query(&query)
        .ok_or_else(|| Error::Font(format!("family '{}' not found in system fonts", family)))?;
    let face = db.face(id).ok_or_else(|| Error::Font("face not found".to_string()))?;
    let bytes: Vec<u8> = match &face.source {
        Source::File(path) => std::fs::read(path)
            .map_err(|e| Error::Font(format!("read font file: {}", e)))?,
        Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };
    let _ = size;
    Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
        .map_err(|e| Error::Font(format!("fontdue parse: {}", e)))
}

#[cfg(test)]
mod blit_tests {
    use super::blit;

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
        blit(&mut fb, width, (0, 0, 8, 4), &m, &bitmap, FG);
        // 本格 [0,8) 列有着色
        assert!(
            fb[0..8].contains(&FG),
            "本格内应有前景像素"
        );
        // 右邻格 [8,16) 列全背景
        let invaded = fb[8..16].iter().filter(|&&p| p != BG).count();
        assert_eq!(
            invaded, 0,
            "右邻格应全为背景色，被侵入像素 {}",
            invaded
        );
    }

    /// 位图超出格高：下邻格像素不变。
    #[test]
    fn blit_clips_oversize_height_to_cell() {
        let width = 8u32; // 单格宽 8，fb 高 8（上下两格各 4）
        let mut fb = vec![BG; (width * 8) as usize];
        let m = metrics(0, 2, 4, 8); // py 覆盖 [2,10)，格高 4 → [4,8) 越界
        let bitmap = vec![255u8; 4 * 8];
        blit(&mut fb, width, (0, 0, 8, 4), &m, &bitmap, FG);
        // 本格行 [0,4) 有着色
        assert!(
            fb[0..32].contains(&FG),
            "本格内应有前景像素"
        );
        // 下邻格行 [4,8) 全背景
        let invaded = fb[32..64].iter().filter(|&&p| p != BG).count();
        assert_eq!(
            invaded, 0,
            "下邻格应全为背景色，被侵入像素 {}",
            invaded
        );
    }
}
