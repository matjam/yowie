//! Descriptions of 3D bodies and colliders, consumed by [`World3D`](super::World3D).

use yowie_core::{Quat, Transform, Vec3};

use super::shape::Shape3D;
use crate::ids::{BodyKind, CollisionLayers, CombineRule};

/// Everything needed to create a 3D body.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyDesc3D {
    /// How the body is simulated.
    pub kind: BodyKind,
    /// Initial world position.
    pub position: Vec3,
    /// Initial world rotation.
    pub rotation: Quat,
    /// Initial linear velocity in units per second.
    pub linear_velocity: Vec3,
    /// Initial angular velocity in radians per second, as an axis scaled by
    /// the rate.
    pub angular_velocity: Vec3,
    /// Per-second fraction of linear velocity lost to drag.
    pub linear_damping: f32,
    /// Per-second fraction of angular velocity lost to drag.
    pub angular_damping: f32,
    /// Multiplier on world gravity for this body.
    pub gravity_scale: f32,
    /// Whether the body may fall asleep when it stops moving.
    pub can_sleep: bool,
    /// Enables continuous collision detection so fast bodies do not tunnel.
    pub ccd: bool,
    /// Prevents the body from rotating.
    pub lock_rotation: bool,
}

impl BodyDesc3D {
    fn new(kind: BodyKind) -> Self {
        Self {
            kind,
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 1.0,
            can_sleep: true,
            ccd: false,
            lock_rotation: false,
        }
    }

    /// A body moved by forces and contacts.
    pub fn dynamic() -> Self {
        Self::new(BodyKind::Dynamic)
    }

    /// A body moved by the caller that pushes dynamic bodies.
    pub fn kinematic() -> Self {
        Self::new(BodyKind::Kinematic)
    }

    /// A body that never moves.
    pub fn fixed() -> Self {
        Self::new(BodyKind::Static)
    }

    /// Sets the initial position.
    pub fn at(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    /// Sets the initial rotation.
    pub fn rotated(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    /// Sets the initial linear velocity.
    pub fn moving(mut self, velocity: Vec3) -> Self {
        self.linear_velocity = velocity;
        self
    }

    /// Sets both damping factors.
    pub fn damped(mut self, linear: f32, angular: f32) -> Self {
        self.linear_damping = linear;
        self.angular_damping = angular;
        self
    }

    /// Turns continuous collision detection on.
    pub fn with_ccd(mut self) -> Self {
        self.ccd = true;
        self
    }

    /// Stops the body from rotating.
    pub fn without_rotation(mut self) -> Self {
        self.lock_rotation = true;
        self
    }
}

/// Everything needed to create a 3D collider.
#[derive(Debug, Clone, PartialEq)]
pub struct ColliderDesc3D {
    /// The collision shape.
    pub shape: Shape3D,
    /// Placement relative to the owning body.
    pub offset: Transform,
    /// Coulomb friction coefficient, at least 0.
    pub friction: f32,
    /// Bounciness from 0 (none) to 1 (perfectly elastic).
    pub restitution: f32,
    /// Mass per unit volume. The body's mass is the sum over its colliders.
    pub density: f32,
    /// A sensor detects overlaps and reports events but does not collide.
    pub sensor: bool,
    /// Collision groups.
    pub layers: CollisionLayers,
    /// Report [`Event::ContactStarted`](crate::Event) and similar events.
    pub events: bool,
    /// Report [`Event::ContactImpulse`](crate::Event) every step.
    pub contact_events: bool,
    /// How friction combines with the other collider's.
    pub friction_rule: CombineRule,
    /// How restitution combines with the other collider's.
    pub restitution_rule: CombineRule,
}

impl ColliderDesc3D {
    /// A solid collider with friction 0.5, no bounce and density 1.
    pub fn new(shape: Shape3D) -> Self {
        Self {
            shape,
            offset: Transform::IDENTITY,
            friction: 0.5,
            restitution: 0.0,
            density: 1.0,
            sensor: false,
            layers: CollisionLayers::ALL,
            events: true,
            contact_events: false,
            friction_rule: CombineRule::Average,
            restitution_rule: CombineRule::Average,
        }
    }

    /// Sets the friction coefficient.
    pub fn with_friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }

    /// Sets the restitution and combines it with the other collider's using
    /// the maximum, so one bouncy collider is enough.
    pub fn with_restitution(mut self, restitution: f32) -> Self {
        self.restitution = restitution;
        self.restitution_rule = CombineRule::Max;
        self
    }

    /// Sets the density.
    pub fn with_density(mut self, density: f32) -> Self {
        self.density = density;
        self
    }

    /// Makes the collider a sensor.
    pub fn as_sensor(mut self) -> Self {
        self.sensor = true;
        self
    }

    /// Sets the collision layers.
    pub fn with_layers(mut self, layers: CollisionLayers) -> Self {
        self.layers = layers;
        self
    }

    /// Sets the placement relative to the body.
    pub fn with_offset(mut self, offset: Transform) -> Self {
        self.offset = offset;
        self
    }

    /// Reports contact impulses each step.
    pub fn with_contact_events(mut self) -> Self {
        self.contact_events = true;
        self
    }
}
