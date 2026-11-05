//! IK solvers applied to a [`Pose`]: they read joint positions from the
//! pose and write the resulting rotations back.

use yowie_core::{Mat4, Quat, Transform, Vec3};

use crate::chain::{ChainOptions, ChainSolve, ccd, fabrik};
use crate::error::{AnimError, Result};
use crate::ik::{look_at, rotation_between, two_bone};
use crate::pose::Pose;
use crate::skeleton::Skeleton;

/// Which iterative solver [`solve_chain`] uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainSolver {
    /// Forward and backward reaching; usually converges in few iterations.
    Fabrik,
    /// Cyclic coordinate descent; rotates one joint at a time.
    Ccd,
}

fn position(world: &Mat4) -> Vec3 {
    world.w_axis.truncate()
}

fn rotation(world: &Mat4) -> Quat {
    Transform::from_matrix(*world).rotation
}

/// Sets the model-space rotation of `joint`, given the current world
/// matrices of the pose, by rewriting its local rotation.
fn set_world_rotation(skeleton: &Skeleton, pose: &mut Pose, world: &[Mat4], joint: usize, q: Quat) {
    let parent = skeleton.joints()[joint]
        .parent
        .map_or(Quat::IDENTITY, |p| rotation(&world[p]));
    if let Some(local) = pose.locals.get_mut(joint) {
        local.rotation = (parent.inverse() * q).normalize();
    }
}

/// Checks that each joint in `joints` is the parent of the next.
fn check_chain(skeleton: &Skeleton, joints: &[usize]) -> Result<()> {
    let bad = joints.iter().any(|&j| j >= skeleton.len())
        || joints
            .windows(2)
            .any(|w| skeleton.joints()[w[1]].parent != Some(w[0]));
    if bad {
        return Err(AnimError::InvalidSkeleton(
            "IK joints must be a chain of consecutive parent and child".into(),
        ));
    }
    Ok(())
}

/// Bends the chain `root`, `mid`, `end` (each the parent of the next) so
/// `end` reaches `target`, a model-space point, with `mid` bending towards
/// `pole`. Returns whether the target was in reach; when not, the chain
/// straightens towards it.
pub fn solve_two_bone(
    skeleton: &Skeleton,
    pose: &mut Pose,
    [root, mid, end]: [usize; 3],
    target: Vec3,
    pole: Vec3,
) -> Result<bool> {
    check_chain(skeleton, &[root, mid, end])?;
    let world = skeleton.world_matrices(pose);
    let s = two_bone(
        position(&world[root]),
        position(&world[mid]),
        position(&world[end]),
        target,
        pole,
    );
    let root_q = s.upper * rotation(&world[root]);
    let mid_q = s.lower * s.upper * rotation(&world[mid]);
    set_world_rotation(skeleton, pose, &world, root, root_q);
    if let Some(local) = pose.locals.get_mut(mid) {
        local.rotation = (root_q.inverse() * mid_q).normalize();
    }
    Ok(s.reached)
}

/// Turns `joint` so its local `forward` axis faces the model-space point
/// `target`, by at most `limit` radians from where the pose has it.
pub fn solve_look_at(
    skeleton: &Skeleton,
    pose: &mut Pose,
    joint: usize,
    forward: Vec3,
    target: Vec3,
    limit: f32,
) -> Result<()> {
    check_chain(skeleton, &[joint])?;
    let world = skeleton.world_matrices(pose);
    let current = rotation(&world[joint]);
    let delta = look_at(current * forward, target - position(&world[joint]), limit);
    set_world_rotation(skeleton, pose, &world, joint, delta * current);
    Ok(())
}

/// Moves the tip of a chain of consecutive joints (`joints[0]` is the fixed
/// root of the chain) to the model-space `target` with FABRIK or CCD. The
/// tip joint keeps its local rotation. See [`ChainOptions`] for limits.
pub fn solve_chain(
    skeleton: &Skeleton,
    pose: &mut Pose,
    joints: &[usize],
    target: Vec3,
    solver: ChainSolver,
    options: &ChainOptions,
) -> Result<ChainSolve> {
    check_chain(skeleton, joints)?;
    let world = skeleton.world_matrices(pose);
    let old: Vec<Vec3> = joints.iter().map(|&j| position(&world[j])).collect();
    let mut points = old.clone();
    let result = match solver {
        ChainSolver::Fabrik => fabrik(&mut points, target, options),
        ChainSolver::Ccd => ccd(&mut points, target, options),
    };
    let mut parent_q = joints
        .first()
        .and_then(|&j| skeleton.joints()[j].parent)
        .map_or(Quat::IDENTITY, |p| rotation(&world[p]));
    for k in 0..joints.len().saturating_sub(1) {
        let swing = rotation_between(old[k + 1] - old[k], points[k + 1] - points[k]);
        let q = swing * rotation(&world[joints[k]]);
        if let Some(local) = pose.locals.get_mut(joints[k]) {
            local.rotation = (parent_q.inverse() * q).normalize();
        }
        parent_q = q;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skeleton::Joint;

    fn leg() -> Skeleton {
        Skeleton::from_rest(vec![
            Joint::new("hip", None, Transform::from_xyz(0.0, 2.0, 0.0)),
            Joint::new("knee", Some(0), Transform::from_xyz(0.0, -1.0, 0.1)),
            Joint::new("foot", Some(1), Transform::from_xyz(0.0, -1.0, -0.1)),
        ])
        .unwrap()
    }

    fn foot(s: &Skeleton, pose: &Pose) -> Vec3 {
        position(&s.world_matrices(pose)[2])
    }

    #[test]
    fn two_bone_on_a_pose_reaches_target() {
        let s = leg();
        let mut pose = s.rest_pose();
        let target = Vec3::new(0.4, 0.5, 0.6);
        let reached = solve_two_bone(&s, &mut pose, [0, 1, 2], target, Vec3::new(0.0, 1.0, 3.0)).unwrap();
        assert!(reached);
        assert!((foot(&s, &pose) - target).length() < 1e-3);
        // An out-of-reach target points the straightened leg at it.
        let mut pose = s.rest_pose();
        let far = Vec3::new(0.0, 2.0, 10.0);
        let reached = solve_two_bone(&s, &mut pose, [0, 1, 2], far, Vec3::Y).unwrap();
        assert!(!reached);
        let dir = (foot(&s, &pose) - Vec3::new(0.0, 2.0, 0.0)).normalize();
        assert!((dir - Vec3::Z).length() < 1e-3);
    }

    #[test]
    fn chain_and_look_at_on_a_pose() {
        let s = leg();
        let mut pose = s.rest_pose();
        let target = Vec3::new(0.3, 0.8, 0.2);
        let r = solve_chain(&s, &mut pose, &[0, 1, 2], target, ChainSolver::Fabrik, &ChainOptions::default()).unwrap();
        assert!(r.converged);
        assert!((foot(&s, &pose) - target).length() < 2e-3);

        let mut pose = s.rest_pose();
        solve_look_at(&s, &mut pose, 0, Vec3::Y, Vec3::new(5.0, 2.0, 0.0), 3.0).unwrap();
        let w = s.world_matrices(&pose);
        let facing = rotation(&w[0]) * Vec3::Y;
        assert!((facing - Vec3::X).length() < 1e-4);
        assert!(solve_chain(&s, &mut pose, &[0, 2], target, ChainSolver::Ccd, &ChainOptions::default()).is_err());
    }
}
