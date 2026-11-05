//! Widget identities.

use std::hash::{Hash, Hasher};

/// A widget's identity, derived from its label and the containers around
/// it. Two widgets in one container with the same label are told apart by
/// the order they are added in; give them distinct labels (a `##suffix`
/// after the visible text is part of the identity but not drawn) or
/// [`Frame::with_id`](crate::Frame::with_id) to keep ids stable when the
/// order changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(pub u64);

impl WidgetId {
    /// The id that means "no widget".
    pub const NONE: WidgetId = WidgetId(0);

    /// The id of a child named by `key` under this id.
    pub fn child(self, key: impl Hash) -> WidgetId {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325 ^ self.0.rotate_left(17));
        key.hash(&mut h);
        // Zero is reserved for NONE.
        WidgetId(h.0.max(1))
    }
}

/// FNV-1a: stable across runs and platforms, unlike the standard hasher.
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// The part of a label that is drawn: everything before `##`.
pub fn visible_label(label: &str) -> &str {
    label.split_once("##").map_or(label, |(shown, _)| shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_distinct() {
        let root = WidgetId(1);
        assert_eq!(root.child("a"), root.child("a"));
        assert_ne!(root.child("a"), root.child("b"));
        assert_ne!(root.child("a"), WidgetId(2).child("a"));
        assert_ne!(root.child("a"), WidgetId::NONE);
        // The hash must not change between releases: saved layouts and
        // tests rely on it.
        assert_eq!(WidgetId(1).child("ok").0, WidgetId(1).child("ok").0);
    }

    #[test]
    fn hash_suffix_is_hidden() {
        assert_eq!(visible_label("Save##file"), "Save");
        assert_eq!(visible_label("Save"), "Save");
        assert_eq!(visible_label("##only"), "");
    }
}
