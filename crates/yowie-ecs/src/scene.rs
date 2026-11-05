//! Scene documents: entities, components and hierarchy as JSON.

use std::collections::{BTreeMap, HashMap, HashSet};

use hecs::Entity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entity_ref::{EntityMap, Unmapped, doc_ref};
use crate::error::{Error, Result};
use crate::registry::Name;
use crate::world::World;

/// Version written to and accepted from scene documents.
pub const SCENE_VERSION: u32 = 1;

/// One entity of a [`Scene`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneEntity {
    /// Label that becomes a [`Name`] component and keys
    /// [`SceneInstance::entity`](crate::SceneInstance::entity). Names are
    /// unique within a scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Number of the parent entity, counted from 1 in `Scene::entities`
    /// order. None makes the entity a root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// Name of a prefab in the library. The prefab is spawned first and
    /// each entry of `components` replaces the prefab's component of the
    /// same name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab: Option<String>,
    /// Component values by registered name. An entity reference inside a
    /// value is the number of another entity of the scene (from 1).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, Value>,
}

/// A document of entities that [`World::instantiate_scene`] spawns as a
/// unit. Read one with [`Scene::from_json`], take one from a live world with
/// [`Scene::capture`], and write it with [`Scene::to_json`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// Label for the scene. Nothing in the crate uses it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Free-form values for the game's own use, such as a music track.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Value>,
    /// The entities, numbered from 1 in this order.
    #[serde(default)]
    pub entities: Vec<SceneEntity>,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    /// Documents without a version are read as the current one.
    #[serde(default = "current_version")]
    version: u32,
    #[serde(flatten)]
    scene: Scene,
}

fn current_version() -> u32 {
    SCENE_VERSION
}

impl Scene {
    /// Creates an empty scene with a label.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    /// Appends an entity and returns its number (counted from 1), which
    /// other entities use in `parent` and in entity references.
    pub fn add(&mut self, entity: SceneEntity) -> usize {
        self.entities.push(entity);
        self.entities.len()
    }

    /// Parses a scene document and checks its structure.
    ///
    /// # Errors
    /// [`Error::Json`], [`Error::UnsupportedVersion`] and
    /// [`Error::InvalidDocument`].
    pub fn from_json(json: &str) -> Result<Self> {
        let wire: Wire = serde_json::from_str(json)?;
        if wire.version > SCENE_VERSION {
            return Err(Error::UnsupportedVersion(wire.version));
        }
        wire.scene.validate()?;
        Ok(wire.scene)
    }

    /// Writes the scene as indented JSON after checking its structure.
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        let wire = Wire {
            version: SCENE_VERSION,
            scene: self.clone(),
        };
        Ok(serde_json::to_string_pretty(&wire)?)
    }

    /// Checks that names are unique and parent numbers are in range and
    /// not self-referencing.
    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for (i, e) in self.entities.iter().enumerate() {
            if let Some(n) = &e.name
                && !names.insert(n.as_str())
            {
                return Err(Error::InvalidDocument(format!(
                    "duplicate entity name {n:?}"
                )));
            }
            if let Some(p) = e.parent
                && (p == 0 || p > self.entities.len() || p == i + 1)
            {
                return Err(Error::InvalidDocument(format!(
                    "entity {} has invalid parent {p}",
                    i + 1
                )));
            }
        }
        Ok(())
    }

    /// Captures `roots` and their descendants from a world.
    ///
    /// A [`Name`] component becomes the entity's `name` when it is unique in
    /// the capture. Components that are not registered are left out, and
    /// references to entities outside the capture are lost.
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] for a missing root, plus encoding errors.
    pub fn capture(world: &World, roots: &[Entity]) -> Result<Self> {
        let mut nodes = Vec::new();
        for &r in roots {
            if !world.contains(r) {
                return Err(Error::NoSuchEntity(r));
            }
            nodes.push(r);
            nodes.extend(world.descendants(r));
        }
        let mut map = EntityMap::new(Unmapped::Dangling);
        for (i, e) in nodes.iter().enumerate() {
            map.insert(*e, doc_ref(i as u64 + 1));
        }
        let mut scene = Scene::default();
        let mut used = HashMap::<String, usize>::new();
        for e in &nodes {
            let mut components = world.components_to_json(*e, &map, false)?;
            let mut name = None;
            if let Ok(n) = world.get::<&Name>(*e) {
                let count = used.entry(n.0.clone()).or_default();
                *count += 1;
                if *count == 1 {
                    name = Some(n.0.clone());
                    components.remove("Name");
                }
            }
            let parent = world
                .parent(*e)
                .and_then(|p| nodes.iter().position(|n| *n == p))
                .map(|i| i + 1);
            scene.add(SceneEntity {
                name,
                parent,
                prefab: None,
                components,
            });
        }
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_round_trip_and_validation() {
        let mut s = Scene::new("level");
        s.properties.insert("music".into(), json!("calm"));
        let a = s.add(SceneEntity {
            name: Some("a".into()),
            ..SceneEntity::default()
        });
        s.add(SceneEntity {
            parent: Some(a),
            components: BTreeMap::from([("Hp".into(), json!(3))]),
            ..SceneEntity::default()
        });
        let text = s.to_json().unwrap();
        assert!(text.contains("\"version\": 1"));
        assert_eq!(Scene::from_json(&text).unwrap(), s);
    }

    #[test]
    fn invalid_documents_are_rejected() {
        let cases = [
            r#"{"version":1,"entities":[{"name":"x"},{"name":"x"}]}"#,
            r#"{"version":1,"entities":[{"parent":2}]}"#,
            r#"{"version":1,"entities":[{"parent":0}]}"#,
            r#"{"version":1,"entities":[{"parent":1}]}"#,
        ];
        for doc in cases {
            assert!(
                matches!(Scene::from_json(doc), Err(Error::InvalidDocument(_))),
                "{doc}"
            );
        }
        assert!(matches!(
            Scene::from_json(r#"{"version":2}"#),
            Err(Error::UnsupportedVersion(2))
        ));
        assert!(matches!(Scene::from_json("[]"), Err(Error::Json(_))));
    }
}
