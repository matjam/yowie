//! The error type for reading Tiled files.

use yowie_asset::BoxError;

/// Everything that can go wrong parsing or loading a Tiled map or tileset.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The JSON text is malformed.
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The XML text is malformed.
    #[error("invalid XML: {0}")]
    Xml(String),
    /// A required field is missing or has the wrong type. The text names
    /// where, for example `layer "ground": missing "data"`.
    #[error("invalid map: {0}")]
    Invalid(String),
    /// The file uses a feature this crate does not read, such as a layer
    /// encoding Tiled added after this crate was written.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Tile data could not be decoded (bad base64, a corrupt compressed
    /// stream, or a length that is not a whole number of cells).
    #[error("tile data: {0}")]
    Data(String),
    /// An external tileset is named but no resolver was given.
    #[error("external tileset {0:?} needs a resolver")]
    NoResolver(String),
    /// The resolver failed to fetch a file.
    #[error("resolving {path:?}: {source}")]
    Resolve {
        /// The path the file named, relative to the map's directory.
        path: String,
        /// The resolver's error.
        #[source]
        source: BoxError,
    },
    /// Reading through the virtual file system failed.
    #[error(transparent)]
    Asset(#[from] yowie_asset::Error),
}

/// A `Result` with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Builds an [`Error::Invalid`] from a message.
pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}
