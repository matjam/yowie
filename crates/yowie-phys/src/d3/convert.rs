//! Conversions between yowie-core (glam 0.34) and rapier (glam 0.33) types.

use rapier3d::dynamics::{ImpulseJointHandle, RigidBodyHandle};
use rapier3d::geometry::{ColliderHandle, Group, InteractionGroups};
use rapier3d::math::{Pose, Rotation, Vector};
use yowie_core::{Quat, Transform, Vec3};

use crate::ids::{BodyId, ColliderId, CollisionLayers, JointId};

pub(crate) fn vec(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

pub(crate) fn unvec(v: Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub(crate) fn rot(q: Quat) -> Rotation {
    Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

pub(crate) fn unrot(q: Rotation) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

pub(crate) fn pose(position: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(vec(position), rot(rotation))
}

pub(crate) fn transform_pose(t: &Transform) -> Pose {
    pose(t.translation, t.rotation)
}

pub(crate) fn untransform(p: &Pose) -> Transform {
    Transform {
        translation: unvec(p.translation),
        rotation: unrot(p.rotation),
        scale: Vec3::ONE,
    }
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
        rapier3d::geometry::InteractionTestMode::And,
    )
}
