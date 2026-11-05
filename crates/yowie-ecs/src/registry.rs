//! Registry of component and resource types, by name.
//!
//! Saving, cloning, prefabs and scenes all work on registered types: the
//! registry knows how to encode a component to JSON, decode one, copy one
//! between entities, and rewrite the entity handles inside it.

use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};

use hecs::{Component, Entity};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use yowie_core::{Transform, Transform2D};

use crate::entity_ref::{EntityMap, MapEntities};
use crate::error::{Error, Result};
use crate::hierarchy::{Children, Parent};
use crate::resources::Resources;
use crate::transform::{GlobalTransform, GlobalTransform2D};
use crate::world::World;

/// Display name of an entity. Scenes give every named entity one, and
/// [`SceneInstance::entity`](crate::SceneInstance::entity) looks entities up
/// by it. Registered in every new world as `"Name"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Name(pub String);

pub(crate) trait ErasedComponent {
    /// Encodes the entity's component, or None when it lacks one.
    fn to_json(
        &self,
        world: &hecs::World,
        entity: Entity,
        map: &EntityMap,
    ) -> Option<serde_json::Result<Value>>;
    /// Decodes `value`, maps its entity references and inserts it.
    fn insert_json(
        &self,
        world: &mut hecs::World,
        entity: Entity,
        value: Value,
        map: &EntityMap,
    ) -> serde_json::Result<()>;
    /// Copies the component from `src` to `dst`, mapping references.
    fn clone_to(&self, world: &mut hecs::World, src: Entity, dst: Entity, map: &EntityMap);
}

struct Typed<T> {
    map_fn: fn(&mut T, &EntityMap),
}

impl<T: Component + Serialize + DeserializeOwned + Clone> ErasedComponent for Typed<T> {
    fn to_json(
        &self,
        world: &hecs::World,
        entity: Entity,
        map: &EntityMap,
    ) -> Option<serde_json::Result<Value>> {
        let mut value: T = (*world.get::<&T>(entity).ok()?).clone();
        (self.map_fn)(&mut value, map);
        Some(serde_json::to_value(&value))
    }

    fn insert_json(
        &self,
        world: &mut hecs::World,
        entity: Entity,
        value: Value,
        map: &EntityMap,
    ) -> serde_json::Result<()> {
        let mut value: T = serde_json::from_value(value)?;
        (self.map_fn)(&mut value, map);
        let _ = world.insert_one(entity, value);
        Ok(())
    }

    fn clone_to(&self, world: &mut hecs::World, src: Entity, dst: Entity, map: &EntityMap) {
        let Some(mut value) = world.get::<&T>(src).ok().map(|c| (*c).clone()) else {
            return;
        };
        (self.map_fn)(&mut value, map);
        let _ = world.insert_one(dst, value);
    }
}

pub(crate) trait ErasedResource {
    fn save(&self, resources: &Resources) -> Option<serde_json::Result<Value>>;
    fn load(&self, resources: &mut Resources, value: Value) -> serde_json::Result<()>;
}

struct TypedResource<T>(std::marker::PhantomData<T>);

impl<T: Serialize + DeserializeOwned + 'static> ErasedResource for TypedResource<T> {
    fn save(&self, resources: &Resources) -> Option<serde_json::Result<Value>> {
        Some(serde_json::to_value(resources.get::<T>()?))
    }
    fn load(&self, resources: &mut Resources, value: Value) -> serde_json::Result<()> {
        resources.insert(serde_json::from_value::<T>(value)?);
        Ok(())
    }
}

/// Registered component and resource types. Owned by [`World`].
#[derive(Default)]
pub(crate) struct ComponentRegistry {
    components: HashMap<TypeId, Box<dyn ErasedComponent>>,
    names: BTreeMap<String, TypeId>,
    /// Types that exist in saved worlds only as derived or structural data.
    ignored: HashSet<TypeId>,
    resources: BTreeMap<String, Box<dyn ErasedResource>>,
}

impl ComponentRegistry {
    fn add(&mut self, name: &str, id: TypeId, entry: Box<dyn ErasedComponent>) {
        self.names.retain(|_, t| *t != id);
        self.names.insert(name.to_owned(), id);
        self.components.insert(id, entry);
    }

    pub(crate) fn by_name(&self, name: &str) -> Option<&dyn ErasedComponent> {
        let id = self.names.get(name)?;
        self.components.get(id).map(|b| b.as_ref())
    }

    pub(crate) fn by_type(&self, id: TypeId) -> Option<(&str, &dyn ErasedComponent)> {
        let (name, _) = self.names.iter().find(|(_, t)| **t == id)?;
        Some((name.as_str(), self.components.get(&id)?.as_ref()))
    }

    pub(crate) fn is_ignored(&self, id: TypeId) -> bool {
        self.ignored.contains(&id)
    }

    pub(crate) fn resources(&self) -> impl Iterator<Item = (&str, &dyn ErasedResource)> {
        self.resources.iter().map(|(n, r)| (n.as_str(), r.as_ref()))
    }

    pub(crate) fn resource(&self, name: &str) -> Option<&dyn ErasedResource> {
        self.resources.get(name).map(|r| r.as_ref())
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.keys().map(String::as_str)
    }
}

impl World {
    pub(crate) fn register_builtins(&mut self) {
        self.register::<Transform>("Transform");
        self.register::<Transform2D>("Transform2D");
        self.register::<Name>("Name");
        let ignored = [
            TypeId::of::<Parent>(),
            TypeId::of::<Children>(),
            TypeId::of::<GlobalTransform>(),
            TypeId::of::<GlobalTransform2D>(),
        ];
        self.registry.ignored.extend(ignored);
    }

    /// Registers component type `T` under `name` for saves, cloning, prefabs
    /// and scenes. Registering a type again renames it; registering another
    /// type under a used name takes the name over.
    ///
    /// `T` must not hold entity handles; use
    /// [`World::register_mapped`] for types that do.
    pub fn register<T>(&mut self, name: &str)
    where
        T: Component + Serialize + DeserializeOwned + Clone,
    {
        let entry = Typed::<T> { map_fn: |_, _| {} };
        self.registry.add(name, TypeId::of::<T>(), Box::new(entry));
    }

    /// Registers component type `T` like [`World::register`], and rewrites
    /// the entity handles inside it through [`MapEntities`] whenever it is
    /// loaded, cloned or instantiated.
    pub fn register_mapped<T>(&mut self, name: &str)
    where
        T: Component + Serialize + DeserializeOwned + Clone + MapEntities,
    {
        let entry = Typed::<T> {
            map_fn: T::map_entities,
        };
        self.registry.add(name, TypeId::of::<T>(), Box::new(entry));
    }

    /// Marks component type `T` as transient: saves and clones skip it
    /// without complaint. [`Parent`], [`Children`] and the global
    /// transforms are already marked.
    pub fn ignore_component<T: Component>(&mut self) {
        self.registry.ignored.insert(TypeId::of::<T>());
    }

    /// Registers resource type `T` under `name` so [`World::save`] writes it
    /// and [`World::load`] restores it.
    pub fn register_resource<T>(&mut self, name: &str)
    where
        T: Serialize + DeserializeOwned + 'static,
    {
        self.registry.resources.insert(
            name.to_owned(),
            Box::new(TypedResource::<T>(std::marker::PhantomData)),
        );
    }

    /// Returns the registered component names in alphabetical order.
    pub fn registered_components(&self) -> Vec<&str> {
        self.registry.names().collect()
    }

    /// Encodes every registered component of `entity`, keyed by name.
    /// References are rewritten through `map`. With `strict`, a component
    /// that is neither registered nor ignored is an error; otherwise it is
    /// skipped.
    pub(crate) fn components_to_json(
        &self,
        entity: Entity,
        map: &EntityMap,
        strict: bool,
    ) -> Result<BTreeMap<String, Value>> {
        let eref = self
            .inner
            .entity(entity)
            .map_err(|_| Error::NoSuchEntity(entity))?;
        let mut out = BTreeMap::new();
        let mut unregistered = 0;
        for id in eref.component_types() {
            match self.registry.by_type(id) {
                Some((name, entry)) => {
                    if let Some(res) = entry.to_json(&self.inner, entity, map) {
                        let value = res.map_err(|source| Error::Component {
                            name: name.to_owned(),
                            source,
                        })?;
                        out.insert(name.to_owned(), value);
                    }
                }
                None if self.registry.is_ignored(id) => {}
                None => unregistered += 1,
            }
        }
        if unregistered > 0 && strict {
            return Err(Error::UnregisteredComponents {
                entity,
                count: unregistered,
            });
        }
        Ok(out)
    }

    /// Checks that every name in `names` is registered, unless `skip`.
    pub(crate) fn check_known<'a>(
        &self,
        names: impl IntoIterator<Item = &'a String>,
        skip: bool,
    ) -> Result<()> {
        if skip {
            return Ok(());
        }
        match names
            .into_iter()
            .find(|n| self.registry.by_name(n).is_none())
        {
            Some(name) => Err(Error::UnknownComponent(name.clone())),
            None => Ok(()),
        }
    }

    /// Decodes and inserts the named components on `entity`. Unknown names
    /// are skipped when `skip_unknown`, an error otherwise.
    pub(crate) fn insert_components_json(
        &mut self,
        entity: Entity,
        components: &BTreeMap<String, Value>,
        map: &EntityMap,
        skip_unknown: bool,
    ) -> Result<()> {
        for (name, value) in components {
            let Some(entry) = self.registry.by_name(name) else {
                if skip_unknown {
                    log::warn!("skipping unregistered component {name:?}");
                    continue;
                }
                return Err(Error::UnknownComponent(name.clone()));
            };
            entry
                .insert_json(&mut self.inner, entity, value.clone(), map)
                .map_err(|source| Error::Component {
                    name: name.clone(),
                    source,
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Hp(u32);

    #[test]
    fn builtins_and_rename() {
        let mut w = World::new();
        assert_eq!(
            w.registered_components(),
            vec!["Name", "Transform", "Transform2D"]
        );
        w.register::<Hp>("Hp");
        w.register::<Hp>("Health");
        assert_eq!(
            w.registered_components(),
            vec!["Health", "Name", "Transform", "Transform2D"]
        );
    }

    #[test]
    fn strict_json_reports_unregistered() {
        let mut w = World::new();
        w.register::<Hp>("Hp");
        let e = w.spawn((Hp(3), 1.5_f32));
        let map = EntityMap::new(crate::Unmapped::Keep);
        assert!(matches!(
            w.components_to_json(e, &map, true),
            Err(Error::UnregisteredComponents { count: 1, .. })
        ));
        let lax = w.components_to_json(e, &map, false).unwrap();
        assert_eq!(lax["Hp"], serde_json::json!(3));
    }

    #[test]
    fn unknown_names_error_unless_skipped() {
        let mut w = World::new();
        w.register::<Hp>("Hp");
        let e = w.spawn(());
        let comps = BTreeMap::from([
            ("Hp".to_owned(), serde_json::json!(9)),
            ("X".to_owned(), Value::Null),
        ]);
        let map = EntityMap::new(crate::Unmapped::Keep);
        assert!(matches!(
            w.insert_components_json(e, &comps, &map, false),
            Err(Error::UnknownComponent(_))
        ));
        w.insert_components_json(e, &comps, &map, true).unwrap();
        assert_eq!(*w.get::<&Hp>(e).unwrap(), Hp(9));
    }
}
