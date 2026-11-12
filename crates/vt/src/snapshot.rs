//! 自有帧快照类型。
//!
//! 本模块任何类型不得出现 libghostty 类型，保证快照可被外部 crate
//!（render/app）独立持有，不借用 Terminal 生命周期。

/// RGB 颜色分量。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// 单元格宽度标记（含宽字符续格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CellWide {
    /// 非宽字符，格宽 1。
    #[default]
    Narrow,
    /// 宽字符，格宽 2。
    Wide,
    /// 宽字符后的续格标记，不渲染。
    SpacerTail,
    /// 软换行行尾宽字符的续格标记。
    SpacerHead,
}

/// 下划线样式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// 光标样式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    HollowBlock,
    Underline,
    Bar,
}

/// 单个网格单元。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cell {
    pub chars: String,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: Underline,
    pub wide: CellWide,
}

/// 光标状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub style: CursorStyle,
}

/// 一帧的完全自有拷贝，不借用 Terminal 生命周期。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Vec<Cell>>,
    pub cursor: Cursor,
    pub scrollback_rows: usize,
    pub alternate_screen: bool,
    /// 每行软折行标记（libghostty Row::is_wrapped）：true=行尾软折行，
    /// 内容续在下一行（提取文本时逻辑行拼接，不插分隔）。
    pub wrapped: Vec<bool>,
}
