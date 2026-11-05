//! Conversions between yowie-core (glam 0.34) and rapier (glam 0.33) types.

use rapier2d::dynamics::{ImpulseJointHandle, RigidBodyHandle};
use rapier2d::geometry::{ColliderHandle, Group, InteractionGroups, InteractionTestMode};
use rapier2d::math::{Pose, Vector};
use yowie_core::{Transform2D, Vec2};

use crate::ids::{BodyId, ColliderId, CollisionLayers, JointId};

pub(crate) fn vec(v: Vec2) -> Vector {
    Vector::new(v.x, v.y)
}

pub(crate) fn unvec(v: Vector) -> Vec2 {
    Vec2::new(v.x, v.y)
}

pub(crate) fn pose(position: Vec2, angle: f32) -> Pose {
    Pose::new(vec(position), angle)
}

pub(crate) fn transform_pose(t: &Transform2D) -> Pose {
    pose(t.position, t.rotation)
}

pub(crate) fn body_id(h: RigidBodyHandle) -> BodyId {
    let (index, generation) = h.into_raw_parts();
    BodyId { index, generation }
}

pub(crate) fn body_handle(id: BodyId) -> RigidBodyHandle {
    RigidBodyHandle::from_raw_parts(id.index, id.generation)
}

pub(crate) fn collider_id(h: ColliderHandle) -> ColliderId {
    let (index, generation) = h.into_raw_parts();
    ColliderId { index, generation }
}

pub(crate) fn collider_handle(id: ColliderId) -> ColliderHandle {
    ColliderHandle::from_raw_parts(id.index, id.generation)
}

pub(crate) fn joint_id(h: ImpulseJointHandle) -> JointId {
    let (index, generation) = h.into_raw_parts();
    JointId { index, generation }
}

pub(crate) fn joint_handle(id: JointId) -> ImpulseJointHandle {
    ImpulseJointHandle::from_raw_parts(id.index, id.generation)
}

pub(crate) fn groups(l: CollisionLayers) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_truncate(l.memberships),
        Group::from_bits_truncate(l.mask),
        InteractionTestMode::And,
    )
}
