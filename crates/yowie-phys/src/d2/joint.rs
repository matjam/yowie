//! 2D joint descriptions.

use rapier2d::dynamics::{
    FixedJointBuilder, GenericJoint, GenericJointBuilder, JointAxesMask, JointAxis,
    PrismaticJointBuilder, RevoluteJointBuilder, RopeJointBuilder, SpringJointBuilder,
};
use yowie_core::Vec2;

use super::convert::vec;
use crate::joint::{Limits, Motor};

/// The kind of constraint a 2D joint applies, with its parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum JointKind2D {
    /// Allows rotation about the anchor (a hinge, or revolute joint).
    Hinge {
        /// Allowed relative angle range in radians.
        limits: Option<Limits>,
        /// Drives the relative angle.
        motor: Option<Motor>,
    },
    /// Allows sliding along one axis.
    Prismatic {
        /// Slide axis in the local space of both bodies.
        axis: Vec2,
        /// Allowed translation range in world units.
        limits: Option<Limits>,
        /// Drives the slide position.
        motor: Option<Motor>,
    },
    /// Locks both bodies together.
    Fixed,
    /// A spring that pulls the anchors toward `rest_length` apart.
    Distance {
        /// Distance at which the spring exerts no force.
        rest_length: f32,
        /// Spring stiffness.
        stiffness: f32,
        /// Spring damping.
        damping: f32,
    },
    /// Keeps the anchors at most `max_length` apart; slack when closer.
    Rope {
        /// Longest allowed distance between the anchors.
        max_length: f32,
    },
    /// A wheel on a spring suspension: the second body spins freely and
    /// slides along `axis` against a spring, as on a vehicle.
    Wheel {
        /// Suspension axis in the local space of both bodies.
        axis: Vec2,
        /// Suspension spring stiffness.
        stiffness: f32,
        /// Suspension damping.
        damping: f32,
        /// Drives the wheel's spin.
        motor: Option<Motor>,
    },
}

/// A joint between two 2D bodies.
#[derive(Debug, Clone, PartialEq)]
pub struct JointDesc2D {
    /// The constraint.
    pub kind: JointKind2D,
    /// Attachment point in the local space of the first body.
    pub anchor_a: Vec2,
    /// Attachment point in the local space of the second body.
    pub anchor_b: Vec2,
    /// Whether the two bodies still collide with each other.
    pub collide_connected: bool,
}

impl JointDesc2D {
    /// Creates a joint of `kind` with both anchors at the body origins.
    pub fn new(kind: JointKind2D) -> Self {
        Self {
            kind,
            anchor_a: Vec2::ZERO,
            anchor_b: Vec2::ZERO,
            collide_connected: false,
        }
    }

    /// A hinge with no limits or motor.
    pub fn hinge() -> Self {
        Self::new(JointKind2D::Hinge {
            limits: None,
            motor: None,
        })
    }

    /// Sets the anchors in each body's local space.
    pub fn with_anchors(mut self, a: Vec2, b: Vec2) -> Self {
        self.anchor_a = a;
        self.anchor_b = b;
        self
    }

    /// Sets angle or translation limits on a hinge or prismatic joint.
    pub fn with_limits(mut self, new: Limits) -> Self {
        match &mut self.kind {
            JointKind2D::Hinge { limits, .. } | JointKind2D::Prismatic { limits, .. } => {
                *limits = Some(new);
            }
            _ => {}
        }
        self
    }

    /// Sets the motor on a hinge, prismatic or wheel joint.
    pub fn with_motor(mut self, new: Motor) -> Self {
        match &mut self.kind {
            JointKind2D::Hinge { motor, .. }
            | JointKind2D::Prismatic { motor, .. }
            | JointKind2D::Wheel { motor, .. } => *motor = Some(new),
            _ => {}
        }
        self
    }

    pub(crate) fn build(&self) -> GenericJoint {
        let (a, b) = (vec(self.anchor_a), vec(self.anchor_b));
        let contacts = self.collide_connected;
        match &self.kind {
            JointKind2D::Hinge { limits, motor } => {
                let mut j = RevoluteJointBuilder::new()
                    .local_anchor1(a)
                    .local_anchor2(b)
                    .contacts_enabled(contacts);
                if let Some(l) = limits {
                    j = j.limits([l.min, l.max]);
                }
                if let Some(m) = motor {
                    j = j
                        .motor(m.target_position, m.target_velocity, m.stiffness, m.damping)
                        .motor_max_force(m.max_force);
                }
                j.build().into()
            }
            JointKind2D::Prismatic {
                axis,
                limits,
                motor,
            } => {
                let mut j = PrismaticJointBuilder::new(vec(*axis))
                    .local_anchor1(a)
                    .local_anchor2(b)
                    .contacts_enabled(contacts);
                if let Some(l) = limits {
                    j = j.limits([l.min, l.max]);
                }
                if let Some(m) = motor {
                    j = j
                        .set_motor(m.target_position, m.target_velocity, m.stiffness, m.damping)
                        .motor_max_force(m.max_force);
                }
                j.build().into()
            }
            JointKind2D::Fixed => FixedJointBuilder::new()
                .local_anchor1(a)
                .local_anchor2(b)
                .contacts_enabled(contacts)
                .build()
                .into(),
            JointKind2D::Distance {
                rest_length,
                stiffness,
                damping,
            } => SpringJointBuilder::new(*rest_length, *stiffness, *damping)
                .local_anchor1(a)
                .local_anchor2(b)
                .contacts_enabled(contacts)
                .build()
                .into(),
            JointKind2D::Rope { max_length } => RopeJointBuilder::new(*max_length)
                .local_anchor1(a)
                .local_anchor2(b)
                .contacts_enabled(contacts)
                .build()
                .into(),
            JointKind2D::Wheel {
                axis,
                stiffness,
                damping,
                motor,
            } => {
                let axis = vec(axis.normalize_or(Vec2::Y));
                let mut j = GenericJointBuilder::new(JointAxesMask::LOCKED_PIN_SLOT_AXES)
                    .local_axis1(axis)
                    .local_axis2(axis)
                    .local_anchor1(a)
                    .local_anchor2(b)
                    .contacts_enabled(contacts)
                    .motor_position(JointAxis::LinX, 0.0, *stiffness, *damping);
                if let Some(m) = motor {
                    j = j
                        .motor_velocity(JointAxis::AngX, m.target_velocity, m.damping)
                        .motor_max_force(JointAxis::AngX, m.max_force);
                }
                j.build()
            }
        }
    }
}
