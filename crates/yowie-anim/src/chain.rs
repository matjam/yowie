//! Iterative IK for chains of any length: FABRIK and CCD.

use yowie_core::{Quat, Vec3};

use crate::ik::{any_perpendicular, rotation_between};

/// Settings shared by [`fabrik`] and [`ccd`].
#[derive(Clone, Debug, PartialEq)]
pub struct ChainOptions {
    /// Distance from the target at which the chain counts as arrived.
    pub tolerance: f32,
    /// Most iterations to run.
    pub max_iterations: usize,
    /// Largest bend in radians allowed at each point of the chain, between
    /// the bone arriving at it and the bone leaving it. Entry `i` limits
    /// point `i`; the first and last points have no bend. Missing or `None`
    /// entries are unlimited.
    pub limits: Vec<Option<f32>>,
}

impl Default for ChainOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-3,
            max_iterations: 128,
            limits: Vec::new(),
        }
    }
}

impl ChainOptions {
    fn limit(&self, i: usize) -> Option<f32> {
        self.limits.get(i).copied().flatten()
    }
}

/// How a chain solve ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainSolve {
    /// True when the tip is within tolerance of the target.
    pub converged: bool,
    /// Iterations run.
    pub iterations: usize,
    /// Distance from the tip to the target afterwards.
    pub error: f32,
}

fn finish(points: &[Vec3], target: Vec3, tol: f32, iterations: usize) -> ChainSolve {
    let error = points.last().map_or(0.0, |p| p.distance(target));
    ChainSolve {
        converged: error <= tol,
        iterations,
        error,
    }
}

/// Turns `dir` towards `prev` until the angle between them is at most `limit`.
fn clamp_bend(prev: Vec3, dir: Vec3, limit: f32) -> Vec3 {
    let angle = prev.angle_between(dir);
    if angle <= limit {
        return dir;
    }
    let axis = prev.cross(dir).try_normalize().unwrap_or_else(|| any_perpendicular(prev));
    Quat::from_axis_angle(axis, limit.max(0.0)) * prev
}

fn bone_dir(points: &[Vec3], i: usize) -> Vec3 {
    (points[i + 1] - points[i]).normalize_or(Vec3::X)
}

/// Moves `points` (joint positions, root first) so the last reaches
/// `target` with the root fixed, using FABRIK: alternating passes from
/// tip to root and root to tip. Bone lengths are kept. A target beyond
/// reach straightens the chain towards it.
pub fn fabrik(points: &mut [Vec3], target: Vec3, options: &ChainOptions) -> ChainSolve {
    let n = points.len();
    if n < 2 {
        return finish(points, target, options.tolerance, 0);
    }
    let lengths: Vec<f32> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
    let root = points[0];
    let total: f32 = lengths.iter().sum();
    if root.distance(target) >= total {
        let dir = (target - root).normalize_or(Vec3::X);
        for i in 0..n - 1 {
            points[i + 1] = points[i] + dir * lengths[i];
        }
        return finish(points, target, options.tolerance, 0);
    }
    let mut iterations = 0;
    while iterations < options.max_iterations
        && points[n - 1].distance(target) > options.tolerance
    {
        points[n - 1] = target;
        for i in (0..n - 1).rev() {
            points[i] = points[i + 1] + (points[i] - points[i + 1]).normalize_or(Vec3::X) * lengths[i];
        }
        points[0] = root;
        for i in 0..n - 1 {
            let mut dir = (points[i + 1] - points[i]).normalize_or(Vec3::X);
            if let (true, Some(limit)) = (i > 0, options.limit(i)) {
                dir = clamp_bend(bone_dir(points, i - 1), dir, limit);
            }
            points[i + 1] = points[i] + dir * lengths[i];
        }
        iterations += 1;
    }
    finish(points, target, options.tolerance, iterations)
}

/// Moves `points` so the last reaches `target` with the root fixed, using
/// cyclic coordinate descent: each joint in turn, tip to root, swings the
/// rest of the chain towards the target, within its bend limit.
pub fn ccd(points: &mut [Vec3], target: Vec3, options: &ChainOptions) -> ChainSolve {
    let n = points.len();
    let mut iterations = 0;
    while n >= 2
        && iterations < options.max_iterations
        && points[n - 1].distance(target) > options.tolerance
    {
        for i in (0..n - 1).rev() {
            let pivot = points[i];
            let mut swing = rotation_between(points[n - 1] - pivot, target - pivot);
            if let (true, Some(limit)) = (i > 0, options.limit(i)) {
                let now = bone_dir(points, i);
                let clamped = clamp_bend(bone_dir(points, i - 1), swing * now, limit);
                swing = rotation_between(now, clamped);
            }
            for p in &mut points[i + 1..] {
                *p = pivot + swing * (*p - pivot);
            }
        }
        iterations += 1;
    }
    finish(points, target, options.tolerance, iterations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(n: usize) -> Vec<Vec3> {
        (0..n).map(|i| Vec3::new(i as f32, 0.0, 0.0)).collect()
    }

    fn lengths(p: &[Vec3]) -> Vec<f32> {
        p.windows(2).map(|w| w[0].distance(w[1])).collect()
    }

    fn max_bend(p: &[Vec3]) -> f32 {
        (1..p.len() - 1)
            .map(|i| bone_dir(p, i - 1).angle_between(bone_dir(p, i)))
            .fold(0.0, f32::max)
    }

    #[test]
    fn both_solvers_converge_and_keep_lengths() {
        let target = Vec3::new(1.5, 2.0, 0.5);
        for solver in [fabrik, ccd] {
            let mut p = straight(5);
            let r = solver(&mut p, target, &ChainOptions::default());
            assert!(r.converged, "{r:?}");
            assert!(p[4].distance(target) < 1e-3);
            assert_eq!(p[0], Vec3::ZERO);
            for l in lengths(&p) {
                assert!((l - 1.0).abs() < 1e-3, "length {l}");
            }
        }
    }

    #[test]
    fn unreachable_target_straightens_towards_it() {
        let target = Vec3::new(0.0, 20.0, 0.0);
        let mut p = straight(4);
        let r = fabrik(&mut p, target, &ChainOptions::default());
        assert!(!r.converged);
        assert!((p[3] - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4);
        let mut p = straight(4);
        ccd(&mut p, target, &ChainOptions::default());
        assert!((p[3] - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-2);
    }

    #[test]
    fn limits_cap_the_bend() {
        let target = Vec3::new(1.0, 1.5, 0.0);
        let options = ChainOptions {
            limits: vec![None, Some(0.5), Some(0.5), Some(0.5), None],
            max_iterations: 200,
            ..ChainOptions::default()
        };
        for solver in [fabrik, ccd] {
            let mut p = straight(5);
            solver(&mut p, target, &options);
            assert!(max_bend(&p) <= 0.5 + 1e-3, "bend {}", max_bend(&p));
        }
    }

    #[test]
    fn short_chains_are_safe() {
        let mut one = [Vec3::X];
        assert!(fabrik(&mut one, Vec3::X, &ChainOptions::default()).converged);
        let mut none: [Vec3; 0] = [];
        let _ = ccd(&mut none, Vec3::X, &ChainOptions::default());
    }
}
