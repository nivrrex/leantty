//! 单元格绘制决策（CPU 与 GPU 后端共用，阶段 4C t4 起为唯一来源）。
//!
//! 背景色、字形前景色、**字形混合底色**、样式线颜色、占格数与续格标记都在这里
//! 一次算清；两条光栅化路径只负责把这些决策画成像素，不得各自再分叉。
//!
//! 阶段 4C t4 修复：`blend_bg` 取 **inverse/选区交换之后的底色**（`bgc`）。
//! 此前 CPU 的 `blit` 用的是填充背景色 `bg`，导致反色格渲染成"黑底黑字"
//! （默认调色板下完全不可见）；GPU 为与 CPU 逐像素一致而照抄了该缺陷。

use vt::Cell;

use crate::Palette;

/// 单元格的绘制决策（纯逻辑结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellDecision {
    /// 背景填充色（0x00RRGGBB）：**最终底色**，inverse/选区交换后与 `blend_bg` 相同。
    pub bg: u32,
    /// 字形前景色。
    pub fg: u32,
    /// 字形混合底（inverse/选区交换后的底色；反色格应在此底色上画前景色）。
    pub blend_bg: u32,
    /// 样式线颜色。
    pub line: u32,
    /// 占格数（1 或 2）。
    pub span: u32,
    /// 是否为宽字符续格（不画字形）。
    pub is_tail: bool,
}

/// `decide` 的输入上下文（打包参数，避免超长参数列表）。
#[derive(Clone, Copy)]
pub struct DecideCtx<'a> {
    pub x: usize,
    pub y: usize,
    pub row_len: usize,
    pub bg_default: u32,
    pub fg_default: u32,
    /// 流式（阅读顺序）选区：(anchor_row, anchor_col, focus_row, focus_col)，
    /// 均为视口坐标；`None` 表示无选区。
    pub selection: Option<(u16, u16, u16, u16)>,
    pub palette: &'a Palette,
    pub draw_bold_with_bright: bool,
}

/// 计算一个单元格的绘制决策（纯函数）。
pub fn decide(cell: &Cell, ctx: &DecideCtx<'_>) -> CellDecision {
    let is_tail = matches!(
        cell.wide,
        vt::CellWide::SpacerTail | vt::CellWide::SpacerHead
    );
    let span = if cell.wide == vt::CellWide::Wide && ctx.x + 1 < ctx.row_len {
        2
    } else {
        1
    };
    let in_sel = ctx.selection.is_some_and(|(ar, ac, fr, fc)| {
        let (start_row, start_col, end_row, end_col) = if ar <= fr {
            (ar, ac, fr, fc)
        } else {
            (fr, fc, ar, ac)
        };
        let cols = ctx.row_len.max(1) as u16;
        let last = cols.saturating_sub(1);
        let row = ctx.y as u16;
        let col = ctx.x as u16;
        if row < start_row || row > end_row {
            return false;
        }
        let (c0, c1) = if start_row == end_row {
            (start_col.min(end_col), start_col.max(end_col))
        } else if row == start_row {
            (start_col, last)
        } else if row == end_row {
            (0, end_col.min(last))
        } else {
            (0, last)
        };
        col >= c0 && col <= c1
    });
    let sel = in_sel && !is_tail;
    let orig_bg = cell.bg.map(rgb_to_u32).unwrap_or(ctx.bg_default);
    let orig_fg = cell.fg.map(rgb_to_u32).unwrap_or(ctx.fg_default);
    let bg = if sel { orig_fg } else { orig_bg };
    let mut fg = orig_fg;
    let mut bgc = bg;
    if cell.inverse {
        std::mem::swap(&mut fg, &mut bgc);
    }
    if cell.bold && ctx.draw_bold_with_bright {
        fg = map_to_bright(fg, ctx.palette);
    }
    if cell.faint {
        fg = dim(fg, bgc);
    }
    let fg = if sel { orig_bg } else { fg };
    let bgc = if sel { orig_fg } else { bgc };
    // 样式线：反色时用交换后的底色（与 CPU 既有行为一致）
    let line = if cell.inverse { bgc } else { fg };
    // 填充底色必须也是交换后的值：反色格要"整格换成原前景色作底、原背景色作字"，
    // 若填充仍用原背景色，字形矩形之外的区域会保留旧底色（表现为黑底黑字）。
    let bg = bgc;
    CellDecision {
        bg,
        fg,
        blend_bg: bgc,
        line,
        span,
        is_tail,
    }
}

/// `0xAARRGGBB`（不透明黑为 `0xFF000000`）：CPU 帧缓冲要求 alpha 位为 0xFF，
/// 与阶段 1 以来的既有编码一致（GPU 侧只取低 24 位，不受影响）。
pub(crate) fn rgb_to_u32(c: vt::Rgb) -> u32 {
    0xFF00_0000 | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

pub(crate) fn dim(fg: u32, bg: u32) -> u32 {
    let r = ((fg >> 16) & 0xFF) / 2 + ((bg >> 16) & 0xFF) / 2;
    let g = ((fg >> 8) & 0xFF) / 2 + ((bg >> 8) & 0xFF) / 2;
    let b = (fg & 0xFF) / 2 + (bg & 0xFF) / 2;
    (r << 16) | (g << 8) | b
}

fn map_to_bright(fg: u32, palette: &Palette) -> u32 {
    for i in 0..8 {
        if fg == rgb_to_u32(palette.normal[i]) {
            return rgb_to_u32(palette.bright[i]);
        }
    }
    fg
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt::{CellWide, Underline};

    fn cell() -> Cell {
        Cell {
            chars: "H".to_string(),
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
            underline: Underline::None,
            wide: CellWide::Narrow,
        }
    }

    fn ctx(p: &Palette) -> DecideCtx<'_> {
        DecideCtx {
            x: 0,
            y: 0,
            row_len: 1,
            bg_default: 0x000000,
            fg_default: 0xFFFFFF,
            selection: None,
            palette: p,
            draw_bold_with_bright: false,
        }
    }

    #[test]
    fn inverse_swaps_blend_background() {
        let p = Palette::default();
        let mut c = cell();
        c.inverse = true;
        let d = decide(&c, &ctx(&p));
        // 底色 = 原前景（白），字色 = 原背景（黑）
        assert_eq!(d.blend_bg, 0xFFFFFF, "反色格底色应为原前景色");
        assert_eq!(d.fg, 0x000000, "反色格字色应为原背景色");
        // 填充底色同样是交换后的原前景色（字形矩形之外也必须是白）
        assert_eq!(d.bg, 0xFFFFFF);
    }

    #[test]
    fn normal_cell_unchanged() {
        let p = Palette::default();
        let d = decide(&cell(), &ctx(&p));
        assert_eq!(d.blend_bg, 0x000000);
        assert_eq!(d.fg, 0xFFFFFF);
        assert_eq!(d.bg, 0x000000);
        assert_eq!(d.span, 1);
        assert!(!d.is_tail);
    }

    #[test]
    fn selection_overrides_inverse() {
        let p = Palette::default();
        let mut c = cell();
        c.inverse = true;
        let mut cx = ctx(&p);
        cx.selection = Some((0, 0, 10, 10)); // (ar, ac, fr, fc)：第 0 行第 0 列 → 第 10 行第 10 列
        let d = decide(&c, &cx);
        // 选区：底色=原前景(白)、字色=原背景(黑)
        assert_eq!(d.blend_bg, 0xFFFFFF);
        assert_eq!(d.fg, 0x000000);
        assert_eq!(d.bg, 0xFFFFFF);
    }
}
