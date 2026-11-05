//! 2D rigid bodies on rapier.

mod body;
mod character;
mod collider;
mod convert;
mod debug;
mod desc;
mod joint;
mod query;
mod shape;
mod world;

#[cfg(test)]
mod tests;

pub use character::{CharacterController2D, CharacterMove2D};
pub use desc::{BodyDesc2D, ColliderDesc2D};
pub use joint::{JointDesc2D, JointKind2D};
pub use query::signed_distance_2d;
pub use shape::Shape2D;
pub use world::World2D;
