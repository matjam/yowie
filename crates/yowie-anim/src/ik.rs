//! Analytic inverse kinematics: two-bone chains and look-at.

use yowie_core::{Quat, Vec3};

/// The shortest rotation taking direction `a` to direction `b`. Identity
/// when either has no length.
pub fn rotation_between(a: Vec3, b: Vec3) -> Quat {
    let (Some(a), Some(b)) = (a.try_normalize(), b.try_normalize()) else {
        return Quat::IDENTITY;
    };
    // Built from the cross product and atan2 rather than `from_rotation_arc`,
    // which loses precision for nearly parallel directions.
    let cross = a.cross(b);
    let (sin, cos) = (cross.length(), a.dot(b));
    match cross.try_normalize() {
        Some(axis) => Quat::from_axis_angle(axis, sin.atan2(cos)),
        None if cos < 0.0 => Quat::from_axis_angle(any_perpendicular(a), std::f32::consts::PI),
        None => Quat::IDENTITY,
    }
}

/// A unit vector perpendicular to `v`.
pub(crate) fn any_perpendicular(v: Vec3) -> Vec3 {
    v.normalize_or(Vec3::X).any_orthonormal_vector()
}

/// The result of solving a two-bone chain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoBone {
    /// New position of the middle joint.
    pub mid: Vec3,
    /// New position of the end joint. Equal to the target when it is
    /// reachable; otherwise the point on the straightened chain nearest it.
    pub end: Vec3,
    /// Rotation to apply to the root joint about its own position.
    pub upper: Quat,
    /// Rotation to apply to the middle joint about its own position after
    /// `upper` has moved it.
    pub lower: Quat,
    /// False when the target is farther than the chain's length or closer
    /// than its minimum reach.
    pub reached: bool,
}

/// Solves a chain of two bones so its end reaches `target`: a leg planted
/// on uneven ground, an arm reaching a handle.
///
/// `root`, `mid` and `end` are the joint positions now, `target` is where
/// the end should go and `pole` is a point the middle joint bends towards
/// (in front of a knee, behind an elbow). All are in one space and the
/// rotations are in that space. A target out of reach straightens the chain
/// towards it.
pub fn two_bone(root: Vec3, mid: Vec3, end: Vec3, target: Vec3, pole: Vec3) -> TwoBone {
    let (a, b) = ((mid - root).length(), (end - mid).length());
    let unchanged = TwoBone {
        mid,
        end,
        upper: Quat::IDENTITY,
        lower: Quat::IDENTITY,
        reached: false,
    };
    if a < 1e-6 || b < 1e-6 {
        return unchanged;
    }
    let to_target = target - root;
    let dist = to_target.length();
    let dir = to_target
        .try_normalize()
        .unwrap_or_else(|| (end - root).normalize_or(Vec3::Y));
    let reached = dist >= (a - b).abs() && dist <= a + b;
    let d = dist.clamp((a - b).abs().max(1e-4), a + b);
    // The bend direction: the pole's offset from the root-target line.
    let reject = |v: Vec3| v - dir * v.dot(dir);
    let bend = [pole - root, mid - root]
        .into_iter()
        .map(reject)
        .find(|v| v.length() > 1e-5)
        .map_or_else(|| any_perpendicular(dir), Vec3::normalize);
    let along = (a * a - b * b + d * d) / (2.0 * d);
    let height = (a * a - along * along).max(0.0).sqrt();
    let new_mid = root + dir * along + bend * height;
    let new_end = root + dir * d;
    let upper = rotation_between(mid - root, new_mid - root);
    let lower = rotation_between(upper * (end - mid), new_end - new_mid);
    TwoBone {
        mid: new_mid,
        end: new_end,
        upper,
        lower,
        reached,
    }
}

/// The rotation that turns `forward` (a direction in the same space as
/// `to`) towards `to`, limited to `limit` radians. Identity when `to` has
/// no length.
pub fn look_at(forward: Vec3, to: Vec3, limit: f32) -> Quat {
    let q = rotation_between(forward, to);
    let (axis, angle) = q.to_axis_angle();
    if angle > limit {
        Quat::from_axis_angle(axis, limit.max(0.0))
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: Vec3 = Vec3::new(0.0, 2.0, 0.0);
    const MID: Vec3 = Vec3::new(0.0, 1.0, 0.1);
    const END: Vec3 = Vec3::new(0.0, 0.0, 0.0);

    fn applied_end(s: &TwoBone) -> Vec3 {
        // Replays the returned rotations on the original chain.
        s.mid + s.lower * (s.upper * (END - MID))
    }

    #[test]
    fn reaches_a_reachable_target_and_bends_to_the_pole() {
        let target = Vec3::new(0.5, 0.6, 0.4);
        let pole = Vec3::new(0.0, 1.5, 2.0);
        let s = two_bone(ROOT, MID, END, target, pole);
        assert!(s.reached);
        assert!((s.end - target).length() < 1e-4);
        assert!((applied_end(&s) - target).length() < 1e-3);
        // Bone lengths are preserved.
        let (a, b) = ((MID - ROOT).length(), (END - MID).length());
        assert!(((s.mid - ROOT).length() - a).abs() < 1e-4);
        assert!(((s.end - s.mid).length() - b).abs() < 1e-4);
        // The knee leans towards the pole side.
        assert!(s.mid.z > (ROOT.z + target.z) / 2.0);
        assert!((s.upper * (MID - ROOT) + ROOT - s.mid).length() < 1e-4);
    }

    #[test]
    fn points_at_an_unreachable_target() {
        let target = Vec3::new(3.0, 6.0, 0.0);
        let s = two_bone(ROOT, MID, END, target, Vec3::new(0.0, 2.0, 5.0));
        assert!(!s.reached);
        let want = (target - ROOT).normalize();
        let got = (s.end - ROOT).normalize();
        assert!((want - got).length() < 1e-4);
        let reach = (MID - ROOT).length() + (END - MID).length();
        assert!(((s.end - ROOT).length() - reach).abs() < 1e-3);
        assert!((applied_end(&s) - s.end).length() < 1e-3);
    }

    #[test]
    fn degenerate_inputs_do_not_produce_nan() {
        let s = two_bone(ROOT, MID, END, ROOT, ROOT);
        assert!(s.end.is_finite() && s.mid.is_finite());
        let s = two_bone(ROOT, ROOT, END, END, END);
        assert_eq!(s.upper, Quat::IDENTITY);
    }

    #[test]
    fn look_at_respects_limit() {
        let q = look_at(Vec3::Z, Vec3::X, 10.0);
        assert!((q * Vec3::Z - Vec3::X).length() < 1e-5);
        let q = look_at(Vec3::Z, Vec3::X, 0.5);
        let (_, angle) = q.to_axis_angle();
        assert!((angle - 0.5).abs() < 1e-5);
        assert_eq!(look_at(Vec3::Z, Vec3::ZERO, 1.0), Quat::IDENTITY);
    }
}
