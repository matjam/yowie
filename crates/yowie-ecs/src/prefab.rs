//! Prefabs: JSON documents describing an entity tree, instantiated many times.

use std::collections::BTreeMap;

use hecs::Entity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entity_ref::{EntityMap, Unmapped, doc_ref};
use crate::error::{Error, Result};
use crate::world::World;

/// A template for an entity and its descendants.
///
/// Components are JSON values keyed by their registered names. The entities
/// of a prefab are numbered from 1 in depth-first order, the root being 1;
/// an entity reference inside a component value is one of these numbers and
/// becomes the handle of that entity when the prefab is instantiated.
///
/// ```json
/// { "components": { "Transform2D": { "position": [0, 0], "rotation": 0, "scale": [1, 1] } },
///   "children": [ { "components": { "Name": "gun" } } ] }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefab {
    /// Component values by registered name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, Value>,
    /// Child prefabs, in attach order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Prefab>,
}

impl Prefab {
    /// Parses a prefab document.
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Writes the prefab as indented JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Captures `root` and its descendants from a world. Components that
    /// are not registered are left out; references to entities outside the
    /// tree are lost.
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] when `root` is missing, plus encoding errors.
    pub fn capture(world: &World, root: Entity) -> Result<Self> {
        let mut nodes = vec![root];
        nodes.extend(world.descendants(root));
        let mut map = EntityMap::new(Unmapped::Dangling);
        for (i, e) in nodes.iter().enumerate() {
            map.insert(*e, doc_ref(i as u64 + 1));
        }
        fn build(world: &World, e: Entity, map: &EntityMap) -> Result<Prefab> {
            Ok(Prefab {
                components: world.components_to_json(e, map, false)?,
                children: world
                    .children(e)
                    .into_iter()
                    .map(|c| build(world, c, map))
                    .collect::<Result<_>>()?,
            })
        }
        build(world, root, &map)
    }
}

/// Component values that replace a prefab's when it is instantiated.
///
/// An override replaces the prefab's whole component of that name, and may
/// add components the prefab lacks. [`Overrides::child`] targets a child by
/// its index in the prefab's `children`.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    components: BTreeMap<String, Value>,
    children: BTreeMap<usize, Overrides>,
}

impl Overrides {
    /// Creates an empty set of overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the component `name` with a JSON value.
    pub fn set_json(mut self, name: &str, value: Value) -> Self {
        self.components.insert(name.to_owned(), value);
        self
    }

    /// Replaces the component `name` with the encoding of `value`.
    pub fn set<T: Serialize>(self, name: &str, value: &T) -> Result<Self> {
        let json = serde_json::to_value(value).map_err(|source| Error::Component {
            name: name.to_owned(),
            source,
        })?;
        Ok(self.set_json(name, json))
    }

    /// Applies `overrides` to the child at `index`.
    pub fn child(mut self, index: usize, overrides: Overrides) -> Self {
        self.children.insert(index, overrides);
        self
    }
}

/// Named prefabs, which scenes refer to.
#[derive(Clone, Debug, Default)]
pub struct PrefabLibrary {
    prefabs: BTreeMap<String, Prefab>,
}

impl PrefabLibrary {
    /// Creates an empty library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a prefab under `name`, replacing any previous one.
    pub fn insert(&mut self, name: &str, prefab: Prefab) {
        self.prefabs.insert(name.to_owned(), prefab);
    }

    /// Parses a prefab document and adds it under `name`.
    pub fn insert_json(&mut self, name: &str, json: &str) -> Result<()> {
        self.insert(name, Prefab::from_json(json)?);
        Ok(())
    }

    /// Returns the prefab called `name`.
    pub fn get(&self, name: &str) -> Option<&Prefab> {
        self.prefabs.get(name)
    }
}

/// One prefab node after overrides are merged in.
struct Flat {
    components: BTreeMap<String, Value>,
    parent: Option<usize>,
}

fn flatten(prefab: &Prefab, over: Option<&Overrides>, parent: Option<usize>, out: &mut Vec<Flat>) {
    let mut components = prefab.components.clone();
    if let Some(o) = over {
        components.extend(o.components.clone());
    }
    let index = out.len();
    out.push(Flat { components, parent });
    for (i, child) in prefab.children.iter().enumerate() {
        flatten(
            child,
            over.and_then(|o| o.children.get(&i)),
            Some(index),
            out,
        );
    }
}

impl World {
    /// Spawns the prefab's entity tree and returns the root.
    ///
    /// Every call creates new entities. Entity references in component
    /// values are rewritten to the new entities. Nothing is left in the
    /// world when the call fails.
    ///
    /// # Errors
    /// [`Error::UnknownComponent`] for an unregistered name, and decoding
    /// errors.
    pub fn spawn_prefab(&mut self, prefab: &Prefab, overrides: &Overrides) -> Result<Entity> {
        let mut flat = Vec::new();
        flatten(prefab, Some(overrides), None, &mut flat);
        for node in &flat {
            self.check_known(node.components.keys(), false)?;
        }
        let entities: Vec<Entity> = flat.iter().map(|_| self.inner.spawn(())).collect();
        match self.fill_prefab(&flat, &entities) {
            Ok(()) => Ok(entities[0]),
            Err(err) => {
                for e in entities {
                    let _ = self.inner.despawn(e);
                }
                Err(err)
            }
        }
    }

    fn fill_prefab(&mut self, flat: &[Flat], entities: &[Entity]) -> Result<()> {
        let mut map = EntityMap::new(Unmapped::Dangling);
        for (i, e) in entities.iter().enumerate() {
            map.insert(doc_ref(i as u64 + 1), *e);
        }
        for (node, e) in flat.iter().zip(entities) {
            self.insert_components_json(*e, &node.components, &map, false)?;
        }
        for (node, e) in flat.iter().zip(entities) {
            if let Some(p) = node.parent {
                self.set_parent(*e, entities[p])?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapEntities, Name};
    use serde_json::json;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Hp(u32);

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Owner(#[serde(with = "crate::entity_serde")] Entity);

    impl MapEntities for Owner {
        fn map_entities(&mut self, map: &EntityMap) {
            self.0 = map.get(self.0);
        }
    }

    fn world() -> World {
        let mut w = World::new();
        w.register::<Hp>("Hp");
        w.register_mapped::<Owner>("Owner");
        w
    }

    const TANK: &str = r#"{
        "components": { "Hp": 100, "Name": "tank" },
        "children": [
            { "components": { "Name": "turret", "Hp": 30, "Owner": 1 } },
            { "components": { "Name": "tread" } }
        ]
    }"#;

    #[test]
    fn instantiate_many_with_overrides() {
        let mut w = world();
        let prefab = Prefab::from_json(TANK).unwrap();
        let plain = w.spawn_prefab(&prefab, &Overrides::new()).unwrap();
        let strong = w
            .spawn_prefab(
                &prefab,
                &Overrides::new()
                    .set("Hp", &Hp(500))
                    .unwrap()
                    .child(0, Overrides::new().set_json("Hp", json!(7))),
            )
            .unwrap();
        assert_ne!(plain, strong);
        assert_eq!(w.get::<&Hp>(plain).unwrap().0, 100);
        assert_eq!(w.get::<&Hp>(strong).unwrap().0, 500);
        let turret = w.children(strong)[0];
        assert_eq!(w.get::<&Hp>(turret).unwrap().0, 7);
        assert_eq!(w.get::<&Name>(turret).unwrap().0, "turret");
        // The reference to entity 1 resolves to this instance's root.
        assert_eq!(*w.get::<&Owner>(turret).unwrap(), Owner(strong));
        let plain_turret = w.children(plain)[0];
        assert_eq!(w.get::<&Hp>(plain_turret).unwrap().0, 30);
        assert_eq!(*w.get::<&Owner>(plain_turret).unwrap(), Owner(plain));
        assert_eq!(w.children(plain).len(), 2);
    }

    #[test]
    fn failed_instantiation_leaves_nothing() {
        let mut w = world();
        let bad =
            Prefab::from_json(r#"{"components":{"Hp":1},"children":[{"components":{"Hp":"x"}}]}"#)
                .unwrap();
        assert!(matches!(
            w.spawn_prefab(&bad, &Overrides::new()),
            Err(Error::Component { .. })
        ));
        let unknown = Prefab::from_json(r#"{"components":{"Nope":1}}"#).unwrap();
        assert!(matches!(
            w.spawn_prefab(&unknown, &Overrides::new()),
            Err(Error::UnknownComponent(_))
        ));
        assert_eq!(w.iter().count(), 0);
    }

    #[test]
    fn capture_round_trips() {
        let mut w = world();
        let prefab = Prefab::from_json(TANK).unwrap();
        let root = w.spawn_prefab(&prefab, &Overrides::new()).unwrap();
        let captured = Prefab::capture(&w, root).unwrap();
        assert_eq!(captured, prefab);
        let reparsed = Prefab::from_json(&captured.to_json().unwrap()).unwrap();
        assert_eq!(reparsed, prefab);
    }
}
