//! WGSL：实例化 quad（背景/样式线/光标）与字形 quad。像素坐标，y 向下。

/// 纯色矩形实例：矩形 + RGB。
pub const BG_WGSL: &str = r#"
struct BgInstance {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec3<f32>,
};
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
};
@group(0) @binding(0) var<uniform> screen: vec2<f32>;

fn corner(i: u32) -> vec2<f32> {
    var cs = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    return cs[i];
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: BgInstance) -> VsOut {
    let c = corner(vi);
    let p = inst.rect.xy + c * inst.rect.zw;
    let ndc = vec2<f32>(p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0);
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// 字形实例：字形矩形 + 裁剪用的单元格矩形 + 图集 uv + 前景/混合底色。
pub const GLYPH_WGSL: &str = r#"
struct GlyphInstance {
    @location(0) rect: vec4<f32>,
    @location(1) cell: vec4<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec3<f32>,
    @location(4) backdrop: vec3<f32>,
};
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(5) backdrop: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) page: vec2<f32>,
    @location(3) cell_min: vec2<f32>,
    @location(4) cell_max: vec2<f32>,
};
@group(0) @binding(0) var<uniform> screen: vec2<f32>;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

fn corner(i: u32) -> vec2<f32> {
    var cs = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    return cs[i];
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: GlyphInstance) -> VsOut {
    let c = corner(vi);
    let p = inst.rect.xy + c * inst.rect.zw;
    let ndc = vec2<f32>(p.x / screen.x * 2.0 - 1.0, 1.0 - p.y / screen.y * 2.0);
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.color = inst.color;
    out.backdrop = inst.backdrop;
    out.uv = mix(inst.uv.xy, inst.uv.zw, c);
    out.page = p;
    out.cell_min = inst.cell.xy;
    out.cell_max = inst.cell.xy + inst.cell.zw;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.page.x < in.cell_min.x || in.page.x >= in.cell_max.x ||
        in.page.y < in.cell_min.y || in.page.y >= in.cell_max.y) {
        discard;
    }
    let a = textureSample(atlas, samp, in.uv).r;
    if (a == 0.0) {
        discard;
    }
    return vec4<f32>(mix(in.backdrop, in.color, a), 1.0);
}
"#;
