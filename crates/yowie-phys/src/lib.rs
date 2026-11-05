//! 2D and 3D rigid bodies with joints, sleeping, continuous collision,
//! character controllers and queries; cloth, volumetric soft bodies and 2D
//! fluids in [`soft`] on the same colliders.
//!
//! Rigid bodies run on rapier behind [`World2D`] and [`World3D`]. Positions
//! and rotations use `yowie-core` types; rapier types do not appear in the
//! public API. Soft bodies are Yowie's own extended position-based dynamics
//! code and collide with the static and kinematic colliders of a world.
//!
//! To simulate, create a world, add bodies and colliders, and call `step`
//! with a fixed time step:
//!
//! ```
//! use yowie_phys::{BodyDesc3D, ColliderDesc3D, Shape3D, World3D};
//! use yowie_core::Vec3;
//!
//! let mut world = World3D::default();
//! let ground = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(10.0, 0.5, 10.0)));
//! world.add_static_collider(&ground).unwrap();
//! let ball = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, 3.0, 0.0)));
//! world
//!     .add_collider(ball, &ColliderDesc3D::new(Shape3D::sphere(0.5)))
//!     .unwrap();
//! for _ in 0..120 {
//!     world.step(1.0 / 60.0);
//! }
//! assert!(world.position(ball).unwrap().y < 3.0);
//! ```

#![warn(missing_docs)]

pub mod d2;
pub mod d3;
pub mod soft;

mod debug;
mod error;
mod event;
#[cfg(feature = "gfx")]
mod gfx_debug;
#[cfg(feature = "gfx")]
mod gfx_soft;
mod ids;
mod joint;
mod stepper;

pub use d2::{
    BodyDesc2D, CharacterController2D, CharacterMove2D, ColliderDesc2D, JointDesc2D, JointKind2D,
    Shape2D, World2D, signed_distance_2d,
};
pub use d3::{
    BodyDesc3D, CharacterController3D, CharacterMove3D, ColliderDesc3D, JointDesc3D, JointKind3D,
    RAGDOLL_PARTS, Ragdoll3D, RagdollBone, RagdollSpec, Shape3D, World3D, signed_distance_3d,
};
pub use debug::DebugDraw;
pub use error::{PhysError, Result};
pub use event::{Event, RayHit, ShapeHit, SurfaceDistance};
#[cfg(feature = "gfx")]
pub use gfx_debug::GfxDebugDraw;
pub use ids::{BodyId, BodyKind, ColliderId, CollisionLayers, CombineRule, JointId, QueryFilter};
pub use joint::{Limits, Motor};
pub use stepper::FixedStepper;
