//! 渲染输出逐字节不变（阶段 4N t3 的黄金校验）。
//!
//! 做法：用固定 VT 序列喂出确定的 `FrameSnapshot`（ASCII / CJK 回退 / 反色 / 粗体 /
//! 宽字符 / 光标），再用 `CpuRenderer` 渲染到固定尺寸，输出帧缓冲的 64 位哈希。
//! 该测试不依赖 X、不依赖输入时序，因此跨构建可比对（用于 4N t3 前后"逐字节相同"）。
//!
//! 哈希用 FNV-1a 64，覆盖全部像素（ARGB u32）。
//!
//! ## 可移植性（阶段 4O t2）
//! 黄金哈希依赖**本机实际选中的字体**。为了让该测试在其它机器上不误报，测试先计算
//! "字体环境指纹"（主字体文件名、字节数、前 4KB 哈希、渲染器实测 cell 尺寸）：
//! - 指纹等于记录值 → 真正断言黄金哈希（Linux 本机必须走到这一步）；
//! - 指纹不符 → **SKIP** 并打印"指纹 X，期望 Y"与本机当前哈希，供人工更新；
//! - `TEST_FONT_FILE=<路径>` 指定外部字体时同样 SKIP（渲染器按族名加载，指纹不可比）。
//!
//! 字体来源一律按族名经 fontdb 查询（或用 `TEST_FONT_FILE`），**不含任何绝对路径**。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::{Rgb, VtPalette, VtTerminal};

/// 主字体族名（与 settings() 一致；按族名经 fontdb 解析，不写死路径）。
const MAIN_FAMILY: &str = "DejaVu Sans Mono";
/// 记录下来的字体环境指纹（Linux 本机，4N t3 之后的代码 + DejaVu Sans Mono）。
const GOLDEN_FINGERPRINT: &str = "DejaVuSansMono.ttf:343140:c69b:8x16";

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

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
        // 含 CJK 回退：覆盖"回退字体参与渲染"的路径（4N t3 改的就是这条链的共享方式）
        fallback: vec![FontSpec {
            family: "Noto Sans CJK SC".to_string(),
            style: "Regular".to_string(),
        }],
    }
}

fn palette() -> Palette {
    let c = |r, g, b| Rgb { r, g, b };
    Palette {
        primary_bg: c(0x10, 0x10, 0x10),
        primary_fg: c(0xd0, 0xd0, 0xd0),
        cursor_text: c(0x10, 0x10, 0x10),
        cursor: c(0x30, 0xa0, 0x30),
        normal: [
            c(0, 0, 0),
            c(0xcc, 0x66, 0x66),
            c(0xb5, 0xbd, 0x68),
            c(0xf0, 0xc6, 0x74),
            c(0x81, 0xa2, 0xbe),
            c(0xb2, 0x94, 0xbb),
            c(0x8a, 0xbe, 0xb7),
            c(0xc5, 0xc8, 0xc6),
        ],
        bright: [
            c(0x66, 0x66, 0x66),
            c(0xd5, 0x4e, 0x53),
            c(0x9c, 0xca, 0x65),
            c(0xf0, 0xc6, 0x74),
            c(0x4f, 0x9b, 0xd1),
            c(0xb2, 0x94, 0xbb),
            c(0x8a, 0xbe, 0xb7),
            c(0xf0, 0xf0, 0xf0),
        ],
    }
}

fn vt_palette() -> VtPalette {
    let c = |r, g, b| Rgb { r, g, b };
    VtPalette {
        foreground: c(0xd0, 0xd0, 0xd0),
        background: c(0x10, 0x10, 0x10),
        cursor: c(0x30, 0xa0, 0x30),
        normal: [c(0, 0, 0); 8],
        bright: [c(0xff, 0xff, 0xff); 8],
    }
}

/// 固定输入：ASCII、粗体、反色、宽字符、CJK、下划线、光标定位。
const SCRIPT: &[u8] = b"\x1b[2J\x1b[H\
ABC abc 0123 ~!@#$%^&*()\r\n\
\x1b[1mBOLD\x1b[0m \x1b[7mINVERSE\x1b[0m \x1b[4mUNDER\x1b[0m\r\n\
wide: \xe4\xb8\xad\xe6\x96\x87 wide2: \xef\xbc\xa1\xef\xbc\xa2\r\n\
\x1b[31mred\x1b[32mgreen\x1b[33myellow\x1b[0m\r\n\
\x1b[5;10Hpositioned";

/// 渲染器实测 cell 尺寸（随字体与字号变化，纳入指纹）。
fn cell_metrics() -> (u32, u32) {
    let r = CpuRenderer::new(&settings(), RenderOptions::default(), palette()).expect("renderer");
    r.cell_metrics(1.0)
}

fn render_hash() -> (u64, u32, u32) {
    let mut t = VtTerminal::new_with_palette(40, 12, &vt_palette()).expect("VtTerminal");
    t.feed(SCRIPT);
    let mut r = CpuRenderer::new(&settings(), RenderOptions::default(), palette()).expect("renderer");
    let (cw, ch) = r.cell_metrics(1.0);
    let snap = t.snapshot().expect("snapshot");
    let pw = u32::from(snap.cols) * cw;
    let ph = u32::from(snap.rows) * ch;
    r.render(&snap, pw, ph).expect("render");
    let fb = r.framebuffer();
    let mut bytes = Vec::with_capacity(fb.len() * 4);
    for px in fb {
        bytes.extend_from_slice(&px.to_le_bytes());
    }
    (fnv1a64(&bytes), pw, ph)
}

/// 字体环境指纹：`文件名:字节数:前4KB的FNV哈希低16位:cell宽x高`。
///
/// `TEST_FONT_FILE` 存在时用它（外部字体 → 与渲染器实际加载的族名无关，测试将 SKIP）。
fn font_fingerprint(cw: u32, ch: u32) -> String {
    if let Ok(p) = std::env::var("TEST_FONT_FILE") {
        let path = std::path::PathBuf::from(&p);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let head = &bytes[..bytes.len().min(4096)];
        return format!(
            "{}:{}:{:04x}:{}x{}",
            path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "<env>".to_string()),
            bytes.len(),
            fnv1a64(head) & 0xFFFF,
            cw,
            ch
        );
    }
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let q = fontdb::Query {
        families: &[fontdb::Family::Name(MAIN_FAMILY)],
        ..Default::default()
    };
    let Some(id) = db.query(&q) else {
        return "<no-font>".to_string();
    };
    let Some(face) = db.face(id) else {
        return "<no-face>".to_string();
    };
    let (name, bytes) = match &face.source {
        fontdb::Source::File(p) => (
            p.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            std::fs::read(p).unwrap_or_default(),
        ),
        fontdb::Source::Binary(d) => ("<binary>".to_string(), d.as_ref().as_ref().to_vec()),
        fontdb::Source::SharedFile(p, d) => (
            p.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            d.as_ref().as_ref().to_vec(),
        ),
    };
    let head = &bytes[..bytes.len().min(4096)];
    format!(
        "{name}:{}:{:04x}:{cw}x{ch}",
        bytes.len(),
        fnv1a64(head) & 0xFFFF
    )
}

#[test]
fn deterministic_frame_hash_is_stable() {
    let (h1, w, h) = render_hash();
    let (h2, _, _) = render_hash();
    assert_eq!(h1, h2, "同一进程内两次渲染应完全一致");
    let (cw, ch) = cell_metrics();
    let fp = font_fingerprint(cw, ch);
    println!("frame_hash={h1:016x} dims={w}x{h} fingerprint={fp}");

    if std::env::var("TEST_FONT_FILE").is_ok() {
        println!(
            "SKIP: TEST_FONT_FILE 指定了外部字体（指纹 {fp}），渲染器按族名加载，指纹不可比；\
             本机当前哈希 {h1:016x}"
        );
        return;
    }
    if fp != GOLDEN_FINGERPRINT {
        println!(
            "SKIP: 字体环境指纹不符——实测 {fp}，期望 {GOLDEN_FINGERPRINT}；\
             本机当前哈希 {h1:016x}（确认无误后可更新 GOLDEN/GOLDEN_FINGERPRINT）"
        );
        return;
    }
    // 指纹匹配（Linux 本机基准环境）：真正断言黄金哈希。
    // 黄金值：4N t3（Arc 共享）与改前（深拷贝）逐字节相同，均为该值。
    assert_eq!(
        h1, GOLDEN,
        "渲染输出与黄金值不一致（若为有意变更，请更新 GOLDEN 并在报告中说明）"
    );
}

/// 黄金哈希（4N t3 前后一致；来源见 PHASE4N-LINUX.md）。
const GOLDEN: u64 = 0x5074_85fb_7945_fb8f;
