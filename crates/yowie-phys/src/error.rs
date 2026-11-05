//! The error type shared by the rigid-body and soft-body APIs.

use crate::ids::{BodyId, ColliderId, JointId};

/// Errors returned by `yowie-phys`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PhysError {
    /// A shape description cannot be turned into a collider, for example a
    /// convex hull of fewer than three points or a mesh with no triangles.
    #[error("invalid shape: {0}")]
    InvalidShape(&'static str),
    /// The body id does not name a live body in this world.
    #[error("no such body: {0:?}")]
    NoSuchBody(BodyId),
    /// The collider id does not name a live collider in this world.
    #[error("no such collider: {0:?}")]
    NoSuchCollider(ColliderId),
    /// The joint id does not name a live joint in this world.
    #[error("no such joint: {0:?}")]
    NoSuchJoint(JointId),
    /// A numeric parameter is out of range, for example a negative radius.
    #[error("invalid parameter: {0}")]
    InvalidParameter(&'static str),
}

/// Result alias using [`PhysError`].
pub type Result<T> = std::result::Result<T, PhysError>;
