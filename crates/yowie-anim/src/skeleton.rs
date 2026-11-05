//! Joint hierarchies, rest poses and skinning matrices.

use std::collections::HashMap;

use yowie_core::{Mat4, Transform};

use crate::clip::Clip;
use crate::error::{AnimError, Result};
use crate::player::Player;
use crate::pose::Pose;
use crate::value::Value;

/// One joint of a [`Skeleton`].
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    /// Unique name; clip tracks address joints by it.
    pub name: String,
    /// Index of the parent joint, which must be lower than this joint's
    /// own index; `None` for a root.
    pub parent: Option<usize>,
    /// Local transform in the rest pose.
    pub rest: Transform,
    /// Transforms a vertex from model space into this joint's rest space.
    pub inverse_bind: Mat4,
}

impl Joint {
    /// A joint whose inverse bind matrix is not yet known; use
    /// [`Skeleton::from_rest`] to derive it from the rest pose.
    pub fn new(name: impl Into<String>, parent: Option<usize>, rest: Transform) -> Self {
        Self {
            name: name.into(),
            parent,
            rest,
            inverse_bind: Mat4::IDENTITY,
        }
    }
}

/// A joint hierarchy with a rest pose and inverse bind matrices.
///
/// Joints are stored parents first, so world transforms resolve in one
/// pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    joints: Vec<Joint>,
    names: HashMap<String, usize>,
}

impl Skeleton {
    /// Builds a skeleton from joints with their inverse bind matrices.
    /// Fails when a parent index does not precede its child or a name
    /// repeats.
    pub fn new(joints: Vec<Joint>) -> Result<Self> {
        let mut names = HashMap::with_capacity(joints.len());
        for (i, j) in joints.iter().enumerate() {
            if j.parent.is_some_and(|p| p >= i) {
                return Err(AnimError::InvalidSkeleton(format!(
                    "joint {} ({}) has a parent that does not precede it",
                    i, j.name
                )));
            }
            if names.insert(j.name.clone(), i).is_some() {
                return Err(AnimError::InvalidSkeleton(format!(
                    "duplicate joint name {}",
                    j.name
                )));
            }
        }
        Ok(Self { joints, names })
    }

    /// Builds a skeleton and sets every inverse bind matrix to the inverse of
    /// the joint's rest-pose world matrix, so the rest pose skins to
    /// identity.
    pub fn from_rest(joints: Vec<Joint>) -> Result<Self> {
        let mut skeleton = Self::new(joints)?;
        let rest = skeleton.rest_pose();
        let world = skeleton.world_matrices(&rest);
        for (j, w) in skeleton.joints.iter_mut().zip(world) {
            j.inverse_bind = w.inverse();
        }
        Ok(skeleton)
    }

    /// The joints, parents first.
    pub fn joints(&self) -> &[Joint] {
        &self.joints
    }

    /// Number of joints.
    pub fn len(&self) -> usize {
        self.joints.len()
    }

    /// True when there are no joints.
    pub fn is_empty(&self) -> bool {
        self.joints.is_empty()
    }

    /// The index of the joint named `name`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    /// The pose with every joint at rest.
    pub fn rest_pose(&self) -> Pose {
        Pose {
            locals: self.joints.iter().map(|j| j.rest).collect(),
            morph: Default::default(),
        }
    }

    /// Model-space matrix of every joint for `pose`.
    pub fn world_matrices(&self, pose: &Pose) -> Vec<Mat4> {
        let mut world: Vec<Mat4> = Vec::with_capacity(self.joints.len());
        for (i, joint) in self.joints.iter().enumerate() {
            let local = pose.locals.get(i).unwrap_or(&joint.rest).to_matrix();
            let m = match joint.parent {
                Some(p) => world[p] * local,
                None => local,
            };
            world.push(m);
        }
        world
    }

    /// The matrices a skinned mesh uploads: each joint's model-space matrix
    /// times its inverse bind matrix. The rest pose gives identity for a
    /// skeleton built with [`Skeleton::from_rest`].
    pub fn joint_matrices(&self, pose: &Pose) -> Vec<Mat4> {
        self.world_matrices(pose)
            .into_iter()
            .zip(&self.joints)
            .map(|(w, j)| w * j.inverse_bind)
            .collect()
    }

    /// Writes a sampled value into `pose` if `name` is a joint (or, for
    /// morph weights, a node name).
    pub fn apply_value(&self, pose: &mut Pose, name: &str, value: &Value) {
        if let Value::Weights(w) = value {
            pose.morph.insert(name.to_string(), w.clone());
            return;
        }
        let Some(local) = self.index_of(name).and_then(|i| pose.locals.get_mut(i)) else {
            return;
        };
        match value {
            Value::Translation(v) | Value::Vec3(v) => local.translation = *v,
            Value::Rotation(q) | Value::Quat(q) => local.rotation = *q,
            Value::Scale(s) => local.scale = *s,
            Value::Transform(t) => *local = *t,
            _ => {}
        }
    }

    /// Samples `clip` at `time` over the rest pose.
    pub fn pose_from_clip(&self, clip: &Clip, time: f32) -> Pose {
        let mut pose = self.rest_pose();
        clip.for_each_value(time, |n, v| self.apply_value(&mut pose, n, &v));
        pose
    }

    /// The pose a player currently shows, crossfades included, over the
    /// rest pose.
    pub fn pose_from_player(&self, player: &Player) -> Pose {
        let mut pose = self.rest_pose();
        player.evaluate(|n, v| self.apply_value(&mut pose, n, &v));
        pose
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use yowie_core::{Quat, Vec3};

    /// A three-joint chain along +Y, one unit apart, with a turned middle joint.
    pub(crate) fn arm() -> Skeleton {
        Skeleton::from_rest(vec![
            Joint::new("shoulder", None, Transform::from_xyz(0.0, 1.0, 0.0)),
            Joint::new(
                "elbow",
                Some(0),
                Transform::from_xyz(0.0, 1.0, 0.0).with_rotation(Quat::from_rotation_z(0.4)),
            ),
            Joint::new("hand", Some(1), Transform::from_xyz(0.0, 1.0, 0.0)),
        ])
        .unwrap()
    }

    #[test]
    fn rest_pose_joint_matrices_are_identity() {
        let s = arm();
        for m in s.joint_matrices(&s.rest_pose()) {
            assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{m:?}");
        }
    }

    #[test]
    fn moved_joint_skins_its_children() {
        let s = arm();
        let mut pose = s.rest_pose();
        pose.locals[0].translation += Vec3::X * 2.0;
        let m = s.joint_matrices(&pose);
        for joint in &m {
            assert!(
                (joint.transform_point3(Vec3::ZERO) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5
            );
        }
    }

    #[test]
    fn rejects_bad_hierarchy_and_duplicates() {
        let bad_parent = vec![Joint::new("a", Some(0), Transform::IDENTITY)];
        assert!(matches!(
            Skeleton::new(bad_parent),
            Err(AnimError::InvalidSkeleton(_))
        ));
        let dup = vec![
            Joint::new("a", None, Transform::IDENTITY),
            Joint::new("a", None, Transform::IDENTITY),
        ];
        assert!(matches!(
            Skeleton::new(dup),
            Err(AnimError::InvalidSkeleton(_))
        ));
    }

    #[test]
    fn clip_poses_by_joint_name() {
        use crate::curve::Curve;
        use crate::value::Channel;
        let s = arm();
        let clip = Clip::new("c").with_track(
            "elbow",
            Channel::Rotation(Curve::linear([
                (0.0, Quat::IDENTITY),
                (1.0, Quat::from_rotation_z(1.0)),
            ])),
        );
        let pose = s.pose_from_clip(&clip, 1.0);
        assert!(
            pose.locals[1]
                .rotation
                .dot(Quat::from_rotation_z(1.0))
                .abs()
                > 0.9999
        );
        assert_eq!(pose.locals[0], s.joints()[0].rest);
    }
}
