//! Typed singletons stored in the world.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Storage for resources: at most one value per type.
#[derive(Default)]
pub(crate) struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub(crate) fn insert<T: 'static>(&mut self, value: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|old| old.downcast().ok())
            .map(|b| *b)
    }

    pub(crate) fn get<T: 'static>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub(crate) fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    pub(crate) fn remove<T: 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|old| old.downcast().ok())
            .map(|b| *b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_replace_remove() {
        let mut r = Resources::default();
        assert_eq!(r.insert(5_u32), None);
        assert_eq!(r.insert(6_u32), Some(5));
        *r.get_mut::<u32>().unwrap() += 1;
        assert_eq!(r.get::<u32>(), Some(&7));
        assert_eq!(r.get::<u64>(), None);
        assert_eq!(r.remove::<u32>(), Some(7));
        assert_eq!(r.get::<u32>(), None);
    }
}
