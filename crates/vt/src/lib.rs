//! vt crate：唯一接触 libghostty-vt 的层。
//!
//! ## 线程安全
//! `VtTerminal` 内部持有 libghostty-vt 的 `Terminal`，后者要求单线程独占访问。
//! 因此 `VtTerminal` 不实现 `Send`/`Sync`，所有方法必须在同一线程调用。
//! PTY 读取在独立线程进行，经 channel 将字节交回主线程后由本类型消费。
pub mod encoder;
pub mod error;
pub mod snapshot;
pub mod terminal;

pub use encoder::{KeyEncoder, MouseEncoder};
pub use error::Error;
pub use libghostty_vt::terminal::ScrollViewport;
pub use snapshot::{Cell, CellWide, Cursor, CursorStyle, FrameSnapshot, Rgb, Underline};
pub use terminal::{VtPalette, VtTerminal};

// 转发 key/mouse 事件类型，供调用方构造编码事件（快照类型不暴露这些）。
pub use libghostty_vt::key::{Action, Key, Mods};
pub use libghostty_vt::mouse::{Action as MouseAction, Button, Position};
