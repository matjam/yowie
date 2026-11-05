//! Handles and small enums shared by the 2D and 3D worlds.

/// How a body is simulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BodyKind {
    /// Moved by forces, contacts and joints.
    #[default]
    Dynamic,
    /// Moved only by the caller, through velocity or by setting its next
    /// position. Pushes dynamic bodies but is never pushed.
    Kinematic,
    /// Never moves. Cheapest to collide against.
    Static,
}

/// Handle to a rigid body in a world.
///
/// Ids are generational: using the id of a removed body returns an error or
/// `None` instead of touching a different body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

/// Handle to a collider in a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColliderId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

/// Handle to a joint in a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JointId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

/// Bit masks that decide which colliders can touch.
///
/// Two colliders interact when each one's `memberships` intersects the
/// other's `mask`. The default belongs to every group and collides with
/// every group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CollisionLayers {
    /// Groups this collider belongs to, one bit per group.
    pub memberships: u32,
    /// Groups this collider can touch, one bit per group.
    pub mask: u32,
}

impl CollisionLayers {
    /// Belongs to every group and collides with every group.
    pub const ALL: Self = Self {
        memberships: u32::MAX,
        mask: u32::MAX,
    };

    /// Creates layers from a membership mask and a collision mask.
    pub const fn new(memberships: u32, mask: u32) -> Self {
        Self { memberships, mask }
    }
}

impl Default for CollisionLayers {
    fn default() -> Self {
        Self::ALL
    }
}

/// How two colliders' friction or restitution values combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CombineRule {
    /// The mean of both values.
    #[default]
    Average,
    /// The smaller value.
    Min,
    /// The product.
    Multiply,
    /// The larger value.
    Max,
}

/// Restricts which colliders a query reports.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryFilter {
    /// When set, only colliders that interact with these layers are reported.
    pub layers: Option<CollisionLayers>,
    /// Skip sensor colliders.
    pub exclude_sensors: bool,
    /// Skip every collider of this body.
    pub exclude_body: Option<BodyId>,
    /// Skip this collider.
    pub exclude_collider: Option<ColliderId>,
    /// Skip colliders on static bodies and parentless colliders.
    pub exclude_static: bool,
}

impl QueryFilter {
    /// A filter that reports everything.
    pub const fn new() -> Self {
        Self {
            layers: None,
            exclude_sensors: false,
            exclude_body: None,
            exclude_collider: None,
            exclude_static: false,
        }
    }

    /// Reports only colliders that interact with `layers`.
    pub const fn with_layers(mut self, layers: CollisionLayers) -> Self {
        self.layers = Some(layers);
        self
    }

    /// Skips sensors.
    pub const fn without_sensors(mut self) -> Self {
        self.exclude_sensors = true;
        self
    }

    /// Skips every collider attached to `body`.
    pub const fn without_body(mut self, body: BodyId) -> Self {
        self.exclude_body = Some(body);
        self
    }
}
