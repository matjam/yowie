//! Types every Yowie crate shares: vectors and matrices (from [`glam`]),
//! [`Color`], [`Rect`], and the 2D and 3D [`Transform2D`] and [`Transform`].
//!
//! Every type here implements `serde::Serialize` and `Deserialize`, so
//! components built from them can be saved by `yowie-ecs` and sent by
//! `yowie-net` without wrappers.

pub mod color;
pub mod rect;
pub mod transform;

pub use color::Color;
pub use rect::Rect;
pub use transform::{Transform, Transform2D};

pub use glam;
pub use glam::{
    Affine2, Affine3A, EulerRot, IVec2, IVec3, Mat2, Mat3, Mat4, Quat, UVec2, UVec3, Vec2, Vec3,
    Vec3A, Vec4, ivec2, ivec3, uvec2, uvec3, vec2, vec3, vec4,
};
