//! Typed double-buffered event queues.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Double-buffered queue of events of one type.
///
/// An event sent during update N is readable during update N and update
/// N+1. [`World::update_events`](crate::World::update_events) rotates the
/// buffers once per update; the [`Schedule`](crate::Schedule) calls it after
/// the last stage.
#[derive(Debug)]
pub struct Events<T> {
    previous: Vec<T>,
    current: Vec<T>,
}

impl<T> Default for Events<T> {
    fn default() -> Self {
        Self {
            previous: Vec::new(),
            current: Vec::new(),
        }
    }
}

impl<T> Events<T> {
    /// Appends an event to the current buffer.
    pub fn send(&mut self, event: T) {
        self.current.push(event);
    }

    /// Iterates over the events of the previous update, then the current one.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.previous.iter().chain(self.current.iter())
    }

    /// Number of readable events.
    pub fn len(&self) -> usize {
        self.previous.len() + self.current.len()
    }

    /// Returns true when no event is readable.
    pub fn is_empty(&self) -> bool {
        self.previous.is_empty() && self.current.is_empty()
    }

    /// Drops every event, including the ones not yet read.
    pub fn clear(&mut self) {
        self.previous.clear();
        self.current.clear();
    }

    /// Drops the previous buffer and makes the current one the previous.
    pub fn rotate(&mut self) {
        self.previous.clear();
        std::mem::swap(&mut self.previous, &mut self.current);
    }
}

trait EventStore: Any {
    fn rotate(&mut self);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> EventStore for Events<T> {
    fn rotate(&mut self) {
        Events::rotate(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// All event queues of a world, keyed by event type.
#[derive(Default)]
pub(crate) struct EventQueues {
    stores: HashMap<TypeId, Box<dyn EventStore>>,
}

impl EventQueues {
    pub(crate) fn get<T: 'static>(&self) -> Option<&Events<T>> {
        self.stores
            .get(&TypeId::of::<T>())
            .and_then(|s| s.as_any().downcast_ref())
    }

    pub(crate) fn get_or_create<T: 'static>(&mut self) -> &mut Events<T> {
        let store = self
            .stores
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Events::<T>::default()));
        store
            .as_any_mut()
            .downcast_mut()
            // The map key is TypeId::of::<T>() and only the line above
            // inserts, always an Events<T>, so the downcast cannot fail.
            .expect("event store type matches its TypeId key")
    }

    pub(crate) fn rotate_all(&mut self) {
        for store in self.stores.values_mut() {
            store.rotate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_is_readable_for_two_updates() {
        let mut ev = Events::<u32>::default();
        ev.send(7);
        let seen: Vec<Vec<u32>> = (0..3)
            .map(|_| {
                let v = ev.iter().copied().collect();
                ev.rotate();
                v
            })
            .collect();
        assert_eq!(seen, vec![vec![7], vec![7], vec![]]);
    }

    #[test]
    fn clear_drops_both_buffers() {
        let mut ev = Events::<u32>::default();
        ev.send(1);
        ev.rotate();
        ev.send(2);
        assert_eq!(ev.len(), 2);
        ev.clear();
        assert!(ev.is_empty());
    }
}
