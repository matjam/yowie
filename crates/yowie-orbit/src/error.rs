use thiserror::Error;

/// Errors from building a [`crate::System`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OrbitError {
    /// A body id does not exist in this system.
    #[error("no such body in this system")]
    UnknownBody,
    /// A ship id does not exist in this system.
    #[error("no such ship in this system")]
    UnknownShip,
}
