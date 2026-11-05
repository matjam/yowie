//! The crate's error type.

/// Errors from reading emitter definitions.
#[derive(Debug, thiserror::Error)]
pub enum ParticleError {
    /// The JSON was malformed or held a value of the wrong type.
    #[error("emitter JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Results from this crate.
pub type Result<T, E = ParticleError> = std::result::Result<T, E>;
