//! 选区模型（任务 2.1，自有：libghostty-vt 无 Selection API，DECISIONS 登记）。
//!
//! 选区以网格坐标存储；滚动偏移由 app 层维护（随 scroll_viewport Delta
//! 联动，任务 2B.4 接线）。宽字符整格选中，不选中 SpacerTail 半格。

use vt::{CellWide, FrameSnapshot};

/// 网格坐标（列，行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub col: u16,
    pub row: u16,
}

/// 选区（anchor 按下点，focus 当前点；Shift+点击扩展时 anchor 不动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: CellPos,
    pub focus: CellPos,
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

/// 选区文本提取（任务 2C.1）：软折行处拼接不插分隔（snap.wrapped 标记）；
/// 硬换行（上行非 wrapped）插 \n；行尾空白去除只作用于每个逻辑行末尾；
/// 宽字符续格（SpacerTail/SpacerHead）不重复；空行保留。
pub fn extract_text(snap: &FrameSnapshot, region: (u16, u16, u16, u16)) -> String {
    let (min_col, min_row, max_col, max_row) = region;
    let mut out = String::new();
    let mut logical = String::new();
    for row in min_row..=max_row {
        let Some(cells) = snap.cells.get(row as usize) else { continue };
        let mut seg = String::new();
        for col in min_col..=max_col {
            let Some(cell) = cells.get(col as usize) else { continue };
            if cell.wide == CellWide::SpacerTail || cell.wide == CellWide::SpacerHead {
                continue;
            }
            seg.push_str(&cell.chars);
        }
        logical.push_str(&seg);
        // 上行非软折行→逻辑行结束（硬换行边界）。
        let wrapped = snap.wrapped.get(row as usize).copied().unwrap_or(false);
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
    }
}

#[test]
fn extract_text_soft_wrap_joins_without_separator() {
    // 行 0 软折行（wrapped=true）→与行 1 拼接无分隔。
    let snap = snap_multi(vec![
        (vec![cell("AB", CellWide::Narrow)], true),
        (vec![cell("CD", CellWide::Narrow)], false),
    ]);
    assert_eq!(extract_text(&snap, (0, 0, 0, 1)), "ABCD", "软折行处不插分隔");
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
    assert_eq!(extract_text(&snap, (0, 0, 0, 3)), "AB\nC\nD", "软折拼接+硬换行分隔");
}

#[test]
fn extract_text_wide_char_across_wrap_boundary() {
    // 宽字符跨软折行边界：Wide+SpacerTail 不重复，拼接无分隔。
    let snap = snap_multi(vec![
        (vec![cell("你", CellWide::Wide), cell("", CellWide::SpacerTail)], true),
        (vec![cell("好", CellWide::Narrow)], false),
    ]);
    assert_eq!(extract_text(&snap, (0, 0, 1, 1)), "你好", "宽字符续格不重复+软折拼接");
}

#[test]
fn extract_text_empty_line_preserved() {
    // 空逻辑行保留（输出中占一行）。
    let snap = snap_multi(vec![
        (vec![cell("A", CellWide::Narrow)], false),
        (vec![], false),
        (vec![cell("B", CellWide::Narrow)], false),
    ]);
    assert_eq!(extract_text(&snap, (0, 0, 0, 2)), "A\n\nB", "空行保留为连续 \\n");
}
}
