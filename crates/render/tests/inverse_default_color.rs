//! 反色（inverse）与默认色语义回归（阶段 4H t4，只加测试，不改渲染代码）。
//!
//! 背景：Windows 3F-b 观察到"默认反色块底色是 normal.white(#757575) 而不是
//! primary.foreground"。本测试把两种情形各自的**预期语义**固定下来：
//!
//! 1. `ESC[37m ESC[7m` + 空格：显式前景 = 索引色 7 → 反色后**底色 = normal[7]**。
//! 2. `ESC[7m` + 空格（无显式色）：前景 = primary.foreground → 反色后**底色 =
//!    primary.foreground**。
//!
//! 两个调色板取值刻意不同（否则两种情形无法区分）。GPU 侧同断言，无可用适配器
//! 时显式 SKIP 并计数。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::{Rgb, VtPalette, VtTerminal};

fn settings() -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: "DejaVu Sans Mono".to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 13.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
    }
}

/// 默认前景（primary.foreground）与 normal[7] 取不同值，便于区分两条语义。
const PRIMARY_FG: Rgb = Rgb {
    r: 0x11,
    g: 0x22,
    b: 0x33,
};
const NORMAL7: Rgb = Rgb {
    r: 0xAB,
    g: 0xCD,
    b: 0xEF,
};
const BG: Rgb = Rgb {
    r: 0x00,
    g: 0x00,
    b: 0x00,
};

fn palette() -> Palette {
    let mut normal = [Rgb { r: 0, g: 0, b: 0 }; 8];
    normal[7] = NORMAL7;
    Palette {
        primary_bg: BG,
        primary_fg: PRIMARY_FG,
        cursor_text: BG,
        cursor: PRIMARY_FG,
        normal,
        bright: [NORMAL7; 8],
    }
}

fn vt_palette() -> VtPalette {
    let mut normal = [Rgb { r: 0, g: 0, b: 0 }; 8];
    normal[7] = NORMAL7;
    VtPalette {
        foreground: PRIMARY_FG,
        background: BG,
        cursor: PRIMARY_FG,
        normal,
        bright: [NORMAL7; 8],
    }
}

fn term_with(bytes: &[u8]) -> VtTerminal {
    let mut t = VtTerminal::new_with_palette(20, 4, &vt_palette()).expect("VtTerminal");
    t.feed(b"\x1b[2J\x1b[H");
    t.feed(bytes);
    t
}

/// 反色空格用例：`ESC[37m ESC[7m 空格`（显式索引色 7）。
fn explicit_37_inverse() -> VtTerminal {
    term_with(b"\x1b[37m\x1b[7m \x1b[0m")
}

/// 默认色反色用例：`ESC[7m 空格`（无显式色）。
fn default_inverse() -> VtTerminal {
    term_with(b"\x1b[7m \x1b[0m")
}

fn rgb_u32(c: Rgb) -> u32 {
    0xFF00_0000 | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

/// 取第 (col,row) 格中心像素（CPU 帧缓冲为 ARGB u32）。
fn pixel_at_cpu(r: &CpuRenderer, col: u32, row: u32, cw: u32, ch: u32) -> u32 {
    let (w, _) = r.framebuffer_size();
    let x = col * cw + cw / 2;
    let y = row * ch + ch / 2;
    r.framebuffer()[(y * w + x) as usize]
}

fn render_cpu(t: &mut VtTerminal) -> (CpuRenderer, u32, u32) {
    let mut r =
        CpuRenderer::new(&settings(), RenderOptions::default(), palette()).expect("renderer");
    let (cw, ch) = r.cell_metrics(1.0);
    let snap = t.snapshot().expect("snapshot");
    let pw = u32::from(snap.cols) * cw;
    let ph = u32::from(snap.rows) * ch;
    r.render(&snap, pw, ph).expect("render");
    (r, cw, ch)
}

#[test]
fn inverse_with_explicit_37_uses_normal7_as_background() {
    let mut t = explicit_37_inverse();
    let (r, cw, ch) = render_cpu(&mut t);
    let px = pixel_at_cpu(&r, 0, 0, cw, ch);
    eprintln!(
        "显式 37 + 反色：底色 = #{:06X}（期望 normal[7] = #{:06X}）",
        px & 0x00FF_FFFF,
        rgb_u32(NORMAL7) & 0x00FF_FFFF
    );
    assert_eq!(
        px,
        rgb_u32(NORMAL7),
        "ESC[37m 的显式索引色在反色后应作底色（normal[7]），而不是 primary.foreground"
    );
}

#[test]
fn inverse_without_explicit_color_uses_primary_foreground_as_background() {
    let mut t = default_inverse();
    let (r, cw, ch) = render_cpu(&mut t);
    let px = pixel_at_cpu(&r, 0, 0, cw, ch);
    eprintln!(
        "默认色 + 反色：底色 = #{:06X}（期望 primary.foreground = #{:06X}）",
        px & 0x00FF_FFFF,
        rgb_u32(PRIMARY_FG) & 0x00FF_FFFF
    );
    assert_eq!(
        px,
        rgb_u32(PRIMARY_FG),
        "无显式色的反色格底色应取 primary.foreground"
    );
}

/// 两个取值必须不同，否则上面的两个测试无法区分语义。
#[test]
fn palette_values_are_distinguishable() {
    assert_ne!(
        NORMAL7, PRIMARY_FG,
        "normal[7] 与 primary.foreground 必须不同"
    );
    assert_ne!(NORMAL7, BG);
    assert_ne!(PRIMARY_FG, BG);
}

/// GPU 侧同一语义（无适配器时显式 SKIP 并计数）。
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn inverse_default_color_semantics_gpu() {
    use render::Error;
    use render::gpu::GpuRenderer;

    let mut gpu = match GpuRenderer::new_offscreen(
        &settings(),
        RenderOptions::default(),
        palette(),
        render::gpu::allow_software_from_env(),
    ) {
        Ok(g) => g,
        Err(Error::Backend(msg)) => {
            eprintln!("SKIP: 无可用真实 GPU 适配器（{msg}）");
            return;
        }
        Err(e) => panic!("GpuRenderer 创建失败: {e}"),
    };
    let (cw, ch) = gpu.cell_metrics(1.0);
    let mut sample = |t: &mut VtTerminal| -> u32 {
        let snap = t.snapshot().expect("snapshot");
        let pw = u32::from(snap.cols) * cw;
        let ph = u32::from(snap.rows) * ch;
        gpu.render(&snap, pw, ph).expect("gpu render");
        let rgba = gpu.framebuffer_rgba();
        let x = (cw / 2) as usize;
        let y = (ch / 2) as usize;
        let i = (y * pw as usize + x) * 4;
        (u32::from(rgba[i]) << 16) | (u32::from(rgba[i + 1]) << 8) | u32::from(rgba[i + 2])
    };
    let got37 = sample(&mut explicit_37_inverse());
    let gotdef = sample(&mut default_inverse());
    let want37 = rgb_u32(NORMAL7) & 0x00FF_FFFF;
    let wantdef = rgb_u32(PRIMARY_FG) & 0x00FF_FFFF;
    eprintln!("[gpu] 显式 37 反色底色 = #{got37:06X}（期望 #{want37:06X}）");
    eprintln!("[gpu] 默认色反色底色 = #{gotdef:06X}（期望 #{wantdef:06X}）");
    assert_eq!(got37, want37, "GPU：ESC[37m 反色底色应为 normal[7]");
    assert_eq!(gotdef, wantdef, "GPU：默认反色底色应为 primary.foreground");
}
