//! vt crate 错误枚举。

/// 本 crate 的错误类型。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// libghostty-vt 返回的错误。
    #[error("libghostty-vt error: {0}")]
    Vt(#[source] libghostty_vt::Error),
}

impl From<libghostty_vt::Error> for Error {
    fn from(e: libghostty_vt::Error) -> Self {
        Error::Vt(e)
    }
}
