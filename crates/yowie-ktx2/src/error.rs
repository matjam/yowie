use thiserror::Error;

/// The error type of this crate.
#[derive(Debug, Error)]
pub enum Error {
    /// A file could not be read.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A KTX2 file is malformed or uses a feature this crate does not read.
    #[error("ktx2: {0}")]
    Ktx2(String),
    /// An argument is not usable, with the reason.
    #[error("{0}")]
    Invalid(String),
}

/// A result whose error is [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
