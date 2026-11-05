//! The crate error type.

use thiserror::Error;

/// Errors from building skeletons and importing animation data.
#[derive(Debug, Error)]
pub enum AnimError {
    /// A skeleton's joint list is malformed: a parent index that does not
    /// precede its child, a duplicate name or an out-of-range index.
    #[error("invalid skeleton: {0}")]
    InvalidSkeleton(String),
    /// A named joint or clip does not exist.
    #[error("unknown name: {0}")]
    UnknownName(String),
    /// A glTF file could not be read or parsed.
    #[cfg(feature = "gltf")]
    #[error("glTF error: {0}")]
    Gltf(#[from] gltf::Error),
    /// A glTF file is valid but uses data this crate cannot import.
    #[cfg(feature = "gltf")]
    #[error("unsupported glTF data: {0}")]
    Unsupported(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, AnimError>;
