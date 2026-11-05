//! Deferred world changes applied at sync points.

use hecs::{Bundle, DynamicBundle, Entity};

use crate::world::World;

type Command = Box<dyn FnOnce(&mut World)>;

/// A queue of world changes recorded now and applied later.
///
/// Use it while a query borrows the world: record spawns, despawns, inserts
/// and removals, end the borrow, then call [`Commands::apply`]. The world
/// also owns a queue, reached with [`World::commands`], that the
/// [`Schedule`](crate::Schedule) applies after every system.
#[derive(Default)]
pub struct Commands {
    queue: Vec<Command>,
}

impl Commands {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a spawn and returns the handle the entity will have.
    ///
    /// The handle is reserved now, so it can be used in later commands of
    /// this queue (for example [`Commands::set_parent`]). The entity does
    /// not exist for queries until the queue is applied.
    pub fn spawn(&mut self, world: &World, bundle: impl DynamicBundle + 'static) -> Entity {
        let entity = world.reserve_entity();
        self.queue.push(Box::new(move |w| {
            w.spawn_at(entity, bundle);
        }));
        entity
    }

    /// Queues a recursive despawn. A missing entity is skipped.
    pub fn despawn(&mut self, entity: Entity) {
        self.queue.push(Box::new(move |w| {
            let _ = w.despawn(entity);
        }));
    }

    /// Queues adding the components of `bundle`, replacing existing ones. A
    /// missing entity is skipped.
    pub fn insert(&mut self, entity: Entity, bundle: impl DynamicBundle + 'static) {
        self.queue.push(Box::new(move |w| {
            let _ = w.insert(entity, bundle);
        }));
    }

    /// Queues removing the components of bundle type `B`. A missing entity
    /// or component is skipped.
    pub fn remove<B: Bundle + 'static>(&mut self, entity: Entity) {
        self.queue.push(Box::new(move |w| {
            let _ = w.remove::<B>(entity);
        }));
    }

    /// Queues [`World::set_parent`]. Errors are logged and skipped.
    pub fn set_parent(&mut self, child: Entity, parent: Entity) {
        self.queue.push(Box::new(move |w| {
            if let Err(e) = w.set_parent(child, parent) {
                log::warn!("deferred set_parent failed: {e}");
            }
        }));
    }

    /// Queues an arbitrary change.
    pub fn run(&mut self, f: impl FnOnce(&mut World) + 'static) {
        self.queue.push(Box::new(f));
    }

    /// Number of queued changes.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Applies every queued change in order and empties the queue.
    pub fn apply(&mut self, world: &mut World) {
        for command in self.queue.drain(..) {
            command(world);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_apply_in_order() {
        let mut world = World::new();
        let keep = world.spawn((1_i32,));
        let gone = world.spawn((2_i32,));
        let mut cmds = Commands::new();
        let fresh = cmds.spawn(&world, (3_i32,));
        cmds.despawn(gone);
        cmds.insert(keep, (1.5_f32,));
        cmds.set_parent(fresh, keep);
        assert_eq!(cmds.len(), 4);
        assert!(world.get::<&i32>(fresh).is_err());
        cmds.apply(&mut world);
        assert!(cmds.is_empty());
        assert!(!world.contains(gone));
        assert_eq!(*world.get::<&i32>(fresh).unwrap(), 3);
        assert_eq!(*world.get::<&f32>(keep).unwrap(), 1.5);
        assert_eq!(world.parent(fresh), Some(keep));
    }

    #[test]
    fn remove_skips_missing() {
        let mut world = World::new();
        let e = world.spawn((1_i32,));
        let mut cmds = Commands::new();
        cmds.remove::<(f32,)>(e);
        cmds.remove::<(i32,)>(e);
        cmds.apply(&mut world);
        assert!(world.get::<&i32>(e).is_err());
    }
}
