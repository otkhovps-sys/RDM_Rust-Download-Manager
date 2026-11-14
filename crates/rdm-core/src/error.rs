use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("server responded with HTTP {0}")]
    Status(u16),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("connection closed before the data was complete")]
    Incomplete,
    #[error("{0}")]
    Other(String),
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    pub(crate) fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    /// Errors that are worth retrying (network hiccups, 5xx, rate limits).
    pub(crate) fn is_transient(&self) -> bool {
        match self {
            Self::Http(_) | Self::Incomplete => true,
            Self::Status(code) => *code == 408 || *code == 429 || *code >= 500,
            _ => false,
        }
    }
}
