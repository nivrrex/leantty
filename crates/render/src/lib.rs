//! render crate：渲染抽象。
//!
//! `Renderer` 只接收自有快照与像素尺寸，不暴露任何后端类型。
//! 字体 family 与字号用按平台 cfg 的默认常量 + 命令行 `--font`/`--font-size`
//! 覆盖，不引入配置文件。

use vt::FrameSnapshot;

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

/// 基于 softbuffer 的 CPU 光栅化渲染器。
///
/// 字体查找用 fontdb，光栅化用 fontdue，单字体等宽网格。
pub struct CpuRenderer {
    font_family: String,
    font_size: f32,
}

impl CpuRenderer {
    /// 新建渲染器，字体 family 与字号用默认常量或命令行覆盖值。
    pub fn new(font_family: Option<String>, font_size: Option<f32>) -> Result<Self, Error> {
        Ok(Self {
            font_family: font_family.unwrap_or_else(|| DEFAULT_FONT_FAMILY.to_string()),
            font_size: font_size.unwrap_or(DEFAULT_FONT_SIZE),
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
}

impl Renderer for CpuRenderer {
    fn render(
        &mut self,
        _frame: &FrameSnapshot,
        _pixel_width: u32,
        _pixel_height: u32,
    ) -> Result<(), Error> {
        // 阶段1骨架：真实光栅化在实现阶段接入 softbuffer/fontdb/fontdue。
        Ok(())
    }
}
