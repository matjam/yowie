//! Kinematic character controller for 2D, on rapier's controller.

use rapier2d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier2d::pipeline::{QueryFilter as RFilter, QueryFilterFlags};
use yowie_core::Vec2;

use super::convert::*;
use super::shape::Shape2D;
use super::world::World2D;
use crate::error::Result;
use crate::ids::BodyId;

/// Settings for moving a character shape through a 2D world.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterController2D {
    /// The character's collision shape, usually a capsule.
    pub shape: Shape2D,
    /// The direction that counts as up.
    pub up: Vec2,
    /// Gap kept between the character and surfaces, in world units.
    pub skin: f32,
    /// Steepest slope the character climbs, in radians from horizontal.
    pub max_climb_angle: f32,
    /// Slope angle in radians above which the character slides down.
    pub min_slide_angle: f32,
    /// Tallest step climbed automatically; `None` disables stepping.
    pub step_height: Option<f32>,
    /// Narrowest ledge that counts as a step, in world units.
    pub min_step_width: f32,
    /// Distance within which the character is pulled down onto the ground;
    /// `None` disables snapping.
    pub snap_to_ground: Option<f32>,
    /// Slide along walls instead of stopping at them.
    pub slide: bool,
}

impl CharacterController2D {
    /// A controller for `shape` with a 0.01 skin, 45 degree slopes, 0.3 unit
    /// steps and 0.2 unit ground snapping.
    pub fn new(shape: Shape2D) -> Self {
        Self {
            shape,
            up: Vec2::Y,
            skin: 0.01,
            max_climb_angle: std::f32::consts::FRAC_PI_4,
            min_slide_angle: std::f32::consts::FRAC_PI_4,
            step_height: Some(0.3),
            min_step_width: 0.1,
            snap_to_ground: Some(0.2),
            slide: true,
        }
    }
}

/// What a character is allowed to do this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterMove2D {
    /// The translation to apply to the character.
    pub translation: Vec2,
    /// Whether the character stands on ground after moving.
    pub grounded: bool,
    /// Whether the character is sliding down a slope steeper than
    /// `min_slide_angle`.
    pub sliding: bool,
}

impl World2D {
    /// Computes how far a character at `position` can move toward `desired`
    /// without entering solid colliders, climbing steps and slopes and
    /// sticking to the ground as configured. The world is not changed.
    ///
    /// `exclude` is the character's own body, if it has one. Sensors are
    /// ignored.
    ///
    /// # Errors
    ///
    /// Returns an error when the controller's shape cannot be built.
    pub fn move_character(
        &self,
        ctrl: &CharacterController2D,
        position: Vec2,
        angle: f32,
        desired: Vec2,
        dt: f32,
        exclude: Option<BodyId>,
    ) -> Result<CharacterMove2D> {
        let shape = ctrl.shape.to_shared()?;
        let mut kcc = KinematicCharacterController {
            up: vec(ctrl.up.normalize_or(Vec2::Y)),
            offset: CharacterLength::Absolute(ctrl.skin.max(1.0e-4)),
            slide: ctrl.slide,
            max_slope_climb_angle: ctrl.max_climb_angle,
            min_slope_slide_angle: ctrl.min_slide_angle,
            snap_to_ground: ctrl.snap_to_ground.map(CharacterLength::Absolute),
            ..KinematicCharacterController::default()
        };
        kcc.autostep = ctrl.step_height.map(|h| CharacterAutostep {
            max_height: CharacterLength::Absolute(h),
            min_width: CharacterLength::Absolute(ctrl.min_step_width),
            include_dynamic_bodies: false,
        });
        let filter = RFilter {
            flags: QueryFilterFlags::EXCLUDE_SENSORS,
            exclude_rigid_body: exclude.map(body_handle),
            ..RFilter::default()
        };
        let queries = self.pw.query_pipeline_with_filter(filter);
        let out = kcc.move_shape(
            dt,
            &queries,
            &*shape,
            &pose(position, angle),
            vec(desired),
            |_| {},
        );
        Ok(CharacterMove2D {
            translation: unvec(out.translation),
            grounded: out.grounded,
            sliding: out.is_sliding_down_slope,
        })
    }

    /// Moves a kinematic body with [`move_character`](Self::move_character)
    /// and schedules the result as its next position. Returns the move.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale id or an invalid controller shape.
    pub fn move_character_body(
        &mut self,
        ctrl: &CharacterController2D,
        body: BodyId,
        desired: Vec2,
        dt: f32,
    ) -> Result<CharacterMove2D> {
        let (pos, angle) = {
            let b = self.rb(body)?;
            (unvec(b.translation()), b.rotation().angle())
        };
        let out = self.move_character(ctrl, pos, angle, desired, dt, Some(body))?;
        self.set_kinematic_target(body, pos + out.translation, angle)?;
        Ok(out)
    }
}
