//! The error type shared by every fallible operation in the crate.

use hecs::Entity;

/// Failure of an ECS operation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The entity does not exist, or has been despawned.
    #[error("entity {0:?} does not exist")]
    NoSuchEntity(Entity),
    /// Attaching `child` under `parent` would make an entity its own ancestor.
    #[error("attaching {child:?} under {parent:?} would create a cycle")]
    HierarchyCycle {
        /// The entity being attached.
        child: Entity,
        /// The entity it was to be attached under.
        parent: Entity,
    },
    /// A document names a component that was not registered.
    #[error("component {0:?} is not registered")]
    UnknownComponent(String),
    /// A document names a resource that was not registered.
    #[error("resource {0:?} is not registered")]
    UnknownResource(String),
    /// A scene names a prefab the library does not hold.
    #[error("no prefab named {0:?}")]
    UnknownPrefab(String),
    /// Entities carry components with no registered name, so they cannot be
    /// written out.
    #[error("{count} component type(s) on entity {entity:?} are not registered")]
    UnregisteredComponents {
        /// The entity that carries them.
        entity: Entity,
        /// How many unregistered component types it carries.
        count: usize,
    },
    /// A component value failed to encode or decode.
    #[error("component {name:?}: {source}")]
    Component {
        /// The registered name of the component.
        name: String,
        /// The JSON error.
        source: serde_json::Error,
    },
    /// The document has a structural problem, such as a duplicate entity
    /// name or a parent number that is out of range.
    #[error("invalid document: {0}")]
    InvalidDocument(String),
    /// The document was written by a newer version of the format.
    #[error("unsupported document version {0}")]
    UnsupportedVersion(u32),
    /// The document is not valid JSON of the expected shape.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Reading or writing failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
