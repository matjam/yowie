//! Fixed-timestep accumulator and body pose interpolation.

use yowie_core::{Quat, Transform, Transform2D, Vec2, Vec3};

/// Turns variable frame times into a whole number of fixed physics steps.
///
/// Call [`advance`](Self::advance) once per frame, run `step(fixed_dt)` that
/// many times, then render with [`alpha`](Self::alpha) passed to the world's
/// `interpolated_transform`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedStepper {
    dt: f32,
    accumulator: f32,
    max_steps: u32,
}

impl FixedStepper {
    /// Creates a stepper that runs steps of `fixed_dt` seconds and never more
    /// than `max_steps` per frame, so a long frame cannot stall the game.
    /// A non-positive or non-finite `fixed_dt` falls back to 1/60 second.
    pub fn new(fixed_dt: f32, max_steps: u32) -> Self {
        let dt = if fixed_dt.is_finite() && fixed_dt > 0.0 {
            fixed_dt
        } else {
            1.0 / 60.0
        };
        Self {
            dt,
            accumulator: 0.0,
            max_steps: max_steps.max(1),
        }
    }

    /// The fixed step length in seconds.
    pub fn fixed_dt(&self) -> f32 {
        self.dt
    }

    /// Adds `frame_dt` seconds and returns how many fixed steps to run. Time
    /// beyond `max_steps` steps is discarded.
    pub fn advance(&mut self, frame_dt: f32) -> u32 {
        if frame_dt.is_finite() && frame_dt > 0.0 {
            self.accumulator += frame_dt;
        }
        let mut steps = 0;
        while self.accumulator >= self.dt && steps < self.max_steps {
            self.accumulator -= self.dt;
            steps += 1;
        }
        if steps == self.max_steps {
            self.accumulator = self.accumulator.min(self.dt);
        }
        steps
    }

    /// Fraction of a step left in the accumulator, in `[0, 1)`. Pass it to
    /// `interpolated_transform`.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.dt).clamp(0.0, 1.0)
    }
}

impl Default for FixedStepper {
    fn default() -> Self {
        Self::new(1.0 / 60.0, 8)
    }
}

/// Position and rotation of a 3D body at one step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose3 {
    pub position: Vec3,
    pub rotation: Quat,
}

impl Pose3 {
    /// Blends from `self` to `other` by `t` and returns a unit-scale transform.
    pub fn lerp(self, other: Pose3, t: f32) -> Transform {
        Transform {
            translation: self.position.lerp(other.position, t),
            rotation: self.rotation.slerp(other.rotation, t),
            scale: Vec3::ONE,
        }
    }
}

/// Position and rotation of a 2D body at one step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose2 {
    pub position: Vec2,
    pub angle: f32,
}

impl Pose2 {
    /// Blends from `self` to `other` by `t` and returns a unit-scale transform.
    pub fn lerp(self, other: Pose2, t: f32) -> Transform2D {
        Transform2D {
            position: self.position.lerp(other.position, t),
            rotation: self.angle + (other.angle - self.angle) * t,
            scale: Vec2::ONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_counts_whole_steps() {
        let cases = [(0.0, 0), (1.0 / 60.0, 1), (0.051, 3), (10.0, 8)];
        for (frame, expected) in cases {
            let mut s = FixedStepper::default();
            assert_eq!(s.advance(frame), expected, "frame {frame}");
            assert!((0.0..=1.0).contains(&s.alpha()));
        }
    }

    #[test]
    fn accumulator_carries_over() {
        let mut s = FixedStepper::new(0.1, 8);
        assert_eq!(s.advance(0.15), 1);
        assert!((s.alpha() - 0.5).abs() < 1e-4);
        assert_eq!(s.advance(0.05), 1);
    }

    #[test]
    fn bad_dt_falls_back() {
        assert!((FixedStepper::new(-1.0, 4).fixed_dt() - 1.0 / 60.0).abs() < 1e-6);
    }
}
