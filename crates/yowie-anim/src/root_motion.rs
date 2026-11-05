//! Root motion: movement authored on a root joint, taken out of the pose.

use std::f32::consts::{PI, TAU};

use yowie_core::{EulerRot, Quat, Transform, Vec3};

use crate::clip::Clip;
use crate::pose::Pose;
use crate::timeline::{LoopMode, Sweep, sweeps};

/// Movement of a root joint over a span of time, in model space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootMotion {
    /// How far the root moved.
    pub translation: Vec3,
    /// How far the root turned about +Y, in radians.
    pub yaw: f32,
}

impl RootMotion {
    /// No movement.
    pub const ZERO: Self = Self {
        translation: Vec3::ZERO,
        yaw: 0.0,
    };

    /// Both parts multiplied by `w`.
    pub fn scaled(self, w: f32) -> Self {
        Self {
            translation: self.translation * w,
            yaw: self.yaw * w,
        }
    }

    /// The sum of two movements.
    pub fn plus(self, other: Self) -> Self {
        Self {
            translation: self.translation + other.translation,
            yaw: self.yaw + other.yaw,
        }
    }
}

/// The rotation of `q` about +Y.
fn yaw_of(q: Quat) -> f32 {
    q.to_euler(EulerRot::YXZ).0
}

/// Wraps an angle into -PI to PI.
fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// The root's movement between two clip times, without wrapping.
fn between(clip: &Clip, root: &str, from: f32, to: f32) -> RootMotion {
    let sample = |t| clip.sample_transform(root, t, Transform::IDENTITY);
    match (sample(from), sample(to)) {
        (Some(a), Some(b)) => RootMotion {
            translation: b.translation - a.translation,
            yaw: wrap(yaw_of(b.rotation) - yaw_of(a.rotation)),
        },
        _ => RootMotion::ZERO,
    }
}

/// The root's movement over a list of clip-time sweeps.
pub fn over_sweeps(clip: &Clip, root: &str, sweeps: &[Sweep]) -> RootMotion {
    sweeps.iter().fold(RootMotion::ZERO, |m, s| {
        m.plus(between(clip, root, s.from, s.to))
    })
}

/// The root joint's movement while playback of `clip` goes from position
/// `x0` to `x1` seconds, summed across loop points. Positions are unwrapped:
/// a looping 1 second clip moved from 0.5 to 1.25 covers the rest of the
/// first lap and the start of the second.
pub fn root_motion_between(
    clip: &Clip,
    root: &str,
    mode: LoopMode,
    x0: f64,
    x1: f64,
) -> RootMotion {
    over_sweeps(clip, root, &sweeps(mode, clip.duration(), x0, x1, false))
}

/// Holds the root joint at its rest translation and yaw, so the authored
/// movement reported by [`RootMotion`] does not also move the model.
/// Translation, including the vertical part, and yaw are held; pitch and
/// roll animate as authored.
pub fn lock_root(pose: &mut Pose, joint: usize, rest: &Transform) {
    if let Some(t) = pose.locals.get_mut(joint) {
        t.translation = rest.translation;
        let fix = Quat::from_rotation_y(yaw_of(rest.rotation) - yaw_of(t.rotation));
        t.rotation = (fix * t.rotation).normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curve;
    use crate::value::Channel;

    fn walk() -> Clip {
        // Moves 4 units along +Z over 2 seconds and turns a quarter turn.
        Clip::new("walk")
            .with_track(
                "root",
                Channel::Translation(Curve::linear([
                    (0.0, Vec3::ZERO),
                    (2.0, Vec3::new(0.0, 0.0, 4.0)),
                ])),
            )
            .with_track(
                "root",
                Channel::Rotation(Curve::linear([
                    (0.0, Quat::IDENTITY),
                    (2.0, Quat::from_rotation_y(1.0)),
                ])),
            )
    }

    #[test]
    fn sums_across_loop_wrap() {
        let clip = walk();
        // 1.5 -> 2.5 on a looping 2 s clip: 0.5 s at the end, 0.5 s at the start.
        let m = root_motion_between(&clip, "root", LoopMode::Loop, 1.5, 2.5);
        assert!((m.translation - Vec3::new(0.0, 0.0, 2.0)).length() < 1e-5);
        assert!((m.yaw - 0.5).abs() < 1e-4);
        // A step of several laps adds up whole laps.
        let m = root_motion_between(&clip, "root", LoopMode::Loop, 0.0, 6.0);
        assert!((m.translation.z - 12.0).abs() < 1e-4);
    }

    #[test]
    fn lock_root_holds_rest() {
        let rest = Transform::IDENTITY;
        let mut pose = Pose {
            locals: vec![
                Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_y(0.7)),
            ],
            morph: Default::default(),
        };
        lock_root(&mut pose, 0, &rest);
        assert_eq!(pose.locals[0].translation, Vec3::ZERO);
        assert!(yaw_of(pose.locals[0].rotation).abs() < 1e-5);
    }
}
