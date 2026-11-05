//! The 2D physics world: creation, stepping and events.

use std::collections::HashMap;
use std::sync::mpsc;

use rapier2d::dynamics::{RigidBody, RigidBodyBuilder, RigidBodyHandle, RigidBodyType};
use rapier2d::geometry::{ColliderHandle, CollisionEvent, CollisionEventFlags};
use rapier2d::pipeline::{ChannelEventCollector, DebugRenderPipeline, PhysicsWorld};
use yowie_core::Vec2;

use super::collider::build_collider;
use super::convert::*;
use super::desc::{BodyDesc2D, ColliderDesc2D};
use super::joint::JointDesc2D;
use crate::error::{PhysError, Result};
use crate::event::Event;
use crate::ids::{BodyId, BodyKind, ColliderId, JointId};
use crate::stepper::Pose2;

/// A 2D rigid-body simulation backed by rapier.
///
/// Create bodies with [`add_body`](Self::add_body), attach colliders with
/// [`add_collider`](Self::add_collider), then call [`step`](Self::step) with a
/// fixed time step. Queries see colliders only after the next `step` or
/// [`refresh_queries`](Self::refresh_queries).
pub struct World2D {
    pub(crate) pw: PhysicsWorld,
    pub(crate) events: Vec<Event>,
    pub(crate) prev: HashMap<RigidBodyHandle, Pose2>,
    pub(crate) debug: DebugRenderPipeline,
}

impl Default for World2D {
    fn default() -> Self {
        Self::new(Vec2::new(0.0, -9.81))
    }
}

impl World2D {
    /// Creates an empty world with the given gravity in units per second
    /// squared.
    pub fn new(gravity: Vec2) -> Self {
        let mut pw = PhysicsWorld::new();
        pw.gravity = vec(gravity);
        Self {
            pw,
            events: Vec::new(),
            prev: HashMap::new(),
            debug: DebugRenderPipeline::default(),
        }
    }

    /// The world gravity.
    pub fn gravity(&self) -> Vec2 {
        unvec(self.pw.gravity)
    }

    /// Sets the world gravity.
    pub fn set_gravity(&mut self, gravity: Vec2) {
        self.pw.gravity = vec(gravity);
    }

    /// Creates a body and returns its id.
    pub fn add_body(&mut self, desc: &BodyDesc2D) -> BodyId {
        let builder = match desc.kind {
            BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind::Kinematic => RigidBodyBuilder::kinematic_position_based(),
            BodyKind::Static => RigidBodyBuilder::fixed(),
        };
        let mut builder = builder
            .pose(pose(desc.position, desc.rotation))
            .linvel(vec(desc.linear_velocity))
            .angvel(desc.angular_velocity)
            .linear_damping(desc.linear_damping)
            .angular_damping(desc.angular_damping)
            .gravity_scale(desc.gravity_scale)
            .can_sleep(desc.can_sleep)
            .ccd_enabled(desc.ccd);
        if desc.lock_rotation {
            builder = builder.lock_rotations();
        }
        body_id(self.pw.insert_body(builder))
    }

    /// Attaches a collider to `body`.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::NoSuchBody`] for a stale id and
    /// [`PhysError::InvalidShape`] when the shape cannot be built.
    pub fn add_collider(&mut self, body: BodyId, desc: &ColliderDesc2D) -> Result<ColliderId> {
        let handle = body_handle(body);
        if self.pw.bodies.get(handle).is_none() {
            return Err(PhysError::NoSuchBody(body));
        }
        let collider = build_collider(desc)?;
        Ok(collider_id(self.pw.insert_collider(collider, Some(handle))))
    }

    /// Creates a collider with no body. It never moves and `desc.offset` is
    /// its world transform. Use it for level geometry.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidShape`] when the shape cannot be built.
    pub fn add_static_collider(&mut self, desc: &ColliderDesc2D) -> Result<ColliderId> {
        let collider = build_collider(desc)?;
        Ok(collider_id(self.pw.insert_collider(collider, None)))
    }

    /// Removes a body together with its colliders and joints.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::NoSuchBody`] for a stale id.
    pub fn remove_body(&mut self, body: BodyId) -> Result<()> {
        let handle = body_handle(body);
        self.prev.remove(&handle);
        self.pw
            .remove_body(handle)
            .map(|_| ())
            .ok_or(PhysError::NoSuchBody(body))
    }

    /// Removes one collider.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::NoSuchCollider`] for a stale id.
    pub fn remove_collider(&mut self, collider: ColliderId) -> Result<()> {
        self.pw
            .remove_collider(collider_handle(collider))
            .map(|_| ())
            .ok_or(PhysError::NoSuchCollider(collider))
    }

    /// Connects two bodies with a joint.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::NoSuchBody`] when either id is stale.
    pub fn add_joint(&mut self, a: BodyId, b: BodyId, desc: &JointDesc2D) -> Result<JointId> {
        for id in [a, b] {
            if self.pw.bodies.get(body_handle(id)).is_none() {
                return Err(PhysError::NoSuchBody(id));
            }
        }
        let h = self
            .pw
            .insert_impulse_joint(body_handle(a), body_handle(b), desc.build());
        Ok(joint_id(h))
    }

    /// Removes a joint.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::NoSuchJoint`] for a stale id.
    pub fn remove_joint(&mut self, joint: JointId) -> Result<()> {
        self.pw
            .remove_impulse_joint(joint_handle(joint))
            .map(|_| ())
            .ok_or(PhysError::NoSuchJoint(joint))
    }

    /// Advances the simulation by `dt` seconds. Use a constant `dt`, for
    /// example from [`FixedStepper`](crate::FixedStepper). Non-positive or
    /// non-finite values are ignored. Events from the previous step are
    /// discarded.
    pub fn step(&mut self, dt: f32) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        self.prev.clear();
        for (h, b) in self.pw.bodies.iter() {
            if !b.is_fixed() && !b.is_sleeping() {
                self.prev.insert(
                    h,
                    Pose2 {
                        position: unvec(b.translation()),
                        angle: b.rotation().angle(),
                    },
                );
            }
        }
        self.pw.integration_parameters.dt = dt;
        let (ctx, crx) = mpsc::channel();
        let (ftx, frx) = mpsc::channel();
        let (stx, _srx) = mpsc::channel();
        let collector = ChannelEventCollector::new(ctx, ftx, stx);
        self.pw.step_with_events(&(), &collector);
        drop(collector);
        self.events.clear();
        for ev in crx.try_iter() {
            self.push_collision_event(ev);
        }
        for ev in frx.try_iter() {
            self.events.push(Event::ContactImpulse {
                a: collider_id(ev.collider1),
                b: collider_id(ev.collider2),
                total: ev.total_force_magnitude * dt,
                max: ev.max_force_magnitude * dt,
            });
        }
    }

    fn push_collision_event(&mut self, ev: CollisionEvent) {
        let (h1, h2, flags, started) = match ev {
            CollisionEvent::Started(a, b, f) => (a, b, f, true),
            CollisionEvent::Stopped(a, b, f) => (a, b, f, false),
        };
        let (a, b) = (collider_id(h1), collider_id(h2));
        let event = if flags.contains(CollisionEventFlags::SENSOR) {
            let is_sensor =
                |h: ColliderHandle| self.pw.colliders.get(h).is_some_and(|c| c.is_sensor());
            let (sensor, other) = if is_sensor(h1) || !is_sensor(h2) {
                (a, b)
            } else {
                (b, a)
            };
            if started {
                Event::SensorEntered { sensor, other }
            } else {
                Event::SensorExited { sensor, other }
            }
        } else if started {
            Event::ContactStarted { a, b }
        } else {
            Event::ContactStopped { a, b }
        };
        self.events.push(event);
    }

    /// Brings the query structures up to date with colliders added or moved
    /// since the last step, without simulating.
    pub fn refresh_queries(&mut self) {
        self.pw.detect_collisions(&(), &());
    }

    /// Events produced by the last [`step`](Self::step).
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Takes the events produced by the last step, leaving none.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Number of bodies in the world.
    pub fn body_count(&self) -> usize {
        self.pw.bodies.len()
    }

    /// Number of colliders in the world.
    pub fn collider_count(&self) -> usize {
        self.pw.colliders.len()
    }

    /// The body a collider is attached to, if any.
    pub fn body_of(&self, collider: ColliderId) -> Option<BodyId> {
        let c = self.pw.colliders.get(collider_handle(collider))?;
        c.parent().map(body_id)
    }

    /// The colliders attached to a body.
    pub fn colliders_of(&self, body: BodyId) -> Vec<ColliderId> {
        self.pw
            .bodies
            .get(body_handle(body))
            .map(|b| b.colliders().iter().copied().map(collider_id).collect())
            .unwrap_or_default()
    }

    pub(crate) fn rb(&self, id: BodyId) -> Result<&RigidBody> {
        self.pw
            .bodies
            .get(body_handle(id))
            .ok_or(PhysError::NoSuchBody(id))
    }

    pub(crate) fn rb_mut(&mut self, id: BodyId) -> Result<&mut RigidBody> {
        self.pw
            .bodies
            .get_mut(body_handle(id))
            .ok_or(PhysError::NoSuchBody(id))
    }

    pub(crate) fn kind_of(body: &RigidBody) -> BodyKind {
        match body.body_type() {
            RigidBodyType::Dynamic => BodyKind::Dynamic,
            RigidBodyType::Fixed => BodyKind::Static,
            _ => BodyKind::Kinematic,
        }
    }
}
