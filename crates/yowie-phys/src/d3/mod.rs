//! 3D rigid bodies on rapier.

mod body;
mod character;
mod convert;
mod debug;
mod desc;
mod joint;
mod query;
mod ragdoll;
mod shape;
mod world;

#[cfg(test)]
mod tests;

pub use character::{CharacterController3D, CharacterMove3D};
pub use desc::{BodyDesc3D, ColliderDesc3D};
pub use joint::{JointDesc3D, JointKind3D};
pub use query::signed_distance_3d;
pub use ragdoll::{RAGDOLL_PARTS, Ragdoll3D, RagdollBone, RagdollSpec};
pub use shape::Shape3D;
pub use world::World3D;
