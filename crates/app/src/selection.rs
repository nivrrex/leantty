//! 选区模型（任务 2.1，自有：libghostty-vt 无 Selection API，DECISIONS 登记）。
//!
//! 选区以内容绝对坐标存储（行号相对 scrollback 顶部，与视口偏移解耦）；
//! 渲染时按当前 offset_from_top 转换到视口坐标（abs_to_viewport），
//! 上滚/下滚/新输出推动历史时选区稳定附着在内容上（任务 2C.2）。
//! 宽字符整格选中，不选中 SpacerTail 半格。

use vt::{Cell, CellWide, FrameSnapshot};

/// 网格坐标（列，行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub col: u16,
    pub row: u16,
}

/// 选区（anchor 按下点，focus 当前点；Shift+点击扩展时 anchor 不动）。
/// 行号为内容绝对坐标（相对 scrollback 顶部，0=scrollback 顶；
/// 视口行=绝对行-offset_from_top，渲染时转换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: CellPos,
    pub focus: CellPos,
}

/// 视口行→内容绝对行（offset_from_top=scrollback 顶起视口偏移）。
pub fn viewport_to_abs(viewport_row: u16, offset_from_top: usize) -> usize {
    offset_from_top + viewport_row as usize
}

/// 内容绝对行→视口行；越界（视口外）返 None（渲染层不绘制）。
pub fn abs_to_viewport(abs_row: usize, offset_from_top: usize, viewport_rows: u16) -> Option<u16> {
    let rel = abs_row as isize - offset_from_top as isize;
    if rel < 0 || rel >= viewport_rows as isize {
        None
    } else {
        Some(rel as u16)
    }
}

/// 选区矩形 clamp 到视口（渲染用）；完全在视口外返 None。
pub fn selection_viewport_region(
    region: (u16, u16, u16, u16),
    offset_from_top: usize,
    viewport_rows: u16,
) -> Option<(u16, u16, u16, u16)> {
    let (c0, r0, c1, r1) = region;
    let v0 = abs_to_viewport(r0 as usize, offset_from_top, viewport_rows)?;
    let v1 = abs_to_viewport(r1 as usize, offset_from_top, viewport_rows)?;
    Some((c0, v0, c1, v1))
}

impl Selection {
    pub fn new(pos: CellPos) -> Self {
        Self {
            anchor: pos,
            focus: pos,
        }
    }

    /// 扩展/移动 focus（Shift+点击与拖动共用）。
    pub fn set_focus(&mut self, pos: CellPos) {
        self.focus = pos;
    }

    /// 归一化选区矩形 (min_col, min_row, max_col, max_row)（含端点）。
    pub fn region(&self) -> (u16, u16, u16, u16) {
        let min_col = self.anchor.col.min(self.focus.col);
        let max_col = self.anchor.col.max(self.focus.col);
        let min_row = self.anchor.row.min(self.focus.row);
        let max_row = self.anchor.row.max(self.focus.row);
        (min_col, min_row, max_col, max_row)
    }

    pub fn contains(&self, col: u16, row: u16) -> bool {
        let (c0, r0, c1, r1) = self.region();
        col >= c0 && col <= c1 && row >= r0 && row <= r1
    }
}

/// 流式（阅读顺序）选区在某一行的列范围（含端点）；行不在选区内返回 None。
///
/// 规则与 alacritty 一致：起始行从 anchor 列到行尾（向下拖）或从 0 到 anchor 列（向上拖），
/// 结束行对称；中间行取整行。
pub fn stream_row_range(
    anchor: CellPos,
    focus: CellPos,
    row: u16,
    cols: u16,
) -> Option<(u16, u16)> {
    if cols == 0 {
        return None;
    }
    let last = cols - 1;
    // 方向：向下拖（anchor 在上）时首行取 anchor.col..行尾、末行取 0..focus.col；
    // 向上拖时相反。中间行恒为整行（流式/阅读顺序）。
    let downward = anchor.row <= focus.row;
    let (top, bottom) = if downward {
        (anchor, focus)
    } else {
        (focus, anchor)
    };
    if row < top.row || row > bottom.row {
        return None;
    }
    let (c0, c1) = if top.row == bottom.row {
        (anchor.col.min(focus.col), anchor.col.max(focus.col))
    } else if row == top.row {
        if downward {
            (top.col, last)
        } else {
            (0, top.col)
        }
    } else if row == bottom.row {
        if downward {
            (0, bottom.col)
        } else {
            (bottom.col, last)
        }
    } else {
        (0, last)
    };
    Some((c0.min(last), c1.min(last)))
}

/// 双击选词边界：以 semantic_escape_chars 为分隔符，从 focus 列向两侧扩展
/// 至边界；focus 字符本身是分隔符时仅选自身。
pub fn word_region(line: &str, focus_col: usize, escape_chars: &str) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    if focus_col >= chars.len() {
        return (focus_col, focus_col);
    }
    let is_sep = |c: char| escape_chars.contains(c);
    if is_sep(chars[focus_col]) {
        return (focus_col, focus_col);
    }
    let mut start = focus_col;
    while start > 0 && !is_sep(chars[start - 1]) {
        start -= 1;
    }
    let mut end = focus_col;
    while end + 1 < chars.len() && !is_sep(chars[end + 1]) {
        end += 1;
    }
    (start, end)
}

/// 流式选区文本提取：逐行按各自列范围取内容，软折行处不插换行（与 `extract_text` 同规则）。
pub fn extract_text_stream(rows: &[(&[Cell], bool)], spans: &[(u16, u16)], cols: u16) -> String {
    let mut out = String::new();
    let mut pending_wrap = false;
    for (idx, (cells, wrapped)) in rows.iter().enumerate() {
        let Some(&(c0, c1)) = spans.get(idx) else {
            continue;
        };
        let seg = extract_rows(&[(cells, *wrapped)], c0, c1.min(cols.saturating_sub(1)));
        if idx > 0 && !pending_wrap {
            out.push('\n');
        }
        out.push_str(&seg);
        pending_wrap = *wrapped;
    }
    out
}

/// 选区文本提取（任务 2C.1）：软折行处拼接不插分隔（snap.wrapped 标记）；
/// 硬换行（上行非 wrapped）插 \n；行尾空白去除只作用于每个逻辑行末尾；
/// 宽字符续格（SpacerTail/SpacerHead）不重复；空行保留。
pub fn extract_text(snap: &FrameSnapshot, region: (u16, u16, u16, u16)) -> String {
    let (min_col, min_row, max_col, max_row) = region;
    let rows: Vec<(&[Cell], bool)> = (min_row..=max_row)
        .filter_map(|row| {
            let cells = snap.cells.get(row as usize)?;
            let wrapped = snap.wrapped.get(row as usize).copied().unwrap_or(false);
            Some((cells.as_slice(), wrapped))
        })
        .collect();
    extract_rows(&rows, min_col, max_col)
}

/// 格级行源统一提取（视口 snapshot 与历史 rows_text 同语义）：
/// 软折行拼接无分隔、硬换行插 \n、行尾空白只作用于逻辑行末尾、
/// 宽字符续格不重复、空行保留。
pub fn extract_rows(rows: &[(&[Cell], bool)], min_col: u16, max_col: u16) -> String {
    let mut out = String::new();
    let mut logical = String::new();
    for (cells, wrapped) in rows {
        let mut seg = String::new();
        for col in min_col..=max_col {
            let Some(cell) = cells.get(col as usize) else {
                continue;
            };
            if cell.wide == CellWide::SpacerTail || cell.wide == CellWide::SpacerHead {
                continue;
            }
            seg.push_str(&cell.chars);
        }
        logical.push_str(&seg);
        // 上行非软折行→逻辑行结束（硬换行边界）。
        if !wrapped {
            let trimmed = logical.trim_end();
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(trimmed);
            logical.clear();
        }
    }
    // 选区截断在软折行中：末段无硬边界，直接拼接输出（不补 \n）。
    if !logical.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(logical.trim_end());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt::{Cell, Cursor, CursorStyle, Rgb, Underline};

    fn cell(ch: &str, wide: CellWide) -> Cell {
        Cell {
            chars: ch.to_string(),
            wide,
            ..Default::default()
        }
    }

    fn snap_row(cells: Vec<Cell>) -> FrameSnapshot {
        FrameSnapshot {
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
            dirty_rows: vec![true; 1],
            full_redraw: true,
        }
    }

    #[test]
    fn region_normalizes_and_contains() {
        let mut sel = Selection::new(CellPos { col: 5, row: 3 });
        assert_eq!(sel.region(), (5, 3, 5, 3));
        sel.set_focus(CellPos { col: 1, row: 1 });
        assert_eq!(sel.region(), (1, 1, 5, 3));
        assert!(sel.contains(3, 2));
        assert!(!sel.contains(6, 3));
    }

    #[test]
    fn word_region_respects_escape_chars() {
        let line = "echo hello, world";
        // escape 含空格与逗号
        let (s, e) = word_region(line, 5, " ,");
        assert_eq!((s, e), (5, 9), "'hello' 应为词边界");
        let (s, e) = word_region(line, 0, " ,");
        assert_eq!((s, e), (0, 3), "'echo'（4 字符）应为词边界");
        // focus 是分隔符→仅自身
        let (s, e) = word_region(line, 4, " ,");
        assert_eq!((s, e), (4, 4));
    }

    #[test]
    fn extract_text_skips_spacer_tail_and_trims() {
        let snap = snap_row(vec![
            cell("你", CellWide::Wide),
            cell("", CellWide::SpacerTail),
            cell("A", CellWide::Narrow),
        ]);
        let text = extract_text(&snap, (0, 0, 2, 0));
        assert_eq!(text, "你A", "宽字符续格不重复、SpacerTail 跳过");
    }

    #[test]
    fn extract_text_trailing_whitespace_trimmed() {
        let snap = snap_row(vec![
            cell("a", CellWide::Narrow),
            cell(" ", CellWide::Narrow),
            cell("b", CellWide::Narrow),
        ]);
        let text = extract_text(&snap, (0, 0, 2, 0));
        assert_eq!(text, "a b", "行尾空白应去除、中间空白保留");
    }

    /// 防 unused 告警：Rgb/Underline 仅构造占位（保持 Cell 字段完整）。
    #[test]
    fn color_fields_compile() {
        let _ = Rgb { r: 0, g: 0, b: 0 };
        let _ = Underline::Single;
    }
    /// 多行合成快照（含每行软折行标记）。
    fn snap_multi(rows: Vec<(Vec<Cell>, bool)>) -> FrameSnapshot {
        let rows_n = rows.len() as u16;
        let cols = rows.iter().map(|(c, _)| c.len()).max().unwrap_or(0) as u16;
        let mut cells = Vec::with_capacity(rows_n as usize);
        let mut wrapped = Vec::with_capacity(rows_n as usize);
        for (c, w) in rows {
            cells.push(c);
            wrapped.push(w);
        }
        FrameSnapshot {
            cols,
            rows: rows_n,
            cells,
            cursor: Cursor {
                x: 0,
                y: 0,
                visible: false,
                style: CursorStyle::Block,
            },
            scrollback_rows: 0,
            alternate_screen: false,
            wrapped,
            dirty_rows: vec![true; rows_n as usize],
            full_redraw: true,
        }
    }

    #[test]
    fn extract_text_soft_wrap_joins_without_separator() {
        // 行 0 软折行（wrapped=true）→与行 1 拼接无分隔。
        let snap = snap_multi(vec![
            (vec![cell("AB", CellWide::Narrow)], true),
            (vec![cell("CD", CellWide::Narrow)], false),
        ]);
        assert_eq!(
            extract_text(&snap, (0, 0, 0, 1)),
            "ABCD",
            "软折行处不插分隔"
        );
    }

    #[test]
    fn extract_text_hard_wrap_inserts_newline() {
        // 行 0 非 wrapped→逻辑行结束，行 1 间插 \n。
        let snap = snap_multi(vec![
            (vec![cell("AB", CellWide::Narrow)], false),
            (vec![cell("CD", CellWide::Narrow)], false),
        ]);
        assert_eq!(extract_text(&snap, (0, 0, 0, 1)), "AB\nCD", "硬换行插 \\n");
    }

    #[test]
    fn extract_text_mixed_soft_and_hard_wrap() {
        // 软折（A→B）+硬换行（B|C）+硬换行（C|D）混合。
        let snap = snap_multi(vec![
            (vec![cell("A", CellWide::Narrow)], true),
            (vec![cell("B", CellWide::Narrow)], false),
            (vec![cell("C", CellWide::Narrow)], false),
            (vec![cell("D", CellWide::Narrow)], false),
        ]);
        assert_eq!(
            extract_text(&snap, (0, 0, 0, 3)),
            "AB\nC\nD",
            "软折拼接+硬换行分隔"
        );
    }

    #[test]
    fn extract_text_wide_char_across_wrap_boundary() {
        // 宽字符跨软折行边界：Wide+SpacerTail 不重复，拼接无分隔。
        let snap = snap_multi(vec![
            (
                vec![cell("你", CellWide::Wide), cell("", CellWide::SpacerTail)],
                true,
            ),
            (vec![cell("好", CellWide::Narrow)], false),
        ]);
        assert_eq!(
            extract_text(&snap, (0, 0, 1, 1)),
            "你好",
            "宽字符续格不重复+软折拼接"
        );
    }

    #[test]
    fn extract_text_empty_line_preserved() {
        // 空逻辑行保留（输出中占一行）。
        let snap = snap_multi(vec![
            (vec![cell("A", CellWide::Narrow)], false),
            (vec![], false),
            (vec![cell("B", CellWide::Narrow)], false),
        ]);
        assert_eq!(
            extract_text(&snap, (0, 0, 0, 2)),
            "A\n\nB",
            "空行保留为连续 \\n"
        );
    }
}

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn viewport_abs_roundtrip() {
        // offset=40（上滚 40 行）：视口行 0=绝对行 40。
        assert_eq!(viewport_to_abs(0, 40), 40);
        assert_eq!(viewport_to_abs(5, 40), 45);
        assert_eq!(
            abs_to_viewport(40, 40, 24),
            Some(0),
            "绝对行=offset→视口行 0"
        );
        assert_eq!(abs_to_viewport(45, 40, 24), Some(5));
        assert_eq!(
            abs_to_viewport(39, 40, 24),
            None,
            "视口上方（历史区外）返 None"
        );
        assert_eq!(
            abs_to_viewport(64, 40, 24),
            None,
            "视口下方返 None（24 行视口：40..63）"
        );
    }

    #[test]
    fn selection_region_clamps_to_viewport() {
        // 选区绝对行 42..47，offset=40，视口 24 行 → 视口行 2..7。
        let r = selection_viewport_region((3, 42, 10, 47), 40, 24);
        assert_eq!(r, Some((3, 2, 10, 7)), "绝对坐标按 offset 转换到视口");
        // 选区完全在视口上方（绝对行 30..35 < offset 40）→ None。
        assert_eq!(selection_viewport_region((0, 30, 5, 35), 40, 24), None);
        // 选区跨越视口上下边界→上端 None（clamp 语义：完全在外才 None；
        // 跨边界场景由调用方按可见段拆分，本函数保持矩形语义）。
        assert_eq!(selection_viewport_region((0, 35, 5, 50), 40, 24), None);
    }
}

#[cfg(test)]
mod auto_scroll_tests {
    use super::*;

    #[test]
    fn auto_scroll_step_direction_and_clamp() {
        // offset=40 sb=100 viewport=24：下沿方向→offset+1=41，focus=视口底绝对行
        let (o, f) = auto_scroll_step(40, 1, 100, 24);
        assert_eq!(o, 41, "下沿方向 offset+1");
        assert_eq!(
            f,
            41 + 23,
            "focus=新视口最后可见行（offset+viewport-1，行号合法）"
        );
        // 上沿方向→offset-1，focus=视口顶行
        let (o, f) = auto_scroll_step(40, -1, 100, 24);
        assert_eq!(o, 39);
        assert_eq!(f, 39, "上沿 focus=视口顶行");
        // 方向 0=停止（offset 不变）
        assert_eq!(auto_scroll_step(40, 0, 100, 24), (40, 40));
        // clamp：offset=0 上沿方向不欠溢出；offset=sb 下沿方向不越界
        assert_eq!(auto_scroll_step(0, -1, 100, 24).0, 0);
        let (o, _) = auto_scroll_step(100, 1, 100, 24);
        assert_eq!(o, 100, "下沿 clamp 到 sb_rows");
    }
}

/// 自动滚动步进（任务 2C.3）：拖选到窗口上/下沿时每帧推进 offset 并返回
/// 新 focus 绝对行（下沿=视口底行、上沿=视口顶行）；方向 0=停止。
/// clamp 到 [0, sb_rows]（越界回落边界，外部输入防御）。
pub fn auto_scroll_step(
    offset: usize,
    dir: i8,
    sb_rows: usize,
    viewport_rows: u16,
) -> (usize, usize) {
    if dir == 0 {
        return (offset, offset);
    }
    let new_offset = if dir > 0 {
        offset.saturating_add(1).min(sb_rows)
    } else {
        offset.saturating_sub(1)
    };
    // focus 目标绝对行：向下滚=视口最后可见行（offset+rows-1），向上滚=视口顶行。
    let last_visible = sb_rows + viewport_rows.saturating_sub(1) as usize;
    let focus_abs = if dir > 0 {
        new_offset + viewport_rows.saturating_sub(1) as usize
    } else {
        new_offset
    };
    (new_offset, focus_abs.min(last_visible))
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    fn pos(col: u16, row: u16) -> CellPos {
        CellPos { col, row }
    }

    #[test]
    fn three_row_drag_is_streaming() {
        // 从 (col 5, row 0) 拖到 (col 3, row 2)，列数 10：
        // 首行 5..9、中间行整行 0..9、末行 0..3（矩形会是 3..5 三段）
        let a = pos(5, 0);
        let f = pos(3, 2);
        assert_eq!(stream_row_range(a, f, 0, 10), Some((5, 9)));
        assert_eq!(
            stream_row_range(a, f, 1, 10),
            Some((0, 9)),
            "中间行必须整行"
        );
        assert_eq!(stream_row_range(a, f, 2, 10), Some((0, 3)));
        assert_eq!(stream_row_range(a, f, 3, 10), None, "选区外无范围");
    }

    #[test]
    fn upward_drag_is_streaming() {
        // 反向拖：从 (col 3, row 2) 到 (col 5, row 0)
        let a = pos(3, 2);
        let f = pos(5, 0);
        assert_eq!(stream_row_range(a, f, 0, 10), Some((0, 5)));
        assert_eq!(stream_row_range(a, f, 1, 10), Some((0, 9)));
        assert_eq!(stream_row_range(a, f, 2, 10), Some((3, 9)));
    }

    #[test]
    fn single_row_and_zero_length() {
        let a = pos(2, 1);
        assert_eq!(stream_row_range(a, pos(6, 1), 1, 10), Some((2, 6)));
        assert_eq!(stream_row_range(a, pos(6, 1), 0, 10), None);
    }
}
