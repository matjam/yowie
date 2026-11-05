//! Reading and changing 3D bodies.

use yowie_core::{Quat, Transform, Vec3};

use super::convert::*;
use super::world::World3D;
use crate::error::Result;
use crate::ids::{BodyId, BodyKind};

impl World3D {
    /// World position of a body, or `None` for a stale id.
    pub fn position(&self, body: BodyId) -> Option<Vec3> {
        self.rb(body).ok().map(|b| unvec(b.translation()))
    }

    /// World rotation of a body, or `None` for a stale id.
    pub fn rotation(&self, body: BodyId) -> Option<Quat> {
        self.rb(body).ok().map(|b| unrot(*b.rotation()))
    }

    /// World transform of a body (scale is 1), or `None` for a stale id.
    pub fn transform(&self, body: BodyId) -> Option<Transform> {
        self.rb(body).ok().map(|b| untransform(b.position()))
    }

    /// The body's transform between the previous and the latest step.
    ///
    /// `alpha` is the fraction of a step that has elapsed since the latest
    /// step, from [`FixedStepper::alpha`](crate::FixedStepper::alpha). 0 gives
    /// the previous pose and 1 the current one. Bodies that did not move in
    /// the last step return their current pose.
    pub fn interpolated_transform(&self, body: BodyId, alpha: f32) -> Option<Transform> {
        let current = self.transform(body)?;
        let Some(prev) = self.prev.get(&body_handle(body)) else {
            return Some(current);
        };
        let now = crate::stepper::Pose3 {
            position: current.translation,
            rotation: current.rotation,
        };
        Some(prev.lerp(now, alpha.clamp(0.0, 1.0)))
    }

    /// Teleports a body. Velocities are kept.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn set_transform(&mut self, body: BodyId, position: Vec3, rotation: Quat) -> Result<()> {
        self.rb_mut(body)?
            .set_position(pose(position, rotation), true);
        Ok(())
    }

    /// Sets where a kinematic body should be after the next step. Its
    /// velocity is derived from the move so it pushes bodies it touches.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn set_kinematic_target(
        &mut self,
        body: BodyId,
        position: Vec3,
        rotation: Quat,
    ) -> Result<()> {
        self.rb_mut(body)?
            .set_next_kinematic_position(pose(position, rotation));
        Ok(())
    }

    /// Linear velocity in units per second.
    pub fn linear_velocity(&self, body: BodyId) -> Option<Vec3> {
        self.rb(body).ok().map(|b| unvec(b.linvel()))
    }

    /// Angular velocity in radians per second (axis scaled by the rate).
    pub fn angular_velocity(&self, body: BodyId) -> Option<Vec3> {
        self.rb(body).ok().map(|b| unvec(b.angvel()))
    }

    /// Sets the linear velocity and wakes the body.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn set_linear_velocity(&mut self, body: BodyId, v: Vec3) -> Result<()> {
        self.rb_mut(body)?.set_linvel(vec(v), true);
        Ok(())
    }

    /// Sets the angular velocity and wakes the body.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn set_angular_velocity(&mut self, body: BodyId, w: Vec3) -> Result<()> {
        self.rb_mut(body)?.set_angvel(vec(w), true);
        Ok(())
    }

    /// Applies an instantaneous impulse (mass times velocity change) at the
    /// centre of mass.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn apply_impulse(&mut self, body: BodyId, impulse: Vec3) -> Result<()> {
        self.rb_mut(body)?.apply_impulse(vec(impulse), true);
        Ok(())
    }

    /// Applies an impulse at a world-space point, which also spins the body.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn apply_impulse_at_point(
        &mut self,
        body: BodyId,
        impulse: Vec3,
        point: Vec3,
    ) -> Result<()> {
        self.rb_mut(body)?
            .apply_impulse_at_point(vec(impulse), vec(point), true);
        Ok(())
    }

    /// Adds a force that stays applied until [`reset_forces`](Self::reset_forces).
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn apply_force(&mut self, body: BodyId, force: Vec3) -> Result<()> {
        self.rb_mut(body)?.add_force(vec(force), true);
        Ok(())
    }

    /// Clears forces added with [`apply_force`](Self::apply_force).
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn reset_forces(&mut self, body: BodyId) -> Result<()> {
        self.rb_mut(body)?.reset_forces(true);
        Ok(())
    }

    /// Whether the body is asleep. Stale ids report `false`.
    pub fn is_sleeping(&self, body: BodyId) -> bool {
        self.rb(body).is_ok_and(|b| b.is_sleeping())
    }

    /// Puts a body to sleep immediately.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn sleep(&mut self, body: BodyId) -> Result<()> {
        self.rb_mut(body)?.sleep();
        Ok(())
    }

    /// Wakes a body.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id.
    pub fn wake(&mut self, body: BodyId) -> Result<()> {
        self.rb_mut(body)?.wake_up(true);
        Ok(())
    }

    /// Total mass of a body in the units of collider density times volume.
    pub fn mass(&self, body: BodyId) -> Option<f32> {
        self.rb(body).ok().map(|b| b.mass())
    }

    /// How the body is simulated.
    pub fn kind(&self, body: BodyId) -> Option<BodyKind> {
        self.rb(body).ok().map(Self::kind_of)
    }
}
