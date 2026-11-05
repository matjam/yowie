//! Turning a [`ColliderDesc3D`] into a rapier collider.

use rapier3d::dynamics::CoefficientCombineRule;
use rapier3d::geometry::{Collider, ColliderBuilder};
use rapier3d::pipeline::ActiveEvents;

use super::convert::{groups, transform_pose};
use super::desc::ColliderDesc3D;
use crate::error::{PhysError, Result};
use crate::ids::CombineRule;

fn rule(r: CombineRule) -> CoefficientCombineRule {
    match r {
        CombineRule::Average => CoefficientCombineRule::Average,
        CombineRule::Min => CoefficientCombineRule::Min,
        CombineRule::Multiply => CoefficientCombineRule::Multiply,
        CombineRule::Max => CoefficientCombineRule::Max,
    }
}

pub(crate) fn build_collider(desc: &ColliderDesc3D) -> Result<Collider> {
    if !(desc.density.is_finite() && desc.density >= 0.0) {
        return Err(PhysError::InvalidParameter("density must be at least 0"));
    }
    let shape = desc.shape.to_shared()?;
    let mut events = ActiveEvents::empty();
    if desc.events {
        events |= ActiveEvents::COLLISION_EVENTS;
    }
    if desc.contact_events {
        events |= ActiveEvents::CONTACT_FORCE_EVENTS;
    }
    Ok(ColliderBuilder::new(shape)
        .position(transform_pose(&desc.offset))
        .friction(desc.friction.max(0.0))
        .friction_combine_rule(rule(desc.friction_rule))
        .restitution(desc.restitution.max(0.0))
        .restitution_combine_rule(rule(desc.restitution_rule))
        .density(desc.density)
        .sensor(desc.sensor)
        .collision_groups(groups(desc.layers))
        .active_events(events)
        .contact_force_event_threshold(0.0)
        .build())
}
