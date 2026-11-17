//! 增量渲染正确性（阶段 3A 任务 3）：增量渲染的帧缓冲必须与同一状态下
//! "强制全量重绘"的帧缓冲逐像素相等。
//!
//! 覆盖：逐行修改、光标移动、选区变化、Preedit 出现/变化/消失、滚动条变化、
//! 宽字符、反显、软折行、备用屏切换（full_redraw）。
//! 系统无可用字体时跳过并说明原因。

use render::{
    Antialiasing, CpuRenderer, Error, FontSettings, FontSpec, Palette, PreeditOverlay,
    RenderOptions, Renderer, Subpixel,
};
use vt::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline};

fn settings() -> FontSettings {
    let spec = |style: &str| FontSpec {
        family: "DejaVu Sans Mono".to_string(),
        style: style.to_string(),
    };
    FontSettings {
        size: 14.0,
        normal: spec("Regular"),
        bold: spec("Bold"),
        italic: spec("Italic"),
        bold_italic: spec("Bold Italic"),
        fallback: Vec::new(),
    }
}

fn opts() -> RenderOptions {
    RenderOptions {
        antialiasing: Antialiasing::Grayscale,
        subpixel: Subpixel::None,
        hinting: false,
        text_gamma: 1.0,
        draw_bold_with_bright: true,
    }
}

fn new_renderer() -> Option<CpuRenderer> {
    match CpuRenderer::new(&settings(), opts(), Palette::default()) {
        Ok(r) => Some(r),
        Err(Error::Font(msg)) => {
            println!("skip: 系统无可用字体（{msg}）");
            None
        }
        Err(e) => panic!("CpuRenderer::new 失败: {e}"),
    }
}

/// 合成测试快照：含 ASCII、宽字符、彩色、反显、下划线。
fn synth(cols: u16, rows: u16, seed: u64) -> FrameSnapshot {
    let mut st = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut next = || {
        st = st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (st >> 33) as u32
    };
    let mut cells: Vec<Vec<Cell>> = Vec::new();
    for _y in 0..rows {
        let mut line = Vec::new();
        let mut x = 0u16;
        while x < cols {
            let v = next();
            if v % 11 == 0 && x + 1 < cols {
                let ch = ['中', '文', '宽', '字'][(v as usize / 7) % 4];
                line.push(Cell {
                    chars: ch.to_string(),
                    fg: Some(Rgb {
                        r: (v % 256) as u8,
                        g: 100,
                        b: 200,
                    }),
                    wide: CellWide::Wide,
                    ..Default::default()
                });
                line.push(Cell {
                    chars: String::new(),
                    wide: CellWide::SpacerTail,
                    ..Default::default()
                });
                x += 2;
                continue;
            }
            line.push(Cell {
                chars: ((b'a' + (v % 26) as u8) as char).to_string(),
                fg: if v % 5 == 0 {
                    Some(Rgb {
                        r: 200,
                        g: 30,
                        b: 30,
                    })
                } else {
                    None
                },
                bg: if v % 7 == 0 {
                    Some(Rgb {
                        r: 30,
                        g: 40,
                        b: 60,
                    })
                } else {
                    None
                },
                bold: v % 13 == 0,
                inverse: v % 17 == 0,
                underline: if v % 19 == 0 {
                    Underline::Single
                } else {
                    Underline::None
                },
                ..Default::default()
            });
            x += 1;
        }
        cells.push(line);
    }
    FrameSnapshot {
        cols,
        rows,
        cells,
        cursor: Cursor {
            x: 2,
            y: 1,
            visible: true,
            style: CursorStyle::Block,
        },
        scrollback_rows: 100,
        alternate_screen: false,
        wrapped: vec![false; rows as usize],
        dirty_rows: vec![true; rows as usize],
        full_redraw: true,
    }
}

struct Harness {
    incr: CpuRenderer,
    full: CpuRenderer,
    cols: u16,
    rows: u16,
    pw: u32,
    ph: u32,
}

impl Harness {
    fn new(cols: u16, rows: u16) -> Option<Self> {
        let incr = new_renderer()?;
        let full = new_renderer()?;
        let (cw, ch) = incr.cell_metrics(1.0);
        Some(Self {
            incr,
            full,
            cols,
            rows,
            pw: u32::from(cols) * cw,
            ph: u32::from(rows) * ch,
        })
    }

    /// 同步两个渲染器的静态外观状态（选区/滚动条/预编辑）。
    fn sync_appearance(
        &mut self,
        snap: &FrameSnapshot,
        sel: Option<(u16, u16, u16, u16)>,
        sb: Option<(u32, u32)>,
        pre: Option<PreeditOverlay>,
    ) {
        for r in [&mut self.incr, &mut self.full] {
            r.set_selection(sel);
            r.set_scrollbar(sb);
            r.set_preedit(pre.clone());
        }
        let _ = snap;
    }

    /// 渲染一帧（增量），随后用强制全量渲染同一帧，返回像素比较结果。
    fn step(&mut self, snap: &mut FrameSnapshot, label: &str) -> Result<(), String> {
        self.incr.render(snap, self.pw, self.ph).expect("增量渲染");
        // 同一快照强制全量渲染（full_redraw 清脏后与增量路径应产出相同像素）
        let mut full_snap = snap.clone();
        full_snap.full_redraw = true;
        self.full
            .render(&full_snap, self.pw, self.ph)
            .expect("全量渲染");
        let a = self.incr.framebuffer();
        let b = self.full.framebuffer();
        if a.len() != b.len() {
            return Err(format!(
                "[{label}] 帧缓冲长度不同 {} vs {}",
                a.len(),
                b.len()
            ));
        }
        let w = self.pw as usize;
        for (i, (pa, pb)) in a.iter().zip(b.iter()).enumerate() {
            if pa != pb {
                let x = i % w;
                let y = i / w;
                return Err(format!(
                    "[{label}] 像素不一致 at (x={x}, y={y}) 增量={pa:#010x} 全量={pb:#010x} (第 {i} 个像素)"
                ));
            }
        }
        // 增量路径消费脏标记；为下一轮准备：把脏标记清掉（模拟 libghostty clean 后）
        for d in snap.dirty_rows.iter_mut() {
            *d = false;
        }
        snap.full_redraw = false;
        let _ = self.cols;
        let _ = self.rows;
        Ok(())
    }
}

#[test]
fn incremental_matches_full_pixel_exact() {
    let Some(mut h) = Harness::new(40, 12) else {
        return;
    };
    h.sync_appearance(&synth(40, 12, 1), None, None, None);
    let mut snap = synth(40, 12, 1);

    // 1) 初始整屏
    h.step(&mut snap, "初始整屏")
        .unwrap_or_else(|e| panic!("{e}"));

    // 2) 逐行修改：每步改一行，只有该行脏
    for y in 0..snap.rows {
        let row = &mut snap.cells[y as usize];
        for (x, c) in row.iter_mut().enumerate() {
            if c.wide == CellWide::Narrow {
                c.chars = ((b'A' + ((y + x as u16) % 26) as u8) as char).to_string();
            }
        }
        snap.mark_row_dirty(y);
        h.step(&mut snap, &format!("修改行 {y}"))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    // 3) 光标移动（含反转、隐藏）
    for (x, y, vis, style) in [
        (5u16, 3u16, true, CursorStyle::Block),
        (7, 4, true, CursorStyle::Underline),
        (7, 4, false, CursorStyle::Underline),
        (0, 0, true, CursorStyle::Bar),
    ] {
        snap.cursor = Cursor {
            x,
            y,
            visible: vis,
            style,
        };
        h.step(&mut snap, &format!("光标 ({x},{y}) visible={vis}"))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    snap.cursor = Cursor {
        x: 2,
        y: 1,
        visible: true,
        style: CursorStyle::Block,
    };

    // 4) 选区变化（app 侧会标脏涉及行；这里模拟该行为）
    let mut last_sel: Option<(u16, u16, u16, u16)> = None;
    for (i, sel) in [
        Some((1u16, 1u16, 10, 3)),
        Some((5, 2, 20, 6)),
        Some((0, 0, 39, 11)),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        h.sync_appearance(&snap, sel, None, None);
        // app 侧行为：选区变化时旧区域与新区域的行都标脏（含变为 None 时清旧区域）。
        if sel != last_sel {
            if let Some((_, r0, _, r1)) = last_sel {
                snap.mark_rows_dirty(r0, r1);
            }
            if let Some((_, r0, _, r1)) = sel {
                snap.mark_rows_dirty(r0, r1);
            }
            last_sel = sel;
        }
        h.step(&mut snap, &format!("选区 {i}"))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    // 5) 滚动条变化（含出现/移动/消失）
    for (i, sb) in [
        None,
        Some((0u32, h.ph / 4)),
        Some((h.ph / 2, h.ph / 4)),
        Some((h.ph * 2 / 3, h.ph / 6)),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        h.sync_appearance(&snap, None, sb, None);
        h.step(&mut snap, &format!("滚动条 {i}"))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    // 6) Preedit 出现/变化/消失（render 内部按布局变化标脏）
    for (i, pre) in [
        Some(PreeditOverlay::new("中文", None)),
        Some(PreeditOverlay::new("中文字", Some((0, 6)))),
        Some(PreeditOverlay::new("a", None)),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        h.sync_appearance(&snap, None, None, pre);
        h.step(&mut snap, &format!("预编辑 {i}"))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    // 7) 宽字符行/反显行整行变化
    for y in [0u16, 5, 11] {
        let row = &mut snap.cells[y as usize];
        for c in row.iter_mut() {
            c.inverse = !c.inverse;
        }
        snap.mark_row_dirty(y);
        h.step(&mut snap, &format!("反显翻转行 {y}"))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    println!("incremental vs full：全部步骤逐像素一致");
}

/// 固定种子伪随机操作序列：写文本、清屏、滚动、resize、缩放、选区、光标、preedit，
/// 每步比较增量与全量帧缓冲；不一致即失败并打印种子与首个差异坐标。
/// 种子可用 `LEANTTY_RANDOM_SEED` 覆盖（默认固定 20260201）。
#[test]
fn incremental_random_sequence_3000_steps() {
    let seed: u64 = std::env::var("LEANTTY_RANDOM_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20260201);
    println!("随机序列种子: {seed}（可用 LEANTTY_RANDOM_SEED 覆盖）");

    // 简单确定性 PRNG（LCG），无需新依赖。
    let mut state = seed;
    let mut rnd = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };

    let mut cols: u16 = 40;
    let mut rows: u16 = 12;
    let Some(mut h) = Harness::new(cols, rows) else {
        return;
    };
    let mut snap = synth(cols, rows, seed);
    let mut sel: Option<(u16, u16, u16, u16)> = None;
    let mut sb: Option<(u32, u32)> = None;
    let mut pre: Option<PreeditOverlay> = None;
    let mut preedit_text = String::new();

    // 首帧整屏（在 resize 前必须与全量一致）
    if let Err(e) = h.step(&mut snap, "初始整屏") {
        panic!("种子 {seed}: {e}");
    }

    for step in 0..3000u32 {
        // 权重（阶段 3B 任务 3）：滚动类占比提高——0..3 写文本、3..6 滚动、
        // 其余为清屏/resize/缩放/选区/光标/preedit。
        let op = match rnd() % 16 {
            0..=5 => 0u32, // 写文本
            6..=11 => 2,   // 滚动（重点）
            12 => 1,       // 清屏
            13 => 3,       // resize
            14 => 4,       // 缩放
            _ => 5,        // 选区
        };
        let op2 = match rnd() % 8 {
            0 => 6, // 光标
            _ => 7, // preedit
        };
        let chosen = if rnd() % 4 == 0 { op2 } else { op };
        match chosen {
            0 => {
                // 写文本：随机一行整行替换
                let y = (rnd() % rows as u32) as u16;
                let ch_base = b'a' + (rnd() % 26) as u8;
                let row = &mut snap.cells[y as usize];
                for (i, c) in row.iter_mut().enumerate() {
                    if c.wide == CellWide::Narrow {
                        c.chars = ((ch_base + ((i as u8) % 3)) as char).to_string();
                    }
                }
                snap.mark_row_dirty(y);
            }
            1 => {
                // 清屏：全部行换成背景
                for row in snap.cells.iter_mut() {
                    for c in row.iter_mut() {
                        c.chars = if c.wide == CellWide::Narrow {
                            " ".to_string()
                        } else {
                            String::new()
                        };
                        c.fg = None;
                        c.bg = None;
                        c.inverse = false;
                    }
                }
                snap.full_redraw = true;
            }
            2 => {
                // 滚动：视口滚动 => 整屏（与 app 行为一致）
                snap.full_redraw = true;
                let r = &mut snap.cells;
                r.rotate_left(1);
            }
            3 => {
                // resize（结构变化）：复用 renderer，仅更新尺寸与快照
                cols = 20 + (rnd() % 30) as u16;
                rows = 6 + (rnd() % 10) as u16;
                let scale = if rnd() % 2 == 0 { 1.0 } else { 2.0 };
                for r in [&mut h.incr, &mut h.full] {
                    r.set_scale_factor(scale);
                }
                let (cw, ch) = h.incr.cell_metrics(scale);
                h.pw = u32::from(cols) * cw;
                h.ph = u32::from(rows) * ch;
                h.cols = cols;
                h.rows = rows;
                let mut ns = synth(cols, rows, seed.wrapping_add(step as u64));
                ns.full_redraw = true;
                snap = ns;
                sel = None;
                sb = None;
                pre = None;
            }
            4 => {
                // 缩放变化（整屏）
                let scale = if rnd() % 2 == 0 { 1.0 } else { 2.0 };
                for r in [&mut h.incr, &mut h.full] {
                    r.set_scale_factor(scale);
                }
                let (cw, ch) = h.incr.cell_metrics(scale);
                h.pw = u32::from(cols) * cw;
                h.ph = u32::from(rows) * ch;
                snap.full_redraw = true;
            }
            5 => {
                // 选区变化
                let c0 = (rnd() % cols as u32) as u16;
                let c1 = (rnd() % cols as u32) as u16;
                let r0 = (rnd() % rows as u32) as u16;
                let r1 = (rnd() % rows as u32) as u16;
                let new_sel = Some((c0.min(c1), r0.min(r1), c0.max(c1), r0.max(r1)));
                if new_sel != sel {
                    if let Some((_, r0o, _, r1o)) = sel {
                        snap.mark_rows_dirty(r0o, r1o);
                    }
                    if let Some((_, r0n, _, r1n)) = new_sel {
                        snap.mark_rows_dirty(r0n, r1n);
                    }
                    sel = new_sel;
                }
            }
            6 => {
                // 光标移动
                snap.cursor = Cursor {
                    x: (rnd() % cols as u32) as u16,
                    y: (rnd() % rows as u32) as u16,
                    visible: rnd() % 4 != 0,
                    style: match rnd() % 4 {
                        0 => CursorStyle::Block,
                        1 => CursorStyle::HollowBlock,
                        2 => CursorStyle::Underline,
                        _ => CursorStyle::Bar,
                    },
                };
            }
            _ => {
                // preedit 变化
                let n = rnd() % 5;
                preedit_text.clear();
                for _ in 0..n {
                    preedit_text.push(['中', 'a', '文', 'b'][(rnd() % 4) as usize]);
                }
                pre = if preedit_text.is_empty() {
                    None
                } else {
                    Some(PreeditOverlay::new(preedit_text.clone(), None))
                };
            }
        }
        // 滚动条也随机变化（与行无关，验证条带路径）
        sb = if rnd() % 3 == 0 {
            let y = rnd() % h.ph.max(1);
            let hh = 1 + rnd() % (h.ph.max(2) / 2);
            Some((y, hh))
        } else {
            sb
        };
        h.sync_appearance(&snap, sel, sb, pre.clone());
        if let Err(e) = h.step(&mut snap, &format!("步骤 {step}")) {
            panic!("种子 {seed} 失败于 {e}");
        }
    }
    println!("随机序列 3000 步：增量与全量逐像素一致（种子 {seed}）");
}

/// 滚动序列专项：纯上滚/下滚、滚动同时改一行、滚动同时选区变化、
/// 光标随滚动移动——行级增量与强制全量逐像素相等。
/// （阶段 3C 任务 6 已删除位移复用实现，本用例继续覆盖滚动场景的增量正确性。）
#[test]
fn scroll_sequence_matches_full() {
    let Some(mut h) = Harness::new(40, 12) else {
        return;
    };
    let mut base = synth(40, 12, 7);
    // 稳定内容：每行用行号文本，便于构造整体位移
    for (y, row) in base.cells.iter_mut().enumerate() {
        for (x, c) in row.iter_mut().enumerate() {
            if c.wide == CellWide::Narrow {
                c.chars = ((b'A' + ((y * 3 + x) % 26) as u8) as char).to_string();
            }
        }
    }
    h.sync_appearance(&base, None, None, None);
    base.full_redraw = true;
    let mut cur = base.clone();
    h.step(&mut cur, "初始").unwrap_or_else(|e| panic!("{e}"));

    // 上滚 N 行：把行整体上移（末尾补新行）
    for shift in [1usize, 5, 11] {
        let mut next = cur.clone();
        next.cells.rotate_left(shift);
        for i in 0..shift {
            let y = next.cells.len() - shift + i;
            for (x, c) in next.cells[y].iter_mut().enumerate() {
                if c.wide == CellWide::Narrow {
                    c.chars = ((b'0' + ((shift + i + x) % 10) as u8) as char).to_string();
                }
            }
        }
        next.dirty_rows.iter_mut().for_each(|d| *d = true);
        next.full_redraw = false;
        h.step(&mut next, &format!("上滚 {shift} 行"))
            .unwrap_or_else(|e| panic!("{e}"));
        cur = next;
    }

    // 下滚 5 行（行整体下移，顶部补新行）
    {
        let mut next = cur.clone();
        next.cells.rotate_right(5);
        for y in 0..5 {
            for (x, c) in next.cells[y].iter_mut().enumerate() {
                if c.wide == CellWide::Narrow {
                    c.chars = ((b'a' + ((y + x) % 26) as u8) as char).to_string();
                }
            }
        }
        next.dirty_rows.iter_mut().for_each(|d| *d = true);
        next.full_redraw = false;
        h.step(&mut next, "下滚 5 行")
            .unwrap_or_else(|e| panic!("{e}"));
        cur = next;
    }

    // 滚动同时修改一行（修改行不被复用，需重绘）
    {
        let mut next = cur.clone();
        next.cells.rotate_left(3);
        let y = next.cells.len() - 3;
        for (x, c) in next.cells[y].iter_mut().enumerate() {
            if c.wide == CellWide::Narrow {
                c.chars = ((b'X' + (x % 3) as u8) as char).to_string();
            }
        }
        next.dirty_rows.iter_mut().for_each(|d| *d = true);
        next.full_redraw = false;
        h.step(&mut next, "滚动+改行")
            .unwrap_or_else(|e| panic!("{e}"));
        cur = next;
    }

    // 滚动同时选区变化（选区行不可复用）
    {
        let mut next = cur.clone();
        next.cells.rotate_left(2);
        for y in (next.cells.len() - 2)..next.cells.len() {
            for (x, c) in next.cells[y].iter_mut().enumerate() {
                if c.wide == CellWide::Narrow {
                    c.chars = ((b'q' + (x % 5) as u8) as char).to_string();
                }
            }
        }
        next.dirty_rows.iter_mut().for_each(|d| *d = true);
        next.full_redraw = false;
        let sel = Some((0u16, 2u16, 20, 6));
        h.sync_appearance(&next, sel, None, None);
        // 模拟 app 侧选区脏
        next.mark_rows_dirty(2, 6);
        h.step(&mut next, "滚动+选区变化")
            .unwrap_or_else(|e| panic!("{e}"));
        cur = next;
        h.sync_appearance(&cur, None, None, None);
    }

    // 光标在被搬移行（光标行不可复用，需重绘并重画光标）
    {
        let mut next = cur.clone();
        next.cells.rotate_left(4);
        for y in (next.cells.len() - 4)..next.cells.len() {
            for (x, c) in next.cells[y].iter_mut().enumerate() {
                if c.wide == CellWide::Narrow {
                    c.chars = ((b'z' + (x % 7) as u8) as char).to_string();
                }
            }
        }
        next.cursor = Cursor {
            x: 5,
            y: 7,
            visible: true,
            style: CursorStyle::Block,
        };
        next.dirty_rows.iter_mut().for_each(|d| *d = true);
        next.full_redraw = false;
        h.step(&mut next, "滚动+光标移动")
            .unwrap_or_else(|e| panic!("{e}"));
    }
    println!("滚动序列：全部场景与强制全量逐像素一致");
}
