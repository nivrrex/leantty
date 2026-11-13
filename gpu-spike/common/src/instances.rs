//! 一帧的实例数据：由 `FrameSnapshot` + 图集构建，wgpu 与 GL 两个 spike 共用，
//! 保证两条 GPU 路径的 CPU 侧输入完全一致（对比口径相同）。

use std::error::Error;

use crate::atlas::{Atlas, group_for};

/// 一帧的实例数据（分区边界见 `gpu::Renderer::submit_render`）。
pub struct Instances {
    pub quads: Vec<BgInstance>,
    pub glyphs: Vec<GlyphInstance>,
    /// quads 中前 `bg_n` 个是单元格背景（其后为样式线与光标填充）。
    pub bg_n: u32,
    /// glyphs 中前 `glyph_n` 个是普通字形（其后为光标格字符）。
    pub glyph_n: u32,
}


/// 纯色矩形实例（背景/样式线/光标）；wgpu 侧对应 shader 的 BgInstance。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BgInstance {
    pub rect: [f32; 4],
    pub color: [f32; 3],
    pub _pad: f32,
}

/// 字形实例（布局必须与 shader 的 GlyphInstance 一致）。
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

pub fn rgb3(c: vt::Rgb) -> [f32; 3] {
    [
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
    ]
}

pub fn dim(fg: vt::Rgb, bg: vt::Rgb) -> vt::Rgb {
    vt::Rgb {
        r: fg.r / 2 + bg.r / 2,
        g: fg.g / 2 + bg.g / 2,
        b: fg.b / 2 + bg.b / 2,
    }
}

/// CPU 侧构建实例数据：完整复刻 `CpuRenderer::draw_row` + `draw_style_lines` + `draw_cursor`
/// 的绘制顺序与颜色决策，使像素对比只反映"光栅化/混合实现的差异"，不含语义差异。
pub fn build_instances(
    snap: &vt::FrameSnapshot,
    atlas: &Atlas,
    cw: u32,
    ch: u32,
) -> Result<Instances, Box<dyn Error>> {
    let bg_default = atlas.default_bg();
    let fg_default = atlas.default_fg();
    let palette = render::Palette::default();
    let sel = crate::SELECTION;
    let n_cells: usize = snap.cells.iter().map(Vec::len).sum();
    let mut quads = Vec::with_capacity(n_cells * 2);
    let mut glyphs = Vec::with_capacity(n_cells);

    // --- 逐 cell：背景 → 字形 → 样式线 ---
    for (y, row) in snap.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let px0 = x as u32 * cw;
            let py0 = y as u32 * ch;
            let is_tail = matches!(
                cell.wide,
                vt::CellWide::SpacerTail | vt::CellWide::SpacerHead
            );
            let span = if cell.wide == vt::CellWide::Wide && x + 1 < row.len() {
                2
            } else {
                1
            };
            let area_w = cw * span;
            let in_sel = (x as u16) >= sel.0
                && (x as u16) <= sel.2
                && (y as u16) >= sel.1
                && (y as u16) <= sel.3;
            let sel_here = in_sel && !is_tail;

            let orig_bg = cell.bg.unwrap_or(bg_default);
            let orig_fg = cell.fg.unwrap_or(fg_default);
            let bg_fill = if sel_here { orig_fg } else { orig_bg };
            quads.push(BgInstance {
                rect: [px0 as f32, py0 as f32, area_w as f32, ch as f32],
                color: rgb3(bg_fill),
                _pad: 0.0,
            });

            if is_tail || cell.chars.is_empty() || cell.invisible {
                continue;
            }

            let mut fg = orig_fg;
            let mut bgc = bg_fill;
            if cell.inverse {
                std::mem::swap(&mut fg, &mut bgc);
            }
            if cell.faint {
                fg = dim(fg, bgc);
            }
            if sel_here {
                fg = orig_bg;
                bgc = orig_fg;
            }

            let ch0 = cell.chars.chars().next().ok_or("空字符意外")?;
            if let Some(g) = atlas
                .get(ch0, span as u8, group_for(cell))
                .filter(|g| g.w > 0 && g.h > 0)
            {
                glyphs.push(GlyphInstance {
                    rect: [
                        (px0 as i32 + g.xmin) as f32,
                        (py0 as i32 + g.ymin) as f32,
                        g.w as f32,
                        g.h as f32,
                    ],
                    // 裁剪到**起始格**而不是宽字符的 2 格宽度：CPU 侧 blit 的实际
                    // 可见范围就是单格（实测把裁剪放宽到 2 格会让续格多出 5-10 列，
                    // 与 CPU 输出不一致）。这是对齐参照实现的选择，不是物理正确性。
                    cell: [px0 as f32, py0 as f32, cw as f32, ch as f32],
                    uv: [g.u0, g.v0, g.u1, g.v1],
                    color: rgb3(fg),
                    backdrop: rgb3(bgc),
                    _pad: 0.0,
                });
            }

            // 样式线：颜色 = inverse ? bgc : fg（与 CPU 一致）
            let line = if cell.inverse { bgc } else { fg };
            push_style_lines(&mut quads, cell, px0, py0, area_w, ch, line);
        }
    }
    let bg_n = quads.len() as u32;
    let glyph_n = glyphs.len() as u32;

    // --- 光标（覆盖在全部内容之上）---
    let cur = &snap.cursor;
    if cur.visible {
        let cx = u32::from(cur.x) * cw;
        let cy = u32::from(cur.y) * ch;
        let color = rgb3(palette.cursor);
        let push = |quads: &mut Vec<BgInstance>, x: u32, y: u32, w: u32, h: u32, c: [f32; 3]| {
            quads.push(BgInstance {
                rect: [x as f32, y as f32, w as f32, h as f32],
                color: c,
                _pad: 0.0,
            });
        };
        match cur.style {
            vt::CursorStyle::Block => {
                push(&mut quads, cx, cy, cw, ch, color);
                // 光标格字符以 cursor_text 色重画（底=cursor 色）
                if let Some(cell) = snap
                    .cells
                    .get(cur.y as usize)
                    .and_then(|r| r.get(cur.x as usize))
                    && !cell.chars.is_empty()
                {
                    let ch0 = cell.chars.chars().next().ok_or("空字符意外")?;
                    if let Some(g) = atlas.get(ch0, 1, 0).filter(|g| g.w > 0 && g.h > 0) {
                        glyphs.push(GlyphInstance {
                            rect: [
                                (cx as i32 + g.xmin) as f32,
                                (cy as i32 + g.ymin) as f32,
                                g.w as f32,
                                g.h as f32,
                            ],
                            cell: [cx as f32, cy as f32, cw as f32, ch as f32],
                            uv: [g.u0, g.v0, g.u1, g.v1],
                            color: rgb3(palette.cursor_text),
                            backdrop: color,
                            _pad: 0.0,
                        });
                    }
                }
            }
            vt::CursorStyle::HollowBlock => {
                push(&mut quads, cx, cy, cw, 1, color);
                push(&mut quads, cx, cy + ch - 1, cw, 1, color);
                push(&mut quads, cx, cy, 1, ch, color);
                push(&mut quads, cx + cw - 1, cy, 1, ch, color);
            }
            vt::CursorStyle::Underline => {
                push(&mut quads, cx, cy + ch - 1, cw, 1, color);
                push(&mut quads, cx, cy + ch - 2, cw, 1, color);
            }
            vt::CursorStyle::Bar => {
                push(&mut quads, cx, cy, 1, ch, color);
                push(&mut quads, cx + 1, cy, 1, ch, color);
            }
        }
    }

    Ok(Instances {
        quads,
        glyphs,
        bg_n,
        glyph_n,
    })
}

/// 复刻 `render::draw_style_lines`：下划线（Single/Double/Dotted/Dashed/Curly→Single）、
/// 删除线（h/2）、上划线（y=1）。虚线按下标取模（i % gap == 0）逐像素点。
pub fn push_style_lines(
    quads: &mut Vec<BgInstance>,
    cell: &vt::Cell,
    px0: u32,
    py0: u32,
    w: u32,
    h: u32,
    color: vt::Rgb,
) {
    let c = rgb3(color);
    let hline = |quads: &mut Vec<BgInstance>, y: i32, gap: u32| {
        if y < 0 || y >= h as i32 {
            return;
        }
        let mut i = 0u32;
        while i < w {
            quads.push(BgInstance {
                rect: [(px0 + i) as f32, (py0 + y as u32) as f32, 1.0, 1.0],
                color: c,
                _pad: 0.0,
            });
            i += gap;
        }
    };
    match cell.underline {
        vt::Underline::None => {}
        vt::Underline::Single => hline(quads, h as i32 - 2, 1),
        vt::Underline::Double => {
            hline(quads, h as i32 - 2, 1);
            hline(quads, h as i32 - 4, 1);
        }
        vt::Underline::Dotted => hline(quads, h as i32 - 2, 2),
        vt::Underline::Dashed => hline(quads, h as i32 - 2, 4),
        vt::Underline::Curly => hline(quads, h as i32 - 2, 1),
    }
    if cell.strikethrough {
        hline(quads, h as i32 / 2, 1);
    }
    if cell.overline {
        hline(quads, 1, 1);
    }
}

