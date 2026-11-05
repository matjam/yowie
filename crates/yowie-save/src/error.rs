use thiserror::Error;

/// Errors from reading and writing saves.
#[derive(Debug, Error)]
pub enum SaveError {
    /// The platform reports no data directory for this user.
    #[error("no data directory is available for this user")]
    NoDataDir,
    /// The slot name is empty, is `.` or `..`, or contains a path
    /// separator or a character Windows forbids in file names.
    #[error("{0:?} is not a valid save name")]
    InvalidName(String),
    /// No save exists with this name.
    #[error("save {0:?} does not exist")]
    NotFound(String),
    /// A file operation failed.
    #[error("save {name:?}: {source}")]
    Io {
        /// The slot involved, or the directory path when no slot applies.
        name: String,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The value could not be encoded, or the file is not valid JSON for
    /// the requested type.
    #[error("save {name:?}: {source}")]
    Json {
        /// The slot involved.
        name: String,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// The save was written by a newer version of the game than the one
    /// reading it.
    #[error("save {name:?} has version {found}, newer than the supported {supported}")]
    TooNew {
        /// The slot involved.
        name: String,
        /// The version recorded in the file.
        found: u32,
        /// The store's current version.
        supported: u32,
    },
    /// A migration returned an error.
    #[error("save {name:?}: migration from version {from} failed: {reason}")]
    Migration {
        /// The slot involved.
        name: String,
        /// The version being upgraded from.
        from: u32,
        /// The message the migration gave.
        reason: String,
    },
    /// The background writer stopped before reporting a result.
    #[error("the save worker thread is gone")]
    WorkerGone,
}
