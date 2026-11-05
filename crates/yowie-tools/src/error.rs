use thiserror::Error;

/// The error type of every operation in this crate.
#[derive(Debug, Error)]
pub enum Error {
    /// A file or directory could not be read, written or copied.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// An image could not be decoded or encoded.
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
    /// A KTX2 file is malformed or uses a feature this crate does not read.
    #[error("ktx2: {0}")]
    Ktx2(String),
    /// An argument or input is not usable, with the reason.
    #[error("{0}")]
    Invalid(String),
    /// The audio library failed.
    #[error("audio: {0}")]
    Audio(#[from] yowie_audio::Error),
}

impl From<yowie_ktx2::Error> for Error {
    fn from(e: yowie_ktx2::Error) -> Self {
        match e {
            yowie_ktx2::Error::Io(e) => Error::Io(e),
            yowie_ktx2::Error::Ktx2(s) => Error::Ktx2(s),
            yowie_ktx2::Error::Invalid(s) => Error::Invalid(s),
        }
    }
}

/// A result whose error is [`Error`](enum@Error).
pub type Result<T> = std::result::Result<T, Error>;
