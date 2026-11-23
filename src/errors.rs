//! Error type shared by the whole application. Every error that reaches the report names the
//! affected file (added by the pipeline) and a human readable reason.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error on {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    /// Valid input that this program deliberately does not convert (animation, CMYK, ...).
    #[error("{0}")]
    Unsupported(String),
    /// The requested lossless operation cannot be performed without loss.
    #[error("{0}")]
    LossyRequired(String),
    #[error("{engine}: {message}")]
    Codec { engine: &'static str, message: String },
    #[error("verification failed: {0}")]
    Verification(String),
    #[error("{0}")]
    Limit(String),
    #[error("{0}")]
    Invalid(String),
    #[error("operation cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
    pub fn codec(engine: &'static str, message: impl Into<String>) -> Self {
        Error::Codec { engine, message: message.into() }
    }
}
