//! IME 预编辑（Preedit）叠加层：布局纯函数 + 叠加层输入类型。
//!
//! 设计约束（任务 2C.3）：
//! - Preedit 不写入终端网格（FrameSnapshot 不含其内容），仅作为渲染叠加层；
//! - 布局为纯函数，便于单测；渲染层与 app（IME 候选窗定位）共用；
//! - 不引入新依赖：宽字符判定用内置区间表（覆盖 CJK/全角/emoji 主要区段）。

/// IME 预编辑叠加层输入（owned，不含后端类型）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreeditOverlay {
    /// 组合中文本。
    pub text: String,
    /// 组合内光标字节范围 (begin, end)；None=不标示。
    pub cursor_range: Option<(usize, usize)>,
}

impl PreeditOverlay {
    pub fn new(text: impl Into<String>, cursor_range: Option<(usize, usize)>) -> Self {
        Self {
            text: text.into(),
            cursor_range,
        }
    }
}

/// 单个预编辑单元：字符 + 占用格宽（1 或 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreeditCell {
    pub ch: char,
    pub width: u8,
}

/// 预编辑布局结果（相对网格的起点与单元序列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreeditLayout {
    /// 起始行（0-based 视口行）。
    pub row: u16,
    /// 起始列。
    pub col: u16,
    /// 单元序列（可能因超宽被截断到可见范围）。
    pub cells: Vec<PreeditCell>,
    /// 是否因总宽超过一整行而截断（DECISIONS 登记：截断而非折行拆分）。
    pub truncated: bool,
}

/// 宽字符判定（East Asian Wide/Fullwidth 主要区段；不引入 unicode-width 依赖）。
pub fn is_wide_char(ch: char) -> bool {
    let c = ch as u32;
    matches!(c,
        0x1100..=0x115F        // Hangul Jamo 初声
        | 0x2E80..=0x303E      // CJK 部首扩展、符号
        | 0x3041..=0x33FF      // 假名、CJK 符号
        | 0x3400..=0x4DBF      // CJK 扩展 A
        | 0x4E00..=0x9FFF      // CJK 基本区
        | 0xA000..=0xA4CF      // 彝文
        | 0xA960..=0xA97F      // Hangul Jamo 扩展 A
        | 0xAC00..=0xD7A3      // Hangul 音节
        | 0xF900..=0xFAFF      // CJK 兼容
        | 0xFE10..=0xFE19      // 竖排标点
        | 0xFE30..=0xFE6F      // CJK 兼容形式
        | 0xFF00..=0xFF60      // 全角形式
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F    // 表情符号
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD    // CJK 扩展 B+
    )
}

/// 布局计算（纯函数）：
/// - 起点=终端光标格（cursor_x, cursor_y）；
/// - 超出右边界时整体换到下一行行首（不拆分宽字符）；
/// - 换行后仍超出或行数越界时，起始位置平移使其可见；
/// - 总宽超过一整行时截断显示（truncated=true）。
pub fn layout_preedit(
    text: &str,
    cursor_x: u16,
    cursor_y: u16,
    cols: u16,
    rows: u16,
) -> Option<PreeditLayout> {
    if text.is_empty() || cols == 0 || rows == 0 {
        return None;
    }
    let mut cells: Vec<PreeditCell> = Vec::new();
    for ch in text.chars() {
        let width = if is_wide_char(ch) { 2 } else { 1 };
        cells.push(PreeditCell { ch, width });
    }
    let total: u32 = cells.iter().map(|c| u32::from(c.width)).sum();
    let cols32 = u32::from(cols);
    if total == 0 {
        return None;
    }
    // 超右边界：整体换到下一行行首。
    let (mut row, mut col) = (cursor_y, cursor_x);
    if u32::from(col) + total > cols32 {
        row = row.saturating_add(1);
        col = 0;
    }
    let truncated = total > cols32;
    // 行越界或截断：平移使起点可见（贴底时从右往左对齐）。
    if row >= rows {
        row = rows - 1;
        col = cols.saturating_sub(total.min(cols32) as u16);
    }
    // 截断到可见范围（cols - col）。
    if truncated || u32::from(col) + total > cols32 {
        let visible = cols32 - u32::from(col);
        let mut acc = 0u32;
        cells.retain(|c| {
            if acc + u32::from(c.width) <= visible {
                acc += u32::from(c.width);
                true
            } else {
                false
            }
        });
    }
    if cells.is_empty() {
        return None;
    }
    Some(PreeditLayout {
        row,
        col,
        cells,
        truncated,
    })
}

/// 预编辑末尾后的格坐标（IME 候选窗定位用）：返回 (row, col_after_last)。
pub fn layout_end_position(layout: &PreeditLayout, cols: u16) -> (u16, u16) {
    let width: u32 = layout.cells.iter().map(|c| u32::from(c.width)).sum();
    let end = u32::from(layout.col) + width;
    if end >= u32::from(cols) {
        (layout.row, cols.saturating_sub(1))
    } else {
        (layout.row, end as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_gives_no_layout() {
        assert_eq!(layout_preedit("", 0, 0, 80, 24), None);
        assert_eq!(layout_preedit("abc", 0, 0, 0, 24), None, "cols=0 防御");
        assert_eq!(layout_preedit("abc", 0, 0, 80, 0), None, "rows=0 防御");
    }

    #[test]
    fn ascii_layout_at_cursor() {
        let l = layout_preedit("abc", 5, 3, 80, 24).expect("layout");
        assert_eq!((l.row, l.col), (3, 5));
        assert_eq!(l.cells.len(), 3);
        assert!(!l.truncated);
        assert_eq!(
            l.cells.iter().map(|c| c.width).sum::<u8>(),
            3,
            "ASCII 每字 1 格"
        );
    }

    #[test]
    fn wide_char_takes_two_cells() {
        let l = layout_preedit("中文", 0, 0, 80, 24).expect("layout");
        assert_eq!(l.cells.iter().map(|c| u32::from(c.width)).sum::<u32>(), 4);
        assert!(is_wide_char('中') && is_wide_char('文'));
        assert!(!is_wide_char('a'));
    }

    #[test]
    fn overflow_wraps_to_next_line() {
        // 光标在倒数第 2 列，3 个 ASCII → 超出 → 整体换到下一行行首。
        let l = layout_preedit("abc", 78, 0, 80, 24).expect("layout");
        assert_eq!((l.row, l.col), (1, 0), "超出右边界整体换到下一行");
        assert_eq!(l.cells.len(), 3);
    }

    #[test]
    fn overflow_at_bottom_shifts_visible() {
        // 最后一行再换行（越界）→ 平移到最后一行并右对齐。
        let l = layout_preedit("abcd", 78, 23, 80, 24).expect("layout");
        assert_eq!(l.row, 23, "贴底不越界");
        assert_eq!(l.col, 76, "右对齐可见（80-4）");
        assert_eq!(l.cells.len(), 4);
    }

    #[test]
    fn wider_than_line_is_truncated() {
        let l = layout_preedit("abcdefghij", 0, 0, 4, 24).expect("layout");
        assert!(l.truncated, "总宽超一整行→截断");
        assert!(l.cells.len() <= 4, "截断到可见列数");
    }

    #[test]
    fn end_position_after_preedit() {
        let l = layout_preedit("abc", 5, 3, 80, 24).expect("layout");
        assert_eq!(layout_end_position(&l, 80), (3, 8), "末尾=起始列+宽度");
        // 宽字符：2 个汉字占 4 格
        let l = layout_preedit("中文", 0, 0, 80, 24).expect("layout");
        assert_eq!(layout_end_position(&l, 80), (0, 4));
        // 贴右边界：末尾 clamp 到最后一列
        let l = layout_preedit("abcd", 78, 23, 80, 24).expect("layout");
        assert_eq!(layout_end_position(&l, 80), (23, 79));
    }
}
