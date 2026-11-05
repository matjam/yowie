//! Collision of soft particles against the colliders of a rigid-body world.

use rapier2d::pipeline::{QueryFilter as Filter2, QueryFilterFlags as Flags2};
use rapier3d::pipeline::{QueryFilter as Filter3, QueryFilterFlags as Flags3};
use yowie_core::{Vec2, Vec3};

use crate::d2::World2D;
use crate::d3::World3D;
use crate::ids::CollisionLayers;

/// How far from a particle, beyond its radius, the world is searched for a
/// surface. A particle buried deeper than `radius + QUERY_REACH` inside a
/// collider is not pushed out.
pub const QUERY_REACH: f32 = 0.5;

/// A particle touching a collider surface in 3D.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact3 {
    /// Unit direction that moves the particle out of the collider.
    pub normal: Vec3,
    /// How far a sphere of the particle's radius penetrates the surface.
    pub depth: f32,
    /// Coulomb friction coefficient of the collider.
    pub friction: f32,
}

/// A particle touching a collider surface in 2D.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact2 {
    /// Unit direction that moves the particle out of the collider.
    pub normal: Vec2,
    /// How far a disc of the particle's radius penetrates the surface.
    pub depth: f32,
    /// Coulomb friction coefficient of the collider.
    pub friction: f32,
}

/// Something soft particles can collide with in 3D. Implemented by
/// [`World3D`] (static and kinematic colliders only) and by `()`, which has
/// nothing to collide with.
pub trait Colliders3: Sync {
    /// Returns the contact of a sphere at `p`, if it touches a collider that
    /// interacts with `layers`.
    fn contact(&self, p: Vec3, radius: f32, layers: CollisionLayers) -> Option<Contact3>;
}

/// Something soft particles can collide with in 2D. Implemented by
/// [`World2D`] (static and kinematic colliders only) and by `()`.
pub trait Colliders2: Sync {
    /// Returns the contact of a disc at `p`, if it touches a collider that
    /// interacts with `layers`.
    fn contact(&self, p: Vec2, radius: f32, layers: CollisionLayers) -> Option<Contact2>;
}

impl Colliders3 for () {
    fn contact(&self, _: Vec3, _: f32, _: CollisionLayers) -> Option<Contact3> {
        None
    }
}

impl Colliders2 for () {
    fn contact(&self, _: Vec2, _: f32, _: CollisionLayers) -> Option<Contact2> {
        None
    }
}

impl Colliders3 for World3D {
    fn contact(&self, p: Vec3, radius: f32, layers: CollisionLayers) -> Option<Contact3> {
        let filter = Filter3 {
            flags: Flags3::EXCLUDE_DYNAMIC | Flags3::EXCLUDE_SENSORS,
            groups: Some(crate::d3::groups_of(layers)),
            ..Filter3::default()
        };
        let point = rapier3d::math::Vector::new(p.x, p.y, p.z);
        let (h, proj) = self
            .pw
            .project_point(point, radius + QUERY_REACH, false, filter)?;
        let s = proj.point;
        let delta = Vec3::new(p.x - s.x, p.y - s.y, p.z - s.z);
        let len = delta.length();
        let (normal, signed) = if proj.is_inside {
            (-delta.try_normalize().unwrap_or(Vec3::Y), -len)
        } else {
            (delta.try_normalize().unwrap_or(Vec3::Y), len)
        };
        let depth = radius - signed;
        (depth > 0.0).then(|| Contact3 {
            normal,
            depth,
            friction: self.pw.colliders.get(h).map_or(0.5, |c| c.friction()),
        })
    }
}

impl Colliders2 for World2D {
    fn contact(&self, p: Vec2, radius: f32, layers: CollisionLayers) -> Option<Contact2> {
        let filter = Filter2 {
            flags: Flags2::EXCLUDE_DYNAMIC | Flags2::EXCLUDE_SENSORS,
            groups: Some(crate::d2::groups_of(layers)),
            ..Filter2::default()
        };
        let point = rapier2d::math::Vector::new(p.x, p.y);
        let (h, proj) = self
            .pw
            .project_point(point, radius + QUERY_REACH, false, filter)?;
        let s = proj.point;
        let delta = Vec2::new(p.x - s.x, p.y - s.y);
        let len = delta.length();
        let (normal, signed) = if proj.is_inside {
            (-delta.try_normalize().unwrap_or(Vec2::Y), -len)
        } else {
            (delta.try_normalize().unwrap_or(Vec2::Y), len)
        };
        let depth = radius - signed;
        (depth > 0.0).then(|| Contact2 {
            normal,
            depth,
            friction: self.pw.colliders.get(h).map_or(0.5, |c| c.friction()),
        })
    }
}

/// A horizontal floor for scenes with no collider under the particles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    /// Height of the floor plane along Y.
    pub y: f32,
    /// Coulomb friction coefficient of the floor.
    pub friction: f32,
}

impl Default for Ground {
    fn default() -> Self {
        Self {
            y: 0.0,
            friction: 0.5,
        }
    }
}

/// Moves `p` back along its tangential travel since `prev`, limited by
/// Coulomb friction for a contact of the given normal and penetration depth.
pub(crate) fn friction_slide3(p: &mut Vec3, prev: Vec3, normal: Vec3, depth: f32, mu: f32) {
    let travel = *p - prev;
    let tangent = travel - normal * travel.dot(normal);
    let len = tangent.length();
    if len > 1.0e-9 {
        *p -= tangent * (mu * depth / len).min(1.0);
    }
}

/// 2D version of [`friction_slide3`].
pub(crate) fn friction_slide2(p: &mut Vec2, prev: Vec2, normal: Vec2, depth: f32, mu: f32) {
    let travel = *p - prev;
    let tangent = travel - normal * travel.dot(normal);
    let len = tangent.length();
    if len > 1.0e-9 {
        *p -= tangent * (mu * depth / len).min(1.0);
    }
}
