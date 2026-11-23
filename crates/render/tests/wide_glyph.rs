//! t1 宽字符取证（阶段 4B）：CpuRenderer 渲染含 CJK 的快照后，逐列统计
//! 非背景像素数，判断宽字符是否完整绘制在 2 格宽内。
//!
//! 输出可直接作为像素证据：列号 → 该列非背景像素数。若字形只出现在起始格
//! （前 cell_w 列）而续格全为 0，则说明 CPU 侧存在截断。

use render::{CpuRenderer, FontSettings, FontSpec, Palette, RenderOptions, Renderer};
use vt::{Cursor, CursorStyle, FrameSnapshot};

/// 主字体：Linux 用 fontconfig 的通用族名 "monospace"；Windows 的 DirectWrite 不认这个别名
/// （FontNotFound），退回系统等宽字体 Consolas。用例只验证跨两格绘制，与具体字形无关。
fn main_family() -> &'static str {
    if cfg!(windows) {
        "Consolas"
    } else {
        "monospace"
    }
}

/// 与 app 一致：主字体等宽族 + 系统 CJK 兜底。
fn settings() -> FontSettings {
    let spec = |family: &str, style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 16.0,
        normal: spec(main_family(), "Regular"),
        bold: spec(main_family(), "Bold"),
        italic: spec(main_family(), "Italic"),
        bold_italic: spec(main_family(), "Bold Italic"),
        fallback: vec![
            spec("Noto Sans CJK SC", "Regular"),
            spec("Noto Sans CJK JP", "Regular"),
            spec("WenQuanYi Zen Hei", "Regular"),
            spec("DejaVu Sans", "Regular"),
        ],
        synthetic_bold: true,
    }
}

fn snap_with(text: &str) -> FrameSnapshot {
    let mut term = vt::VtTerminal::new(40, 2).expect("VtTerminal");
    term.feed(b"\x1b[2J\x1b[H");
    term.feed(text.as_bytes());
    term.feed(b"\x1b[1;1H");
    let mut s = term.snapshot().expect("snapshot");
    s.full_redraw = true;
    for d in s.dirty_rows.iter_mut() {
        *d = true;
    }
    s
}

#[test]
fn cjk_wide_glyph_spans_two_cells() {
    let mut r = CpuRenderer::new(&settings(), RenderOptions::default(), Palette::default())
        .expect("CpuRenderer::new");
    let (cw, ch) = r.cell_metrics(1.0);
    assert!(cw > 0 && ch > 0);

    // 用外框字符 '回'：其字形填充接近整格宽，能可靠判断是否跨格
    // （'中' 的右半可能是空白，会与"截断"混淆）。
    let snap = snap_with("回A");
    assert_eq!(
        snap.cells[0][0].wide,
        vt::CellWide::Wide,
        "首个字符应为宽字符"
    );
    assert_eq!(snap.cells[0][0].chars, "回");
    assert_eq!(
        snap.cells[0][1].wide,
        vt::CellWide::SpacerTail,
        "第二格应为宽字符续格"
    );

    let (pw, ph) = (40 * cw, 2 * ch);
    r.render(&snap, pw, ph).expect("render");
    let (fw, fh) = r.framebuffer_size();
    assert_eq!((fw, fh), (pw, ph));
    let fb = r.framebuffer();

    let bg = Palette::default().primary_bg;
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
    let is_ink = |px: u32| (px & 0x00FF_FFFF) != bg_u32;

    // 逐列统计前 4 格范围（0..4*cw）内的墨迹像素数。
    let cols_to_scan = 4 * cw;
    let mut per_col = vec![0u32; cols_to_scan as usize];
    for y in 0..ch {
        for x in 0..cols_to_scan {
            if is_ink(fb[(y * fw + x) as usize]) {
                per_col[x as usize] += 1;
            }
        }
    }
    eprintln!("cell = {cw}x{ch}；列 0..{cols_to_scan} 的墨迹像素数：");
    for (x, n) in per_col.iter().enumerate() {
        if x % cw as usize == 0 {
            eprint!("\n  [cell {}] ", x / cw as usize);
        }
        eprint!("{n:2} ");
    }
    eprintln!();

    let first: u32 = per_col[..cw as usize].iter().sum();
    let second: u32 = per_col[cw as usize..(2 * cw) as usize].iter().sum();
    let third: u32 = per_col[(2 * cw) as usize..(3 * cw) as usize].iter().sum();
    eprintln!("  起始格(0)墨迹={first}  续格(1)墨迹={second}  下一字符格(2)墨迹={third}");

    assert!(first > 0, "起始格应有字形像素");
    // 结论判断：宽字形是否跨到续格。
    if second == 0 {
        panic!(
            "CPU 侧宽字形未跨格：续格墨迹为 0（字形被裁到起始单格）。\
             这属于 CPU 截断缺陷，需先修 CPU 再继续 4B。"
        );
    }
    // 且不得越界到第 3 格（那是 'A' 的位置，'A' 自身有墨迹，故只检查不误判）。
    eprintln!("PASS: 宽字形跨越 2 格（续格墨迹 {second} 像素）");
}

/// 手工构造快照（绕过 vt）：排除 "vt snapshot 的 wide 字段" 因素。
#[test]
fn manual_wide_cell_spans_two_cells() {
    let spec = |family: &str, style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    let st = FontSettings {
        size: 16.0,
        normal: spec(main_family(), "Regular"),
        bold: spec(main_family(), "Bold"),
        italic: spec(main_family(), "Italic"),
        bold_italic: spec(main_family(), "Bold Italic"),
        // 0.2.9 起字形覆盖按真实 cmap 判断（不再借 fontconfig 自动替换），
        // 所以这里必须显式给出含汉字的回退字体；本用例只验证"跨两格绘制"。
        fallback: vec![spec("Noto Sans CJK SC", "Regular")],
        synthetic_bold: true,
    };
    let mut r = CpuRenderer::new(&st, RenderOptions::default(), Palette::default()).expect("new");
    let (cw, ch) = r.cell_metrics(1.0);
    let blank = |ch_str: &str, wide: vt::CellWide| vt::Cell {
        chars: ch_str.to_string(),
        fg: None,
        bg: None,
        bold: false,
        italic: false,
        faint: false,
        blink: false,
        inverse: false,
        invisible: false,
        strikethrough: false,
        overline: false,
        underline: vt::Underline::None,
        wide,
    };
    let mut cells = vec![
        blank("回", vt::CellWide::Wide),
        blank("", vt::CellWide::SpacerTail),
    ];
    cells.extend((2..20).map(|_| blank("", vt::CellWide::Narrow)));
    let snap = FrameSnapshot {
        cols: cells.len() as u16,
        rows: 1,
        cells: vec![cells],
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: false,
            style: CursorStyle::Block,
        },
        scrollback_rows: 0,
        alternate_screen: false,
        wrapped: Vec::new(),
        dirty_rows: vec![true],
        full_redraw: true,
    };
    let (pw, ph) = (snap.cols as u32 * cw, ch);
    r.render(&snap, pw, ph).expect("render");
    let fb = r.framebuffer();
    let bg = Palette::default().primary_bg;
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
    let mut per_col = vec![0u32; (3 * cw) as usize];
    for y in 0..ch {
        for x in 0..3 * cw {
            if (fb[(y * pw + x) as usize] & 0x00FF_FFFF) != bg_u32 {
                per_col[x as usize] += 1;
            }
        }
    }
    let first: u32 = per_col[..cw as usize].iter().sum();
    let second: u32 = per_col[cw as usize..(2 * cw) as usize].iter().sum();
    // 参照：同一字体对 '回' 直接光栅化后的非零 alpha 列范围（含 xmin 偏移）。
    // 阶段 0.2.8：回退字体在构造时已加载，直接问 FontSet 要 '回' 的位图作参照。
    let set = render::FontSet::load(&settings()).expect("fontset");
    let (m, bm) = {
        let mut set = set.borrow_mut();
        let idx = set.index_for(0, '回').expect("回 应有回退字形");
        let (m, bm) = set.rasterize(0, idx, '回', 16.0).expect("rasterize");
        (m, bm)
    };
    let mut ink_cols = Vec::new();
    for gx in 0..m.width {
        if (0..m.height).any(|gy| bm[gy * m.width + gx] != 0) {
            ink_cols.push(gx as i32 + m.xmin);
        }
    }
    eprintln!(
        "位图参照: {}x{} xmin={} 非零 alpha 列范围={:?}..{:?}",
        m.width,
        m.height,
        m.xmin,
        ink_cols.first(),
        ink_cols.last()
    );
    let mut rendered_cols = Vec::new();
    for (x, n) in per_col.iter().enumerate() {
        if *n > 0 {
            rendered_cols.push(x as i32);
        }
    }
    eprintln!(
        "渲染墨迹列范围={:?}..{:?}",
        rendered_cols.first(),
        rendered_cols.last()
    );
    eprintln!("手工构造: cell={cw}x{ch} 起始格墨迹={first} 续格墨迹={second}");
    assert!(first > 0);
    assert!(
        second > 0,
        "手工构造的 Wide cell 也未跨格 → render 的 span/blit 逻辑问题"
    );
}

/// 对照实验：把 CJK 字体直接设为主字体（不经 add_background_fonts），
/// 用于区分"字体选择问题"与"blit 裁剪逻辑问题"。
#[test]
fn cjk_as_primary_font_spans_two_cells() {
    let spec = |family: &str, style: &str| FontSpec {
        family: family.to_string(),
        style: style.to_string(),
    };
    let st = FontSettings {
        size: 16.0,
        normal: spec("Noto Sans CJK SC", "Regular"),
        bold: spec("Noto Sans CJK SC", "Bold"),
        italic: spec("Noto Sans CJK SC", "Regular"),
        bold_italic: spec("Noto Sans CJK SC", "Bold"),
        fallback: Vec::new(),
        synthetic_bold: true,
    };
    let mut r = match CpuRenderer::new(&st, RenderOptions::default(), Palette::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("SKIP: CJK 主字体不可用: {e}");
            return;
        }
    };
    let (cw, ch) = r.cell_metrics(1.0);
    let snap = snap_with("回A");
    let (pw, ph) = (40 * cw, 2 * ch);
    r.render(&snap, pw, ph).expect("render");
    let fb = r.framebuffer();
    let bg = Palette::default().primary_bg;
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);
    let mut per_col = vec![0u32; (3 * cw) as usize];
    for y in 0..ch {
        for x in 0..3 * cw {
            if (fb[(y * pw + x) as usize] & 0x00FF_FFFF) != bg_u32 {
                per_col[x as usize] += 1;
            }
        }
    }
    let first: u32 = per_col[..cw as usize].iter().sum();
    let second: u32 = per_col[cw as usize..(2 * cw) as usize].iter().sum();
    eprintln!("主字体=CJK: cell={cw}x{ch} 起始格墨迹={first} 续格墨迹={second}");
    assert!(first > 0, "起始格应有字形像素");
    assert!(
        second > 0,
        "CJK 主字体下宽字形仍被裁到单格 → 问题在 blit 裁剪逻辑，而非字体选择"
    );
}

#[test]
fn cjk_wide_glyph_200x60_scale2() {
    let mut r = CpuRenderer::new(&settings(), RenderOptions::default(), Palette::default())
        .expect("CpuRenderer::new");
    r.set_scale_factor(2.0);
    let (cw, ch) = r.cell_metrics(2.0);
    let mut term = vt::VtTerminal::new(200, 60).expect("VtTerminal");
    term.feed(b"\x1b[2J\x1b[H");
    let mut line = String::new();
    for _ in 0..20 {
        line.push_str("中文字形宽度验证 ");
    }
    term.feed(line.as_bytes());
    let mut snap = term.snapshot().expect("snapshot");
    snap.full_redraw = true;
    for d in snap.dirty_rows.iter_mut() {
        *d = true;
    }
    let (pw, ph) = (200 * cw, 60 * ch);
    r.render(&snap, pw, ph).expect("render");
    let fb = r.framebuffer();
    let bg = Palette::default().primary_bg;
    let bg_u32 = (u32::from(bg.r) << 16) | (u32::from(bg.g) << 8) | u32::from(bg.b);

    // 统计前 4 个 cell 的墨迹，确认续格有内容。
    let mut per_col = vec![0u32; (4 * cw) as usize];
    for y in 0..ch {
        for x in 0..4 * cw {
            if (fb[(y * pw + x) as usize] & 0x00FF_FFFF) != bg_u32 {
                per_col[x as usize] += 1;
            }
        }
    }
    let first: u32 = per_col[..cw as usize].iter().sum();
    let second: u32 = per_col[cw as usize..(2 * cw) as usize].iter().sum();
    eprintln!("2x: cell={cw}x{ch} 起始格墨迹={first} 续格墨迹={second}");
    assert!(first > 0);
    assert!(second > 0, "2x 下宽字形同样应跨越到续格");
}
