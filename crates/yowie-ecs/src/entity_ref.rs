//! Entity references inside components: remapping and serde support.

use std::collections::HashMap;

use hecs::Entity;

/// What [`EntityMap::get`] returns for an entity the map does not contain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmapped {
    /// Keep the reference as it is. Used when cloning a subtree, where a
    /// reference to an entity outside the subtree stays valid.
    Keep,
    /// Replace the reference with [`Entity::DANGLING`]. Used when loading a
    /// document, where a reference to an entity the document lacks is
    /// meaningless.
    Dangling,
}

/// A translation from old entity handles to new ones, applied to the entity
/// references inside components by [`MapEntities`].
#[derive(Clone, Debug)]
pub struct EntityMap {
    map: HashMap<Entity, Entity>,
    unmapped: Unmapped,
}

impl EntityMap {
    /// Creates an empty map with the given policy for unknown entities.
    pub fn new(unmapped: Unmapped) -> Self {
        Self {
            map: HashMap::new(),
            unmapped,
        }
    }

    /// Records that `old` becomes `new`.
    pub fn insert(&mut self, old: Entity, new: Entity) {
        self.map.insert(old, new);
    }

    /// Returns the new handle for `old`, or the policy's answer when `old`
    /// is not in the map.
    pub fn get(&self, old: Entity) -> Entity {
        match (self.map.get(&old), self.unmapped) {
            (Some(new), _) => *new,
            (None, Unmapped::Keep) => old,
            (None, Unmapped::Dangling) => Entity::DANGLING,
        }
    }

    /// Returns the new handle only when `old` is in the map.
    pub fn lookup(&self, old: Entity) -> Option<Entity> {
        self.map.get(&old).copied()
    }

    /// Iterates over `(old, new)` pairs in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (Entity, Entity)> + '_ {
        self.map.iter().map(|(a, b)| (*a, *b))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true when the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Implemented by components that hold entity handles. Registering such a
/// component with [`World::register_mapped`](crate::World::register_mapped)
/// makes loading, cloning and instantiating rewrite its references.
///
/// ```
/// use yowie_ecs::{Entity, EntityMap, MapEntities};
///
/// struct Target(Entity);
///
/// impl MapEntities for Target {
///     fn map_entities(&mut self, map: &EntityMap) {
///         self.0 = map.get(self.0);
///     }
/// }
/// ```
pub trait MapEntities {
    /// Replaces every entity handle in `self` with `map.get(handle)`.
    fn map_entities(&mut self, map: &EntityMap);
}

/// Serde adapters for `Entity` fields, for use with `#[serde(with = ...)]`.
///
/// An entity is written as its 64-bit handle number. In scene and prefab
/// documents, authors write small positive numbers instead; see
/// [`Scene`](crate::Scene).
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use yowie_ecs::Entity;
///
/// #[derive(Serialize, Deserialize)]
/// struct Follow {
///     #[serde(with = "yowie_ecs::entity_serde")]
///     target: Entity,
///     #[serde(with = "yowie_ecs::entity_serde::option", default)]
///     leader: Option<Entity>,
/// }
/// ```
pub mod entity_serde {
    use hecs::Entity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Converts a document number to an entity. Numbers below 2^32 are not
    /// valid hecs handles; they are read as generation 1 and that index.
    pub fn from_number(n: u64) -> Entity {
        Entity::from_bits(n)
            .or_else(|| Entity::from_bits((1 << 32) | n))
            .unwrap_or(Entity::DANGLING)
    }

    /// Converts an entity to its document number: the full 64-bit handle,
    /// except that generation 1 is written as the bare index, so the
    /// numbers authors write by hand round-trip unchanged.
    pub fn to_number(entity: Entity) -> u64 {
        let bits = entity.to_bits().get();
        if bits >> 32 == 1 {
            bits & 0xFFFF_FFFF
        } else {
            bits
        }
    }

    /// Writes `entity` as its handle number.
    pub fn serialize<S: Serializer>(entity: &Entity, s: S) -> Result<S::Ok, S::Error> {
        to_number(*entity).serialize(s)
    }

    /// Reads an entity from a handle number.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Entity, D::Error> {
        u64::deserialize(d).map(from_number)
    }

    /// Adapter for `Option<Entity>`; `None` is written as null.
    pub mod option {
        use super::*;

        /// Writes the entity's handle number, or null.
        pub fn serialize<S: Serializer>(entity: &Option<Entity>, s: S) -> Result<S::Ok, S::Error> {
            entity.map(to_number).serialize(s)
        }

        /// Reads an optional handle number.
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Entity>, D::Error> {
            Ok(Option::<u64>::deserialize(d)?.map(from_number))
        }
    }
}

/// The handle a document author's number `n` stands for. Scene and prefab
/// documents number entities from 1; this is the placeholder those numbers
/// decode to, which [`EntityMap`] then replaces with the spawned entity.
pub(crate) fn doc_ref(n: u64) -> Entity {
    entity_serde::from_number(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_policies() {
        let a = doc_ref(1);
        let b = doc_ref(2);
        let mut keep = EntityMap::new(Unmapped::Keep);
        let mut dangle = EntityMap::new(Unmapped::Dangling);
        keep.insert(a, b);
        dangle.insert(a, b);
        assert_eq!(keep.get(a), b);
        assert_eq!(keep.get(b), b);
        assert_eq!(dangle.get(b), Entity::DANGLING);
        assert_eq!(dangle.lookup(b), None);
        assert_eq!(dangle.len(), 1);
    }

    #[test]
    fn numbers_roundtrip() {
        for n in [1_u64, 7, (1 << 32) | 5, (3 << 32) | 9] {
            let e = entity_serde::from_number(n);
            assert_ne!(e, Entity::DANGLING);
            assert_eq!(entity_serde::from_number(entity_serde::to_number(e)), e);
        }
        assert_eq!(doc_ref(1).id(), 1);
    }
}
