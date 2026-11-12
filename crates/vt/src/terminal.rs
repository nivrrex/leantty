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
        let mut wrapped = Vec::new();
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
            // 软折行标记：render state RowData 无 WRAP 变体，走 grid_ref(Viewport)
            // 逐行查 screen::Row::is_wrapped（GridRef 借 terminal，与迭代器借
            // 并存均为不可变借，编译期安全）。
            let coord = libghostty_vt::terminal::PointCoordinate { x: 0, y: cells.len() as u32 - 1 };
            let is_wrapped = self
                .inner
                .grid_ref(libghostty_vt::terminal::Point::Viewport(coord))
                .and_then(|g| g.row())
                .and_then(|r| r.is_wrapped())
                .unwrap_or(false);
            wrapped.push(is_wrapped);
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
            wrapped,
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

    /// 括号粘贴模式（?2004）当前状态（粘贴规范化包裹用）。
    pub fn is_bracketed_paste(&self) -> Result<bool, Error> {
        Ok(self
            .inner
            .mode(libghostty_vt::terminal::Mode::BRACKETED_PASTE)?)
    }

    /// 鼠标上报模式当前状态（X10/1000/1002/1003 任一开启）。
    pub fn is_mouse_tracking(&self) -> Result<bool, Error> {
        Ok(self.inner.is_mouse_tracking()?)
    }

    /// 滚动视口（scrollback/备用屏联动；上滚偏移由调用方维护）。
    pub fn scroll_viewport(&mut self, scroll: libghostty_vt::terminal::ScrollViewport) {
        self.inner.scroll_viewport(scroll);
    }

    /// 终端标题（OSC 0/2 设置；每次查询返回当前值，无标题返 None）。
    ///
    /// 任务 2C.4：window.dynamic_title 据此更新窗口标题。
    pub fn title(&self) -> Result<Option<String>, Error> {
        let bytes = self.inner.title()?;
        if bytes.is_empty() {
            return Ok(None);
        }
        Ok(std::str::from_utf8(bytes).ok().map(|s| s.to_string()))
    }

    /// 读内容绝对行范围文本（任务 2C.2：选区跨视口外历史行，只读不改变视口）。
    ///
    /// 坐标：Point::Screen（full incl scrollback），y=scrollback 顶起绝对行
    /// （与 Selection 绝对行模型一致）。返回 (行文本, 上行是否软折行)。
    /// 代价：逐 cell grid_ref+graphemes（2 FFI/cell，cols×rows 次——百行级
    /// 毫秒级；DECISIONS 登记评估结论）。视口不变（grid_ref 不移动视口）。
    pub fn rows_text(
        &self,
        abs_start: usize,
        abs_end: usize,
        cols: u16,
    ) -> Result<Vec<(Vec<Cell>, bool)>, Error> {
        let mut out = Vec::with_capacity(abs_end - abs_start + 1);
        for abs in abs_start..=abs_end {
            let coord = libghostty_vt::terminal::PointCoordinate { x: 0, y: abs as u32 };
            let grid = self
                .inner
                .grid_ref(libghostty_vt::terminal::Point::Screen(coord))?;
            let wrapped = grid.row()?.is_wrapped().unwrap_or(false);
            let mut line = Vec::with_capacity(cols as usize);
            for col in 0..cols {
                let coord = libghostty_vt::terminal::PointCoordinate { x: col, y: abs as u32 };
                let cell_grid = self
                    .inner
                    .grid_ref(libghostty_vt::terminal::Point::Screen(coord))?;
                let mut buf = ['\0'; 8];
                let n = cell_grid.graphemes(&mut buf)?;
                let chars: String = buf[..n].iter().collect();
                let wide = match cell_grid.cell()?.wide()? {
                    libghostty_vt::screen::CellWide::Narrow => CellWide::Narrow,
                    libghostty_vt::screen::CellWide::Wide => CellWide::Wide,
                    libghostty_vt::screen::CellWide::SpacerTail => CellWide::SpacerTail,
                    libghostty_vt::screen::CellWide::SpacerHead => CellWide::SpacerHead,
                };
                line.push(Cell {
                    chars,
                    wide,
                    ..Default::default()
                });
            }
            out.push((line, wrapped));
        }
        Ok(out)
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
