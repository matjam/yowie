//! The crate's error type.

/// Errors from reading emitter definitions and creating GPU systems.
#[derive(Debug, thiserror::Error)]
pub enum ParticleError {
    /// The JSON was malformed or held a value of the wrong type.
    #[error("emitter JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The GPU cannot run the particle compute path; the message names
    /// what is missing. Use the CPU [`System`](crate::System) instead.
    #[error("GPU particles are not supported here: {0}")]
    Unsupported(String),
}

/// Results from this crate.
pub type Result<T, E = ParticleError> = std::result::Result<T, E>;
