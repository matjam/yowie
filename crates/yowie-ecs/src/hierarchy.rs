//! Parent and child links between entities.

use hecs::Entity;

use crate::error::{Error, Result};
use crate::world::World;

/// Component on an entity that has a parent. Maintained by
/// [`World::set_parent`] and [`World::detach`]; do not insert it directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub Entity);

/// Component on an entity that has children, in attach order. Maintained by
/// [`World::set_parent`] and [`World::detach`]; do not insert it directly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Children(pub Vec<Entity>);

impl World {
    /// Returns the parent of `entity`, if it has one.
    pub fn parent(&self, entity: Entity) -> Option<Entity> {
        self.inner.get::<&Parent>(entity).ok().map(|p| p.0)
    }

    /// Returns the children of `entity` in attach order. The list is empty
    /// for a missing entity or one without children.
    pub fn children(&self, entity: Entity) -> Vec<Entity> {
        self.inner
            .get::<&Children>(entity)
            .map(|c| c.0.clone())
            .unwrap_or_default()
    }

    /// Returns every descendant of `entity`, parents before children,
    /// not including `entity` itself.
    pub fn descendants(&self, entity: Entity) -> Vec<Entity> {
        let mut out = Vec::new();
        let mut stack = self.children(entity);
        stack.reverse();
        while let Some(e) = stack.pop() {
            out.push(e);
            let mut kids = self.children(e);
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    /// Makes `child` the last child of `parent`, detaching it from its
    /// current parent first.
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] when either entity is missing, and
    /// [`Error::HierarchyCycle`] when `parent` is `child` or one of its
    /// descendants.
    pub fn set_parent(&mut self, child: Entity, parent: Entity) -> Result<()> {
        for e in [child, parent] {
            if !self.inner.contains(e) {
                return Err(Error::NoSuchEntity(e));
            }
        }
        let mut ancestor = Some(parent);
        while let Some(a) = ancestor {
            if a == child {
                return Err(Error::HierarchyCycle { child, parent });
            }
            ancestor = self.parent(a);
        }
        self.detach(child)?;
        let existing = self.inner.get::<&mut Children>(parent).map(|mut c| {
            c.0.push(child);
        });
        if existing.is_err() {
            self.inner
                .insert_one(parent, Children(vec![child]))
                .map_err(|_| Error::NoSuchEntity(parent))?;
        }
        self.inner
            .insert_one(child, Parent(parent))
            .map_err(|_| Error::NoSuchEntity(child))
    }

    /// Removes `child` from its parent, leaving it a root. Does nothing for
    /// an entity without a parent.
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] when `child` is missing.
    pub fn detach(&mut self, child: Entity) -> Result<()> {
        if !self.inner.contains(child) {
            return Err(Error::NoSuchEntity(child));
        }
        let Some(parent) = self.parent(child) else {
            return Ok(());
        };
        let _ = self.inner.remove_one::<Parent>(child);
        let now_empty = match self.inner.get::<&mut Children>(parent) {
            Ok(mut c) => {
                c.0.retain(|e| *e != child);
                c.0.is_empty()
            }
            Err(_) => false,
        };
        if now_empty {
            let _ = self.inner.remove_one::<Children>(parent);
        }
        Ok(())
    }

    /// Despawns `entity` and all its descendants and removes it from its
    /// parent's children. This shadows [`hecs::World::despawn`].
    ///
    /// # Errors
    /// [`Error::NoSuchEntity`] when `entity` is missing.
    pub fn despawn(&mut self, entity: Entity) -> Result<()> {
        self.detach(entity)?;
        let doomed = self.descendants(entity);
        for e in doomed.into_iter().rev().chain([entity]) {
            let _ = self.inner.despawn(e);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (World, [Entity; 4]) {
        let mut w = World::new();
        let root = w.spawn(("root",));
        let a = w.spawn(("a",));
        let b = w.spawn(("b",));
        let c = w.spawn(("c",));
        w.set_parent(a, root).unwrap();
        w.set_parent(b, root).unwrap();
        w.set_parent(c, a).unwrap();
        (w, [root, a, b, c])
    }

    #[test]
    fn links_and_order() {
        let (w, [root, a, b, c]) = tree();
        assert_eq!(w.children(root), vec![a, b]);
        assert_eq!(w.parent(c), Some(a));
        assert_eq!(w.descendants(root), vec![a, c, b]);
        assert_eq!(w.parent(root), None);
    }

    #[test]
    fn reparent_and_detach() {
        let (mut w, [root, a, b, c]) = tree();
        w.set_parent(c, b).unwrap();
        assert!(w.get::<&Children>(a).is_err());
        assert_eq!(w.children(b), vec![c]);
        w.detach(c).unwrap();
        assert_eq!(w.parent(c), None);
        assert_eq!(w.children(root), vec![a, b]);
    }

    #[test]
    fn cycles_and_missing_entities_are_errors() {
        let (mut w, [root, a, _, c]) = tree();
        assert!(matches!(
            w.set_parent(root, c),
            Err(Error::HierarchyCycle { .. })
        ));
        assert!(matches!(
            w.set_parent(a, a),
            Err(Error::HierarchyCycle { .. })
        ));
        let dead = w.spawn(());
        w.despawn(dead).unwrap();
        assert!(matches!(
            w.set_parent(dead, root),
            Err(Error::NoSuchEntity(_))
        ));
        assert!(matches!(w.despawn(dead), Err(Error::NoSuchEntity(_))));
    }

    #[test]
    fn despawn_is_recursive_and_unlinks() {
        let (mut w, [root, a, b, c]) = tree();
        w.despawn(a).unwrap();
        assert!(!w.contains(a) && !w.contains(c));
        assert!(w.contains(b) && w.contains(root));
        assert_eq!(w.children(root), vec![b]);
    }
}
