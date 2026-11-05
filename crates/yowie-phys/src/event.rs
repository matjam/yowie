//! Events produced by a world step, and the result types of queries.

use crate::ids::{BodyId, ColliderId};

/// Something that happened during the last [`step`](crate::World3D::step).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// Two solid colliders began touching.
    ContactStarted {
        /// First collider of the pair.
        a: ColliderId,
        /// Second collider of the pair.
        b: ColliderId,
    },
    /// Two solid colliders stopped touching.
    ContactStopped {
        /// First collider of the pair.
        a: ColliderId,
        /// Second collider of the pair.
        b: ColliderId,
    },
    /// A collider started overlapping a sensor.
    SensorEntered {
        /// The sensor collider.
        sensor: ColliderId,
        /// The collider that entered it.
        other: ColliderId,
    },
    /// A collider stopped overlapping a sensor, or was removed.
    SensorExited {
        /// The sensor collider.
        sensor: ColliderId,
        /// The collider that left it.
        other: ColliderId,
    },
    /// Two colliders pushed on each other during the step. Reported only for
    /// colliders created with `contact_events` enabled.
    ContactImpulse {
        /// First collider of the pair.
        a: ColliderId,
        /// Second collider of the pair.
        b: ColliderId,
        /// Sum of the magnitudes of the contact impulses, in newton-seconds.
        total: f32,
        /// The largest single contact impulse, in newton-seconds.
        max: f32,
    },
}

/// A ray hit. `V` is `Vec2` in 2D and `Vec3` in 3D.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit<V> {
    /// The collider that was hit.
    pub collider: ColliderId,
    /// The body that owns the collider, if it has one.
    pub body: Option<BodyId>,
    /// World-space hit point.
    pub point: V,
    /// Surface normal at the hit point, pointing away from the surface.
    pub normal: V,
    /// Distance from the ray origin to the hit point.
    pub distance: f32,
}

/// The first contact of a swept shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeHit<V> {
    /// The collider that was hit.
    pub collider: ColliderId,
    /// The body that owns the collider, if it has one.
    pub body: Option<BodyId>,
    /// World-space contact point on the hit collider.
    pub point: V,
    /// Surface normal of the hit collider at the contact point.
    pub normal: V,
    /// How far the shape travelled before touching, in world units.
    pub distance: f32,
}

/// Signed distance from a point to a placed shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceDistance<V> {
    /// Distance to the surface: positive outside, negative inside.
    pub distance: f32,
    /// The closest point on the surface.
    pub point: V,
    /// Unit direction of steepest increase of `distance`: away from the
    /// surface outside the shape, toward the surface from inside.
    pub normal: V,
}
