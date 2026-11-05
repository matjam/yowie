//! Joint motor and limit descriptions shared by 2D and 3D.

/// A motor that drives a joint axis toward a target velocity or position.
///
/// Angles are radians and distances are world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motor {
    /// Target position of the axis. Used when `stiffness > 0`.
    pub target_position: f32,
    /// Target velocity of the axis. Used when `damping > 0`.
    pub target_velocity: f32,
    /// Strength of the pull toward `target_position`.
    pub stiffness: f32,
    /// Resistance to deviation from `target_velocity`.
    pub damping: f32,
    /// Largest force or torque the motor can apply.
    pub max_force: f32,
}

impl Motor {
    /// A motor that spins the axis at `velocity`, applying at most
    /// `max_force`.
    pub fn velocity(velocity: f32, max_force: f32) -> Self {
        Self {
            target_position: 0.0,
            target_velocity: velocity,
            stiffness: 0.0,
            damping: 1.0e4,
            max_force,
        }
    }

    /// A spring-like motor that holds the axis at `position`.
    pub fn position(position: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            target_position: position,
            target_velocity: 0.0,
            stiffness,
            damping,
            max_force: f32::MAX,
        }
    }
}

/// Lower and upper bound of a joint axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Lowest allowed value (radians or world units).
    pub min: f32,
    /// Highest allowed value (radians or world units).
    pub max: f32,
}

impl Limits {
    /// Creates limits from `min` to `max`.
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }
}
