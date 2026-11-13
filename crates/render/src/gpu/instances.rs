//! GPU 实例结构（顶点布局须与 `shaders.rs` 一致）。

/// 纯色矩形实例（背景/样式线/光标填充）。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BgInstance {
    pub rect: [f32; 4],
    pub color: [f32; 3],
    pub _pad: f32,
}

/// 字形实例。
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
