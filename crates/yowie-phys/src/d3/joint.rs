//! 3D joint descriptions.

use rapier3d::dynamics::{
    FixedJointBuilder, GenericJoint, JointAxis, PrismaticJointBuilder, RevoluteJointBuilder,
    RopeJointBuilder, SphericalJointBuilder, SpringJointBuilder,
};
use yowie_core::Vec3;

use super::convert::vec;
use crate::joint::{Limits, Motor};

/// The kind of constraint a 3D joint applies, with its parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum JointKind3D {
    /// Allows rotation about one axis (a hinge, or revolute joint).
    Hinge {
        /// Rotation axis in the local space of both bodies.
        axis: Vec3,
        /// Allowed angle range in radians.
        limits: Option<Limits>,
        /// Drives the hinge angle.
        motor: Option<Motor>,
    },
    /// Allows sliding along one axis.
    Prismatic {
        /// Slide axis in the local space of both bodies.
        axis: Vec3,
        /// Allowed translation range in world units.
        limits: Option<Limits>,
        /// Drives the slide position.
        motor: Option<Motor>,
    },
    /// Locks both bodies together.
    Fixed,
    /// A ball-and-socket joint: free rotation about the anchor.
    Spherical {
        /// Optional angle limits (radians) about the local X, Y and Z axes.
        limits: [Option<Limits>; 3],
    },
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
}

/// A joint between two 3D bodies.
#[derive(Debug, Clone, PartialEq)]
pub struct JointDesc3D {
    /// The constraint.
    pub kind: JointKind3D,
    /// Attachment point in the local space of the first body.
    pub anchor_a: Vec3,
    /// Attachment point in the local space of the second body.
    pub anchor_b: Vec3,
    /// Whether the two bodies still collide with each other.
    pub collide_connected: bool,
}

impl JointDesc3D {
    /// Creates a joint of `kind` with both anchors at the body origins.
    pub fn new(kind: JointKind3D) -> Self {
        Self {
            kind,
            anchor_a: Vec3::ZERO,
            anchor_b: Vec3::ZERO,
            collide_connected: false,
        }
    }

    /// A hinge about `axis` with no limits or motor.
    pub fn hinge(axis: Vec3) -> Self {
        Self::new(JointKind3D::Hinge {
            axis,
            limits: None,
            motor: None,
        })
    }

    /// Sets the anchors in each body's local space.
    pub fn with_anchors(mut self, a: Vec3, b: Vec3) -> Self {
        self.anchor_a = a;
        self.anchor_b = b;
        self
    }

    /// Sets angle or translation limits on a hinge or prismatic joint.
    pub fn with_limits(mut self, new: Limits) -> Self {
        match &mut self.kind {
            JointKind3D::Hinge { limits, .. } | JointKind3D::Prismatic { limits, .. } => {
                *limits = Some(new);
            }
            _ => {}
        }
        self
    }

    /// Sets the motor on a hinge or prismatic joint.
    pub fn with_motor(mut self, new: Motor) -> Self {
        match &mut self.kind {
            JointKind3D::Hinge { motor, .. } | JointKind3D::Prismatic { motor, .. } => {
                *motor = Some(new);
            }
            _ => {}
        }
        self
    }

    pub(crate) fn build(&self) -> GenericJoint {
        let (a, b) = (vec(self.anchor_a), vec(self.anchor_b));
        let contacts = self.collide_connected;
        match &self.kind {
            JointKind3D::Hinge {
                axis,
                limits,
                motor,
            } => {
                let mut j = RevoluteJointBuilder::new(vec(*axis))
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
            JointKind3D::Prismatic {
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
            JointKind3D::Fixed => FixedJointBuilder::new()
                .local_anchor1(a)
                .local_anchor2(b)
                .contacts_enabled(contacts)
                .build()
                .into(),
            JointKind3D::Spherical { limits } => {
                let mut j = SphericalJointBuilder::new()
                    .local_anchor1(a)
                    .local_anchor2(b)
                    .contacts_enabled(contacts);
                let axes = [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ];
                for (axis, l) in axes.into_iter().zip(limits) {
                    if let Some(l) = l {
                        j = j.limits(axis, [l.min, l.max]);
                    }
                }
                j.build().into()
            }
            JointKind3D::Distance {
                rest_length,
                stiffness,
                damping,
            } => SpringJointBuilder::new(*rest_length, *stiffness, *damping)
                .local_anchor1(a)
                .local_anchor2(b)
                .contacts_enabled(contacts)
                .build()
                .into(),
            JointKind3D::Rope { max_length } => RopeJointBuilder::new(*max_length)
                .local_anchor1(a)
                .local_anchor2(b)
                .contacts_enabled(contacts)
                .build()
                .into(),
        }
    }
}
