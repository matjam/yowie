//! Saving and loading a whole world as JSON.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use hecs::Entity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entity_ref::{EntityMap, Unmapped, entity_serde};
use crate::error::{Error, Result};
use crate::world::World;

/// Version written to and accepted from save documents.
pub const SAVE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SaveDoc {
    version: u32,
    entities: Vec<SaveEntity>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    resources: BTreeMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct SaveEntity {
    id: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<u64>,
    components: BTreeMap<String, Value>,
}

/// Options for [`World::save`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SaveOptions {
    /// Leave out components that are not registered instead of failing with
    /// [`Error::UnregisteredComponents`].
    pub skip_unregistered: bool,
    /// Write indented, human-readable JSON.
    pub pretty: bool,
}

/// Options for [`World::load`].
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadOptions {
    /// Ignore components and resources whose names are not registered
    /// instead of failing with [`Error::UnknownComponent`] or
    /// [`Error::UnknownResource`].
    pub skip_unknown: bool,
}

impl World {
    /// Writes every entity, its registered components, the hierarchy and the
    /// registered resources as JSON.
    ///
    /// Entities are written under their handle numbers. [`Parent`] and
    /// [`Children`] are written as child lists, and global transforms are
    /// left out because [`propagate_transforms`] rebuilds them.
    ///
    /// # Errors
    /// [`Error::UnregisteredComponents`] when an entity carries an
    /// unregistered component and `skip_unregistered` is off, plus encoding
    /// and I/O errors.
    ///
    /// [`Parent`]: crate::Parent
    /// [`Children`]: crate::Children
    /// [`propagate_transforms`]: crate::propagate_transforms
    pub fn save<W: Write>(&self, out: W, options: &SaveOptions) -> Result<()> {
        let identity = EntityMap::new(Unmapped::Keep);
        let mut entities: Vec<Entity> = self.inner.iter().map(|e| e.entity()).collect();
        entities.sort_by_key(|e| e.to_bits());
        let mut doc = SaveDoc {
            version: SAVE_VERSION,
            entities: Vec::with_capacity(entities.len()),
            resources: BTreeMap::new(),
        };
        for e in entities {
            doc.entities.push(SaveEntity {
                id: e.to_bits().get(),
                children: self.children(e).iter().map(|c| c.to_bits().get()).collect(),
                components: self.components_to_json(e, &identity, !options.skip_unregistered)?,
            });
        }
        for (name, entry) in self.registry.resources() {
            if let Some(value) = entry.save(&self.resources) {
                let value = value.map_err(|source| Error::Component {
                    name: name.to_owned(),
                    source,
                })?;
                doc.resources.insert(name.to_owned(), value);
            }
        }
        if options.pretty {
            serde_json::to_writer_pretty(out, &doc)?;
        } else {
            serde_json::to_writer(out, &doc)?;
        }
        Ok(())
    }

    /// Like [`World::save`], returning the JSON as a string.
    pub fn save_string(&self, options: &SaveOptions) -> Result<String> {
        let mut buf = Vec::new();
        self.save(&mut buf, options)?;
        String::from_utf8(buf).map_err(|e| Error::InvalidDocument(e.to_string()))
    }

    /// Adds the entities and resources of a document written by
    /// [`World::save`] to this world and returns the map from the document's
    /// handles to the new ones.
    ///
    /// Every entity gets a new handle. Entity references inside registered
    /// components are rewritten through [`MapEntities`](crate::MapEntities);
    /// a reference to an entity the document lacks becomes
    /// [`Entity::DANGLING`]. Loaded resources replace existing ones.
    /// Existing entities are untouched. When loading fails, the entities
    /// spawned so far are removed again.
    ///
    /// # Errors
    /// [`Error::UnknownComponent`] or [`Error::UnknownResource`] for names
    /// that are not registered (unless `skip_unknown`),
    /// [`Error::UnsupportedVersion`], and decoding errors.
    pub fn load<R: Read>(&mut self, input: R, options: &LoadOptions) -> Result<EntityMap> {
        let doc: SaveDoc = serde_json::from_reader(input)?;
        if doc.version > SAVE_VERSION {
            return Err(Error::UnsupportedVersion(doc.version));
        }
        for entity in &doc.entities {
            self.check_known(entity.components.keys(), options.skip_unknown)?;
        }
        if !options.skip_unknown
            && let Some(name) = doc
                .resources
                .keys()
                .find(|n| self.registry.resource(n).is_none())
        {
            return Err(Error::UnknownResource(name.clone()));
        }

        let mut map = EntityMap::new(Unmapped::Dangling);
        let mut spawned = Vec::with_capacity(doc.entities.len());
        for e in &doc.entities {
            let new = self.inner.spawn(());
            map.insert(entity_serde::from_number(e.id), new);
            spawned.push(new);
        }
        match self.fill_from_save(&doc, &map, options) {
            Ok(()) => Ok(map),
            Err(err) => {
                for e in spawned {
                    let _ = self.inner.despawn(e);
                }
                Err(err)
            }
        }
    }

    /// Like [`World::load`], reading from a string.
    pub fn load_str(&mut self, json: &str, options: &LoadOptions) -> Result<EntityMap> {
        self.load(json.as_bytes(), options)
    }

    fn fill_from_save(
        &mut self,
        doc: &SaveDoc,
        map: &EntityMap,
        options: &LoadOptions,
    ) -> Result<()> {
        for e in &doc.entities {
            let new = map.get(entity_serde::from_number(e.id));
            self.insert_components_json(new, &e.components, map, options.skip_unknown)?;
        }
        for e in &doc.entities {
            let parent = map.get(entity_serde::from_number(e.id));
            for child in &e.children {
                if let Some(child) = map.lookup(entity_serde::from_number(*child)) {
                    self.set_parent(child, parent)?;
                }
            }
        }
        for (name, value) in &doc.resources {
            match self.registry.resource(name) {
                Some(entry) => {
                    entry
                        .load(&mut self.resources, value.clone())
                        .map_err(|source| Error::Component {
                            name: name.clone(),
                            source,
                        })?
                }
                None => log::warn!("skipping unregistered resource {name:?}"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapEntities, Name};
    use yowie_core::{Transform2D, vec2};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Hp(u32);

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Target(#[serde(with = "crate::entity_serde")] Entity);

    impl MapEntities for Target {
        fn map_entities(&mut self, map: &EntityMap) {
            self.0 = map.get(self.0);
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Score(u32);

    fn world() -> World {
        let mut w = World::new();
        w.register::<Hp>("Hp");
        w.register_mapped::<Target>("Target");
        w.register_resource::<Score>("Score");
        w
    }

    #[test]
    fn round_trip_with_hierarchy_refs_and_resources() {
        let mut src = world();
        // Burn a few slots so old and new handles differ.
        for _ in 0..3 {
            let t = src.spawn(());
            src.despawn(t).unwrap();
        }
        let root = src.spawn((
            Name("root".into()),
            Transform2D::from_position(vec2(1.0, 2.0)),
        ));
        let a = src.spawn((Hp(10),));
        let b = src.spawn((Hp(20),));
        src.set_parent(a, root).unwrap();
        src.set_parent(b, root).unwrap();
        src.insert_one(a, Target(b)).unwrap();
        src.insert_resource(Score(99));
        let json = src.save_string(&SaveOptions::default()).unwrap();

        let mut dst = world();
        dst.spawn((Hp(1),));
        let map = dst.load_str(&json, &LoadOptions::default()).unwrap();
        assert_eq!(map.len(), 3);
        let (root2, a2, b2) = (map.get(root), map.get(a), map.get(b));
        assert_eq!(dst.children(root2), vec![a2, b2]);
        assert_eq!(dst.parent(a2), Some(root2));
        assert_eq!(*dst.get::<&Hp>(b2).unwrap(), Hp(20));
        assert_eq!(*dst.get::<&Target>(a2).unwrap(), Target(b2));
        assert_eq!(dst.get::<&Name>(root2).unwrap().0, "root");
        assert_eq!(
            dst.get::<&Transform2D>(root2).unwrap().position,
            vec2(1.0, 2.0)
        );
        assert_eq!(dst.resource::<Score>(), Some(&Score(99)));
        assert_eq!(dst.iter().count(), 4);
    }

    #[test]
    fn dangling_reference_and_pretty_output() {
        let mut src = world();
        let gone = src.spawn(());
        let a = src.spawn((Target(gone),));
        src.despawn(gone).unwrap();
        let json = src
            .save_string(&SaveOptions {
                pretty: true,
                ..SaveOptions::default()
            })
            .unwrap();
        assert!(json.contains('\n'));
        let mut dst = world();
        let map = dst.load_str(&json, &LoadOptions::default()).unwrap();
        assert_eq!(dst.get::<&Target>(map.get(a)).unwrap().0, Entity::DANGLING);
    }

    #[test]
    fn unregistered_and_unknown_handling() {
        let mut src = world();
        src.spawn((Hp(1), 2.5_f32));
        assert!(matches!(
            src.save_string(&SaveOptions::default()),
            Err(Error::UnregisteredComponents { .. })
        ));
        let lax = SaveOptions {
            skip_unregistered: true,
            ..SaveOptions::default()
        };
        let json = src.save_string(&lax).unwrap();

        let mut bare = World::new();
        assert!(matches!(
            bare.load_str(&json, &LoadOptions::default()),
            Err(Error::UnknownComponent(_))
        ));
        assert_eq!(bare.iter().count(), 0);
        bare.load_str(&json, &LoadOptions { skip_unknown: true })
            .unwrap();
        assert_eq!(bare.iter().count(), 1);
    }

    #[test]
    fn bad_value_rolls_back_and_future_version_is_rejected() {
        let mut w = world();
        let bad = r#"{"version":1,"entities":[{"id":4294967297,"components":{"Hp":"x"}}]}"#;
        assert!(matches!(
            w.load_str(bad, &LoadOptions::default()),
            Err(Error::Component { .. })
        ));
        assert_eq!(w.iter().count(), 0);
        let future = r#"{"version":9,"entities":[]}"#;
        assert!(matches!(
            w.load_str(future, &LoadOptions::default()),
            Err(Error::UnsupportedVersion(9))
        ));
    }
}
