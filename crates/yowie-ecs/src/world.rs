//! The [`World`]: entity storage plus resources, events and deferred commands.

use std::cell::{RefCell, RefMut};
use std::ops::{Deref, DerefMut};

use crate::commands::Commands;
use crate::events::{EventQueues, Events};
use crate::registry::ComponentRegistry;
use crate::resources::Resources;

/// The game state container.
///
/// A `World` wraps a [`hecs::World`] and dereferences to it, so every hecs
/// method is available directly: `spawn`, `get`, `query`, `query_mut`,
/// `insert`, `remove`, `iter` and so on. It adds typed resources, events,
/// a deferred [`Commands`] queue, the entity hierarchy and a component
/// registry for saves, cloning, prefabs and scenes.
///
/// [`World::despawn`] shadows the hecs method: it also despawns the
/// descendants of the entity and removes it from its parent's children.
pub struct World {
    pub(crate) inner: hecs::World,
    pub(crate) resources: Resources,
    pub(crate) events: EventQueues,
    pub(crate) registry: ComponentRegistry,
    commands: RefCell<Commands>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for World {
    type Target = hecs::World;
    fn deref(&self) -> &hecs::World {
        &self.inner
    }
}

impl DerefMut for World {
    fn deref_mut(&mut self) -> &mut hecs::World {
        &mut self.inner
    }
}

impl World {
    /// Creates an empty world. The registry starts with [`Transform`],
    /// [`Transform2D`] and [`Name`](crate::Name) registered.
    ///
    /// [`Transform`]: yowie_core::Transform
    /// [`Transform2D`]: yowie_core::Transform2D
    pub fn new() -> Self {
        let mut world = Self {
            inner: hecs::World::new(),
            resources: Resources::default(),
            events: EventQueues::default(),
            registry: ComponentRegistry::default(),
            commands: RefCell::new(Commands::new()),
        };
        world.register_builtins();
        world
    }

    /// Stores `value` as the resource of type `T`, returning the previous
    /// value if there was one.
    pub fn insert_resource<T: 'static>(&mut self, value: T) -> Option<T> {
        self.resources.insert(value)
    }

    /// Returns the resource of type `T`.
    pub fn resource<T: 'static>(&self) -> Option<&T> {
        self.resources.get()
    }

    /// Returns the resource of type `T` mutably.
    pub fn resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut()
    }

    /// Removes and returns the resource of type `T`.
    pub fn remove_resource<T: 'static>(&mut self) -> Option<T> {
        self.resources.remove()
    }

    /// Returns true when a resource of type `T` is stored.
    pub fn has_resource<T: 'static>(&self) -> bool {
        self.resources.get::<T>().is_some()
    }

    /// Sends an event. It is readable during this update and the next.
    pub fn send_event<T: 'static>(&mut self, event: T) {
        self.events.get_or_create::<T>().send(event);
    }

    /// Iterates over the readable events of type `T`: those sent in the
    /// previous update first, then those sent in this one.
    pub fn read_events<T: 'static>(&self) -> impl Iterator<Item = &T> {
        self.events.get::<T>().into_iter().flat_map(Events::iter)
    }

    /// Returns the event queue of type `T`, if any event was ever sent.
    pub fn events<T: 'static>(&self) -> Option<&Events<T>> {
        self.events.get()
    }

    /// Drops all events of type `T`, including ones not yet read.
    pub fn clear_events<T: 'static>(&mut self) {
        self.events.get_or_create::<T>().clear();
    }

    /// Ends the current update for events: events sent two updates ago are
    /// dropped and the rest become the previous update's. The
    /// [`Schedule`](crate::Schedule) calls this after its last stage; call
    /// it yourself when you drive systems by hand.
    pub fn update_events(&mut self) {
        self.events.rotate_all();
    }

    /// Returns the world's own deferred command queue.
    ///
    /// It can be filled while the world is borrowed by a query. The
    /// [`Schedule`](crate::Schedule) applies it after every system; apply it
    /// by hand with [`World::flush_commands`].
    pub fn commands(&self) -> RefMut<'_, Commands> {
        self.commands.borrow_mut()
    }

    /// Applies the commands queued in [`World::commands`]. Commands queued
    /// while this runs wait for the next flush.
    pub fn flush_commands(&mut self) {
        let mut pending = std::mem::take(&mut *self.commands.borrow_mut());
        pending.apply(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_roundtrip() {
        let mut w = World::new();
        assert!(!w.has_resource::<String>());
        w.insert_resource(String::from("a"));
        w.resource_mut::<String>().unwrap().push('b');
        assert_eq!(w.resource::<String>().map(String::as_str), Some("ab"));
        assert_eq!(w.remove_resource::<String>().as_deref(), Some("ab"));
        assert!(!w.has_resource::<String>());
    }

    #[test]
    fn event_lifetime_across_updates() {
        let mut w = World::new();
        w.send_event(1_u8);
        let mut seen = Vec::new();
        for _ in 0..3 {
            seen.push(w.read_events::<u8>().copied().collect::<Vec<_>>());
            w.update_events();
        }
        assert_eq!(seen, vec![vec![1], vec![1], vec![]]);

        // An event sent in the second update lives through the third.
        let mut w = World::new();
        w.update_events();
        w.send_event(2_u8);
        w.update_events();
        assert_eq!(w.read_events::<u8>().count(), 1);
        w.update_events();
        assert_eq!(w.read_events::<u8>().count(), 0);
    }

    #[test]
    fn world_queue_flushes() {
        let mut w = World::new();
        let e = w.spawn((1_u8,));
        w.commands().despawn(e);
        assert!(w.contains(e));
        w.flush_commands();
        assert!(!w.contains(e));
    }
}
