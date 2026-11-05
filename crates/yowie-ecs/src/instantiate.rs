//! Spawning scenes into a world.

use std::collections::HashMap;

use hecs::Entity;
use serde_json::Value;

use crate::entity_ref::{EntityMap, Unmapped, doc_ref};
use crate::error::{Error, Result};
use crate::prefab::{Overrides, PrefabLibrary};
use crate::registry::Name;
use crate::scene::Scene;
use crate::world::World;

/// Options for [`World::instantiate_scene`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneOptions<'a> {
    /// Resolves the scene's prefab references. Without a library, a scene
    /// that names a prefab fails with [`Error::UnknownPrefab`].
    pub prefabs: Option<&'a PrefabLibrary>,
    /// Hangs every root of the scene under this entity.
    pub parent: Option<Entity>,
    /// Ignore component names that are not registered instead of failing.
    pub skip_unknown: bool,
}

/// One spawned copy of a scene. Keep it to find entities by name and to
/// remove the copy again.
#[derive(Clone, Debug, Default)]
pub struct SceneInstance {
    /// Entities of the scene that have no parent, in scene order.
    pub roots: Vec<Entity>,
    /// Every entity the copy created, in scene order. A prefab's children
    /// follow the scene entity that referenced it.
    pub spawned: Vec<Entity>,
    named: HashMap<String, Entity>,
}

impl SceneInstance {
    /// Returns the entity of this copy that the scene gave `name`.
    pub fn entity(&self, name: &str) -> Option<Entity> {
        self.named.get(name).copied()
    }

    /// Despawns everything the copy created, including children gained
    /// since, and empties the instance. Other copies are untouched.
    pub fn despawn(&mut self, world: &mut World) {
        for e in std::mem::take(&mut self.roots) {
            let _ = world.despawn(e);
        }
        for e in std::mem::take(&mut self.spawned) {
            let _ = world.despawn(e);
        }
        self.named.clear();
    }
}

impl World {
    /// Spawns a copy of `scene` and returns what it created.
    ///
    /// Every call creates new entities, so several copies live side by
    /// side. Entity references in component values are scene numbers and
    /// become the spawned entities; a number the scene does not use becomes
    /// [`Entity::DANGLING`]. Named entities get a [`Name`] component.
    /// Nothing is left in the world when the call fails.
    ///
    /// # Errors
    /// [`Error::InvalidDocument`], [`Error::UnknownPrefab`],
    /// [`Error::UnknownComponent`], [`Error::HierarchyCycle`] and decoding
    /// errors.
    pub fn instantiate_scene(
        &mut self,
        scene: &Scene,
        options: &SceneOptions<'_>,
    ) -> Result<SceneInstance> {
        scene.validate()?;
        for e in &scene.entities {
            self.check_known(e.components.keys(), options.skip_unknown)?;
            if let Some(p) = &e.prefab
                && options.prefabs.and_then(|l| l.get(p)).is_none()
            {
                return Err(Error::UnknownPrefab(p.clone()));
            }
        }
        let mut instance = SceneInstance::default();
        match self.spawn_scene(scene, options, &mut instance) {
            Ok(()) => Ok(instance),
            Err(err) => {
                for e in instance.spawned {
                    let _ = self.inner.despawn(e);
                }
                Err(err)
            }
        }
    }

    fn spawn_scene(
        &mut self,
        scene: &Scene,
        options: &SceneOptions<'_>,
        instance: &mut SceneInstance,
    ) -> Result<()> {
        let mut map = EntityMap::new(Unmapped::Dangling);
        let mut entities = Vec::with_capacity(scene.entities.len());
        for (i, se) in scene.entities.iter().enumerate() {
            let prefab = se.prefab.as_deref().and_then(|p| options.prefabs?.get(p));
            let entity = match prefab {
                Some(p) => self.spawn_prefab(p, &Overrides::default())?,
                None => self.inner.spawn(()),
            };
            instance.spawned.push(entity);
            instance.spawned.extend(self.descendants(entity));
            map.insert(doc_ref(i as u64 + 1), entity);
            entities.push(entity);
        }
        for (se, entity) in scene.entities.iter().zip(&entities) {
            let mut components = se.components.clone();
            if let Some(name) = &se.name {
                components.insert("Name".into(), Value::String(name.clone()));
                instance.named.insert(name.clone(), *entity);
            }
            self.insert_components_json(*entity, &components, &map, options.skip_unknown)?;
        }
        for (se, entity) in scene.entities.iter().zip(&entities) {
            match se.parent {
                Some(p) => self.set_parent(*entity, entities[p - 1])?,
                None => {
                    if let Some(outer) = options.parent {
                        self.set_parent(*entity, outer)?;
                    }
                    instance.roots.push(*entity);
                }
            }
        }
        Ok(())
    }

    /// Returns the [`Name`] of an entity.
    pub fn name_of(&self, entity: Entity) -> Option<String> {
        self.get::<&Name>(entity).ok().map(|n| n.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapEntities, Prefab};
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Hp(u32);

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Link(#[serde(with = "crate::entity_serde")] Entity);

    impl MapEntities for Link {
        fn map_entities(&mut self, map: &EntityMap) {
            self.0 = map.get(self.0);
        }
    }

    fn world() -> World {
        let mut w = World::new();
        w.register::<Hp>("Hp");
        w.register_mapped::<Link>("Link");
        w
    }

    const SCENE: &str = r#"{
        "version": 1, "name": "level",
        "properties": {"music": "calm"},
        "entities": [
            {"name": "door", "components": {"Hp": 5, "Link": 2}},
            {"name": "key", "parent": 1, "components": {"Link": 99}},
            {"name": "guard", "prefab": "soldier", "components": {"Hp": 40}}
        ]
    }"#;

    fn library() -> PrefabLibrary {
        let mut lib = PrefabLibrary::new();
        lib.insert(
            "soldier",
            Prefab::from_json(
                r#"{"components":{"Hp":10},"children":[{"components":{"Name":"sword"}}]}"#,
            )
            .unwrap(),
        );
        lib
    }

    #[test]
    fn instantiate_twice_and_despawn_one() {
        let mut w = world();
        let scene = Scene::from_json(SCENE).unwrap();
        let lib = library();
        let opts = SceneOptions {
            prefabs: Some(&lib),
            ..SceneOptions::default()
        };
        let mut one = w.instantiate_scene(&scene, &opts).unwrap();
        let two = w.instantiate_scene(&scene, &opts).unwrap();
        let door = one.entity("door").unwrap();
        let key = one.entity("key").unwrap();
        assert_ne!(door, two.entity("door").unwrap());
        assert_eq!(*w.get::<&Link>(door).unwrap(), Link(key));
        assert_eq!(w.get::<&Link>(key).unwrap().0, Entity::DANGLING);
        assert_eq!(w.parent(key), Some(door));
        assert_eq!(w.name_of(door).as_deref(), Some("door"));
        let guard = one.entity("guard").unwrap();
        assert_eq!(w.get::<&Hp>(guard).unwrap().0, 40);
        assert_eq!(w.children(guard).len(), 1);
        assert_eq!(one.roots, vec![door, guard]);
        assert_eq!(one.spawned.len(), 4);

        one.despawn(&mut w);
        assert!(!w.contains(door) && !w.contains(guard));
        assert!(w.contains(two.entity("door").unwrap()));
        assert_eq!(w.iter().count(), 4);
    }

    #[test]
    fn parent_option_and_failure_rollback() {
        let mut w = world();
        let holder = w.spawn(());
        let scene = Scene::from_json(SCENE).unwrap();
        let lib = library();
        let ok = SceneOptions {
            prefabs: Some(&lib),
            parent: Some(holder),
            skip_unknown: false,
        };
        let inst = w.instantiate_scene(&scene, &ok).unwrap();
        assert_eq!(w.children(holder), inst.roots);

        let before = w.iter().count();
        assert!(matches!(
            w.instantiate_scene(&scene, &SceneOptions::default()),
            Err(Error::UnknownPrefab(_))
        ));
        let bad = Scene::from_json(r#"{"entities":[{"components":{"Hp":"x"}}]}"#).unwrap();
        assert!(matches!(
            w.instantiate_scene(&bad, &SceneOptions::default()),
            Err(Error::Component { .. })
        ));
        assert_eq!(w.iter().count(), before);
    }

    #[test]
    fn capture_then_instantiate_reproduces_the_tree() {
        let mut w = world();
        let scene = Scene::from_json(SCENE).unwrap();
        let lib = library();
        let opts = SceneOptions {
            prefabs: Some(&lib),
            ..SceneOptions::default()
        };
        let inst = w.instantiate_scene(&scene, &opts).unwrap();
        let captured = Scene::capture(&w, &inst.roots).unwrap();
        assert_eq!(captured.entities.len(), 4);
        assert_eq!(captured.entities[0].name.as_deref(), Some("door"));
        assert_eq!(captured.entities[1].parent, Some(1));
        let copy = w
            .instantiate_scene(
                &Scene::from_json(&captured.to_json().unwrap()).unwrap(),
                &SceneOptions::default(),
            )
            .unwrap();
        let door = copy.entity("door").unwrap();
        let key = copy.entity("key").unwrap();
        assert_eq!(*w.get::<&Link>(door).unwrap(), Link(key));
        assert_eq!(w.children(copy.entity("guard").unwrap()).len(), 1);
    }
}
