//! 视口上滚后不应画出光标（阶段 4G t1，像素级）。
//!
//! 光标色取与背景/文字显著不同的值，统计整窗该色像素：上滚后应为 0，回底后 > 0。

use render::{CpuRenderer, FontSettings, FontSpec, RenderOptions, Renderer};
use vt::ScrollViewport;
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

/// 光标色取醒目的品红，与默认前景(白)/背景(黑)都不同。
const CURSOR: Rgb = Rgb {
    r: 0xFF,
    g: 0x00,
    b: 0xFF,
};

fn palette() -> render::Palette {
    render::Palette {
        primary_bg: Rgb { r: 0, g: 0, b: 0 },
        primary_fg: Rgb {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        },
        cursor_text: Rgb { r: 0, g: 0, b: 0 },
        cursor: CURSOR,
        normal: [Rgb { r: 0, g: 0, b: 0 }; 8],
        bright: [Rgb {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        }; 8],
    }
}

fn vt_palette() -> VtPalette {
    let c = |r: u8, g: u8, b: u8| Rgb { r, g, b };
    VtPalette {
        foreground: c(0xFF, 0xFF, 0xFF),
        background: c(0, 0, 0),
        cursor: CURSOR,
        normal: [c(0, 0, 0); 8],
        bright: [c(0xFF, 0xFF, 0xFF); 8],
    }
}

fn term() -> VtTerminal {
    let mut t = VtTerminal::new_with_palette(40, 10, &vt_palette()).expect("VtTerminal");
    t.feed(b"\x1b[2J\x1b[H");
    for i in 0..30 {
        t.feed(format!("line {i:02}\r\n").as_bytes());
    }
    t.feed(b"prompt$ ");
    t
}

/// 统计帧缓冲中等于光标块的像素数（块光标整格填充）。
fn cursor_pixels(fb: &[u32]) -> usize {
    let target = 0xFF00_0000
        | (u32::from(CURSOR.r) << 16)
        | (u32::from(CURSOR.g) << 8)
        | u32::from(CURSOR.b);
    fb.iter().filter(|&&p| p == target).count()
}

fn render_cursor_pixels(mut t: &mut VtTerminal) -> (usize, bool) {
    let mut r =
        CpuRenderer::new(&settings(), RenderOptions::default(), palette()).expect("renderer");
    let snap = t.snapshot().expect("snapshot");
    let (cw, ch) = r.cell_metrics(1.0);
    let pw = u32::from(snap.cols) * cw;
    let ph = u32::from(snap.rows) * ch;
    r.render(&snap, pw, ph).expect("render");
    let n = cursor_pixels(r.framebuffer());
    let _ = &mut t;
    (n, snap.cursor.visible)
}

#[test]
fn cursor_pixels_absent_after_scroll_up_and_back_after_bottom() {
    let mut t = term();
    let (at_bottom, vis_bottom) = render_cursor_pixels(&mut t);
    eprintln!("回底：光标像素={at_bottom} visible={vis_bottom}");
    assert!(vis_bottom, "回底时光标应可见");
    assert!(at_bottom > 0, "回底时应画出光标块（实测 0）");

    t.scroll_viewport(ScrollViewport::Delta(-5));
    let (scrolled, vis_scrolled) = render_cursor_pixels(&mut t);
    eprintln!("上滚 5 行：光标像素={scrolled} visible={vis_scrolled}");
    assert!(!vis_scrolled, "上滚后光标应不可见");
    assert_eq!(scrolled, 0, "上滚后不应存在光标色像素（实测 {scrolled}）");

    t.scroll_viewport(ScrollViewport::Bottom);
    let (back, vis_back) = render_cursor_pixels(&mut t);
    eprintln!("回底后：光标像素={back} visible={vis_back}");
    assert!(vis_back && back > 0, "回底后光标应重新出现");
}

/// GPU 侧同样断言（无适配器显式 SKIP 并计数）。
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn cursor_pixels_absent_after_scroll_up_gpu() {
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
    let mut t = term();
    let (cw, ch) = gpu.cell_metrics(1.0);
    let mut count = |t: &VtTerminal| -> (usize, bool) {
        let snap = t.snapshot().expect("snapshot");
        let pw = u32::from(snap.cols) * cw;
        let ph = u32::from(snap.rows) * ch;
        gpu.render(&snap, pw, ph).expect("gpu render");
        let rgba = gpu.framebuffer_rgba();
        let mut n = 0;
        for i in 0..(pw * ph) as usize {
            let px = (u32::from(rgba[i * 4]) << 16)
                | (u32::from(rgba[i * 4 + 1]) << 8)
                | u32::from(rgba[i * 4 + 2]);
            if px == 0x00FF_00FF {
                n += 1;
            }
        }
        (n, snap.cursor.visible)
    };
    let (bottom, vis) = count(&t);
    eprintln!("[gpu] 回底：光标像素={bottom} visible={vis}");
    assert!(vis && bottom > 0, "回底时 GPU 应画出光标");
    t.scroll_viewport(ScrollViewport::Delta(-5));
    let (scrolled, vis2) = count(&t);
    eprintln!("[gpu] 上滚：光标像素={scrolled} visible={vis2}");
    assert!(!vis2, "GPU 上滚后光标应不可见");
    assert_eq!(scrolled, 0, "GPU 上滚后不应有光标色像素");
}
