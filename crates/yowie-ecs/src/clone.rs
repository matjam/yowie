//! Cloning entities and subtrees through the component registry.

use hecs::Entity;

use crate::entity_ref::{EntityMap, Unmapped};
use crate::error::{Error, Result};
use crate::world::World;

impl World {
    /// Copies the registered components of `entity` onto a new entity and
    /// returns it.
    ///
    /// The copy has no parent and no children. Components that are not
    /// registered are left out (a warning is logged). References to other
    /// entities inside copied components keep pointing at the originals.
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] when `entity` is missing.
    pub fn clone_entity(&mut self, entity: Entity) -> Result<Entity> {
        if !self.inner.contains(entity) {
            return Err(Error::NoSuchEntity(entity));
        }
        let copy = self.inner.spawn(());
        let mut map = EntityMap::new(Unmapped::Keep);
        map.insert(entity, copy);
        self.copy_components(entity, copy, &map);
        Ok(copy)
    }

    /// Copies `entity` and all its descendants, rebuilding the hierarchy
    /// with the same child order, and returns the copy of `entity`, which
    /// has no parent.
    ///
    /// References between entities of the subtree are rewritten to point at
    /// the copies; references to entities outside it are kept. Components
    /// that are not registered are left out.
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] when `entity` is missing.
    pub fn clone_tree(&mut self, entity: Entity) -> Result<Entity> {
        if !self.inner.contains(entity) {
            return Err(Error::NoSuchEntity(entity));
        }
        let mut nodes = vec![entity];
        nodes.extend(self.descendants(entity));
        let mut map = EntityMap::new(Unmapped::Keep);
        for &old in &nodes {
            let new = self.inner.spawn(());
            map.insert(old, new);
        }
        for &old in &nodes {
            self.copy_components(old, map.get(old), &map);
        }
        for &old in &nodes {
            for child in self.children(old) {
                self.set_parent(map.get(child), map.get(old))?;
            }
        }
        Ok(map.get(entity))
    }

    fn copy_components(&mut self, src: Entity, dst: Entity, map: &EntityMap) {
        let ids: Vec<_> = match self.inner.entity(src) {
            Ok(e) => e.component_types().collect(),
            Err(_) => return,
        };
        for id in ids {
            match self.registry.by_type(id) {
                Some((_, entry)) => entry.clone_to(&mut self.inner, src, dst, map),
                None if self.registry.is_ignored(id) => {}
                None => log::warn!("clone skipped an unregistered component on {src:?}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapEntities, Name};
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Hp(u32);

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Target(#[serde(with = "crate::entity_serde")] Entity);

    impl MapEntities for Target {
        fn map_entities(&mut self, map: &EntityMap) {
            self.0 = map.get(self.0);
        }
    }

    fn world() -> World {
        let mut w = World::new();
        w.register::<Hp>("Hp");
        w.register_mapped::<Target>("Target");
        w
    }

    #[test]
    fn clone_entity_copies_components_only() {
        let mut w = world();
        let other = w.spawn(());
        let kid = w.spawn(());
        let a = w.spawn((Hp(5), Name("a".into()), Target(other), 1.5_f32));
        w.set_parent(kid, a).unwrap();
        let b = w.clone_entity(a).unwrap();
        assert_ne!(a, b);
        assert_eq!(*w.get::<&Hp>(b).unwrap(), Hp(5));
        assert_eq!(*w.get::<&Target>(b).unwrap(), Target(other));
        assert!(w.get::<&f32>(b).is_err());
        assert!(w.children(b).is_empty());
        assert_eq!(w.parent(b), None);
    }

    #[test]
    fn clone_tree_rebuilds_structure_and_remaps_inner_refs() {
        let mut w = world();
        let outside = w.spawn(());
        let root = w.spawn((Hp(1),));
        let c1 = w.spawn((Hp(2),));
        let c2 = w.spawn((Hp(3),));
        w.set_parent(c1, root).unwrap();
        w.set_parent(c2, root).unwrap();
        w.insert_one(c1, Target(c2)).unwrap();
        w.insert_one(c2, Target(outside)).unwrap();

        let copy = w.clone_tree(root).unwrap();
        let kids = w.children(copy);
        assert_eq!(kids.len(), 2);
        assert!(!kids.contains(&c1) && !kids.contains(&c2));
        let hp: Vec<u32> = kids.iter().map(|k| w.get::<&Hp>(*k).unwrap().0).collect();
        assert_eq!(hp, vec![2, 3]);
        assert_eq!(*w.get::<&Target>(kids[0]).unwrap(), Target(kids[1]));
        assert_eq!(*w.get::<&Target>(kids[1]).unwrap(), Target(outside));
        // The original is untouched.
        assert_eq!(*w.get::<&Target>(c1).unwrap(), Target(c2));
        assert_eq!(w.children(root), vec![c1, c2]);
    }

    #[test]
    fn clone_missing_entity_fails() {
        let mut w = world();
        let e = w.spawn(());
        w.despawn(e).unwrap();
        assert!(matches!(w.clone_entity(e), Err(Error::NoSuchEntity(_))));
        assert!(matches!(w.clone_tree(e), Err(Error::NoSuchEntity(_))));
    }
}
