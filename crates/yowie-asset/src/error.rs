//! The error type shared by every part of the crate.

use std::path::PathBuf;

/// A boxed error from a user-supplied loader.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Everything that can go wrong reading, packing, loading or watching assets.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No mount holds the asset path.
    #[error("asset not found: {0}")]
    NotFound(String),
    /// The path is empty, escapes the root with `..`, or is not valid UTF-8.
    #[error("invalid asset path: {0:?}")]
    InvalidPath(String),
    /// A file system operation failed.
    #[error("{path}: {source}", path = .path.display())]
    Io {
        /// The file the operation was on.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A pack file is malformed: bad magic, unsupported version, truncated,
    /// or an index that fails its checksum or points outside the file.
    #[error("corrupt pack: {0}")]
    CorruptPack(String),
    /// A pack entry's data does not match its stored checksum.
    #[error("pack entry {path} fails its checksum")]
    Checksum {
        /// The entry's asset path.
        path: String,
    },
    /// A pack writer was given a path twice.
    #[error("duplicate pack entry: {0}")]
    DuplicateEntry(String),
    /// No loader is registered for the requested asset type.
    #[error("no loader registered for {0}")]
    NoLoader(&'static str),
    /// A loader rejected the bytes of an asset.
    #[error("loading {path}: {source}")]
    Decode {
        /// The asset path.
        path: String,
        /// The loader's error.
        #[source]
        source: BoxError,
    },
    /// The asset manager has been shut down or the load was abandoned.
    #[error("asset manager is closed")]
    Closed,
    /// The worker thread pool could not be created.
    #[error("worker pool: {0}")]
    ThreadPool(String),
    /// The file watcher failed.
    #[error("file watcher: {0}")]
    Watch(#[from] notify::Error),
}

impl Error {
    /// Wraps an I/O error with the path it happened on.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Reports whether this is a [`Error::NotFound`] error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }
}

/// A `Result` with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
