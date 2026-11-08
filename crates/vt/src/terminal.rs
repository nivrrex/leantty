//! libghostty-vt Terminal 的封装。
//!
//! ## 线程安全
//! `Terminal` 仅允许单个线程访问：`VtTerminal` 不实现 `Send`/`Sync`，
//! 所有方法必须在同一线程调用。PTY 读取在独立线程进行，
//! 经 channel 将字节交回主线程后由本类型消费。

use libghostty_vt::render::{CellIterator, RowIterator, RenderState};
use libghostty_vt::screen::{CellWide as VtCellWide, Screen};
use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::Terminal;

use crate::encoder::{KeyEncoder, MouseEncoder};
use crate::error::Error;
use crate::snapshot::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline};

/// 单线程独占的终端封装。
pub struct VtTerminal {
    inner: Terminal<'static, 'static>,
}

impl VtTerminal {
    /// 新建 cols x rows 网格的终端。
    pub fn new(cols: u16, rows: u16) -> Result<Self, Error> {
        Ok(Self {
            inner: Terminal::new(cols, rows)?,
        })
    }

    /// 调整网格大小。
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        self.inner.resize(cols, rows, 0, 0)?;
        Ok(())
    }

    /// 向终端输入字节。
    pub fn feed(&mut self, bytes: &[u8]) {
        self.inner.vt_write(bytes);
    }

    /// 注册终端响应字节回调（查询应答等写回 PTY 的数据）。
    ///
    /// 回调在 `feed`/`resize` 等调用内部同步触发；回调内不得再次调用
    /// 本类型方法（libghostty-vt 的可变引用不变量）。
    pub fn on_pty_write<F>(&mut self, f: F) -> Result<(), Error>
    where
        F: FnMut(&Terminal<'_, '_>, &[u8]) + 'static,
    {
        self.inner.on_pty_write(f)?;
        Ok(())
    }

    /// 拷贝当前帧为自有快照。
    pub fn snapshot(&self) -> Result<FrameSnapshot, Error> {
        let mut render_state = RenderState::new()?;
        let mut row_iter_factory = RowIterator::new()?;
        let mut cell_iter_factory = CellIterator::new()?;
        let snap = render_state.update(&self.inner)?;
        let mut row_iter = row_iter_factory.update(&snap)?;
        let mut cells = Vec::new();
        while let Some(row) = row_iter.next() {
            let mut cell_iter = cell_iter_factory.update(row)?;
            let mut line = Vec::new();
            while let Some(cell) = cell_iter.next() {
                let chars: String = cell.graphemes()?.into_iter().collect();
                let fg = cell.fg_color()?.map(vt_rgb_to_rgb);
                let bg = cell.bg_color()?.map(vt_rgb_to_rgb);
                let style = cell.style()?;
                let wide = match cell.raw_cell()?.wide()? {
                    VtCellWide::Narrow => CellWide::Narrow,
                    VtCellWide::Wide => CellWide::Wide,
                    VtCellWide::SpacerTail => CellWide::SpacerTail,
                    VtCellWide::SpacerHead => CellWide::SpacerHead,
                };
                line.push(Cell {
                    chars,
                    fg,
                    bg,
                    bold: style.bold,
                    italic: style.italic,
                    faint: style.faint,
                    blink: style.blink,
                    inverse: style.inverse,
                    invisible: style.invisible,
                    strikethrough: style.strikethrough,
                    overline: style.overline,
                    underline: map_underline(style.underline),
                    wide,
                });
            }
            cells.push(line);
        }
        Ok(FrameSnapshot {
            cols: self.inner.cols()?,
            rows: self.inner.rows()?,
            cells,
            cursor: Cursor {
                x: self.inner.cursor_x()?,
                y: self.inner.cursor_y()?,
                visible: snap.cursor_visible()?,
                style: map_cursor_style(snap.cursor_visual_style()?),
            },
            scrollback_rows: self.inner.scrollback_rows()?,
            alternate_screen: self.inner.active_screen()? == Screen::Alternate,
        })
    }

    /// 新建键盘编码器（选项取自当前终端模式）。
    pub fn key_encoder(&self) -> Result<KeyEncoder, Error> {
        let mut encoder = KeyEncoder::new()?;
        encoder.set_options_from_terminal(self)?;
        Ok(encoder)
    }

    /// 新建鼠标编码器（选项取自当前终端模式）。
    pub fn mouse_encoder(&self) -> Result<MouseEncoder, Error> {
        let mut encoder = MouseEncoder::new()?;
        encoder.set_options_from_terminal(self)?;
        Ok(encoder)
    }

    /// 暴露内部引用，供 encoder 的 set_options_from_terminal 使用。
    pub(crate) fn inner(&self) -> &Terminal<'static, 'static> {
        &self.inner
    }
}

fn vt_rgb_to_rgb(c: RgbColor) -> Rgb {
    Rgb { r: c.r, g: c.g, b: c.b }
}

fn map_underline(u: libghostty_vt::style::Underline) -> Underline {
    match u {
        libghostty_vt::style::Underline::None => Underline::None,
        libghostty_vt::style::Underline::Single => Underline::Single,
        libghostty_vt::style::Underline::Double => Underline::Double,
        libghostty_vt::style::Underline::Curly => Underline::Curly,
        libghostty_vt::style::Underline::Dotted => Underline::Dotted,
        libghostty_vt::style::Underline::Dashed => Underline::Dashed,
        // non_exhaustive：上游新增变体时回落 None。
        _ => Underline::None,
    }
}

fn map_cursor_style(s: libghostty_vt::render::CursorVisualStyle) -> CursorStyle {
    match s {
        libghostty_vt::render::CursorVisualStyle::Bar => CursorStyle::Bar,
        libghostty_vt::render::CursorVisualStyle::Block => CursorStyle::Block,
        libghostty_vt::render::CursorVisualStyle::Underline => CursorStyle::Underline,
        libghostty_vt::render::CursorVisualStyle::BlockHollow => CursorStyle::HollowBlock,
        // non_exhaustive：上游新增变体时回落 Block。
        _ => CursorStyle::Block,
    }
}
