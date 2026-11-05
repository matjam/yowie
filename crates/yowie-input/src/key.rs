//! Physical keys, named by their position on a US keyboard.

use std::fmt;
use std::str::FromStr;

use crate::InputError;

/// Defines [`Key`] with its saved names and the list of every key.
macro_rules! keys {
    ($($variant:ident = $name:literal,)*) => {
        /// A physical key, identified by its position on the keyboard and
        /// named after the key in that position on a US layout.
        ///
        /// `Key::W` is the key in W's place whatever the active layout
        /// prints on it, which is what movement bindings need. Use the
        /// typed text from [`InputState::text`](crate::InputState::text)
        /// for text entry.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Key {
            $(
                #[doc = concat!("The `", $name, "` key position.")]
                $variant,
            )*
        }

        impl Key {
            /// Every key, in declaration order.
            pub const ALL: &'static [Key] = &[$(Key::$variant,)*];

            /// The stable name used when bindings are saved, such as
            /// `"Space"` or `"KeypadEnter"`.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Key::$variant => $name,)*
                }
            }
        }
    };
}

keys! {
    A = "A", B = "B", C = "C", D = "D", E = "E", F = "F", G = "G", H = "H",
    I = "I", J = "J", K = "K", L = "L", M = "M", N = "N", O = "O", P = "P",
    Q = "Q", R = "R", S = "S", T = "T", U = "U", V = "V", W = "W", X = "X",
    Y = "Y", Z = "Z",
    Digit0 = "0", Digit1 = "1", Digit2 = "2", Digit3 = "3", Digit4 = "4",
    Digit5 = "5", Digit6 = "6", Digit7 = "7", Digit8 = "8", Digit9 = "9",
    F1 = "F1", F2 = "F2", F3 = "F3", F4 = "F4", F5 = "F5", F6 = "F6",
    F7 = "F7", F8 = "F8", F9 = "F9", F10 = "F10", F11 = "F11", F12 = "F12",
    F13 = "F13", F14 = "F14", F15 = "F15", F16 = "F16", F17 = "F17",
    F18 = "F18", F19 = "F19", F20 = "F20", F21 = "F21", F22 = "F22",
    F23 = "F23", F24 = "F24",
    Space = "Space", Enter = "Enter", Escape = "Escape", Tab = "Tab",
    Backspace = "Backspace", Delete = "Delete", Insert = "Insert",
    Home = "Home", End = "End", PageUp = "PageUp", PageDown = "PageDown",
    Left = "Left", Right = "Right", Up = "Up", Down = "Down",
    Minus = "Minus", Equal = "Equal", LeftBracket = "LeftBracket",
    RightBracket = "RightBracket", Backslash = "Backslash",
    Semicolon = "Semicolon", Apostrophe = "Apostrophe", Grave = "Grave",
    Comma = "Comma", Period = "Period", Slash = "Slash",
    IntlBackslash = "IntlBackslash", IntlRo = "IntlRo", IntlYen = "IntlYen",
    CapsLock = "CapsLock", NumLock = "NumLock", ScrollLock = "ScrollLock",
    PrintScreen = "PrintScreen", Pause = "Pause", ContextMenu = "ContextMenu",
    LeftShift = "LeftShift", RightShift = "RightShift",
    LeftControl = "LeftControl", RightControl = "RightControl",
    LeftAlt = "LeftAlt", RightAlt = "RightAlt",
    LeftSuper = "LeftSuper", RightSuper = "RightSuper",
    Keypad0 = "Keypad0", Keypad1 = "Keypad1", Keypad2 = "Keypad2",
    Keypad3 = "Keypad3", Keypad4 = "Keypad4", Keypad5 = "Keypad5",
    Keypad6 = "Keypad6", Keypad7 = "Keypad7", Keypad8 = "Keypad8",
    Keypad9 = "Keypad9", KeypadDecimal = "KeypadDecimal",
    KeypadDivide = "KeypadDivide", KeypadMultiply = "KeypadMultiply",
    KeypadSubtract = "KeypadSubtract", KeypadAdd = "KeypadAdd",
    KeypadEnter = "KeypadEnter", KeypadEqual = "KeypadEqual",
}

impl Key {
    /// The number of keys; every `key as usize` is below it.
    pub const COUNT: usize = Key::ALL.len();

    /// The key whose saved name is `name`, compared without regard to case.
    pub fn from_name(name: &str) -> Option<Key> {
        Key::ALL
            .iter()
            .copied()
            .find(|k| k.name().eq_ignore_ascii_case(name))
    }

    /// The key's position in [`Key::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Key {
    type Err = InputError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Key::from_name(s).ok_or_else(|| InputError::UnknownName {
            kind: "key",
            name: s.to_owned(),
        })
    }
}

// KeySet stores 128 bits.
const _: () = assert!(Key::COUNT <= 128);

/// A set of keys stored as bits, used for the held and edge sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeySet([u64; 2]);

impl KeySet {
    pub(crate) fn contains(&self, key: Key) -> bool {
        let i = key.index();
        self.0[i / 64] & (1 << (i % 64)) != 0
    }

    pub(crate) fn set(&mut self, key: Key, on: bool) {
        let i = key.index();
        if on {
            self.0[i / 64] |= 1 << (i % 64);
        } else {
            self.0[i / 64] &= !(1 << (i % 64));
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Key> + '_ {
        Key::ALL.iter().copied().filter(|k| self.contains(*k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        for (i, k) in Key::ALL.iter().enumerate() {
            assert_eq!(k.index(), i);
            assert_eq!(Key::from_name(k.name()), Some(*k));
            assert_eq!(k.name().parse::<Key>().ok(), Some(*k));
        }
        assert_eq!(Key::from_name("space"), Some(Key::Space));
        assert!("NoSuchKey".parse::<Key>().is_err());
    }

    #[test]
    fn key_set_bits() {
        let mut s = KeySet::default();
        for k in [Key::A, Key::KeypadEqual, Key::Space] {
            s.set(k, true);
        }
        assert!(s.contains(Key::KeypadEqual));
        assert_eq!(s.iter().count(), 3);
        s.set(Key::A, false);
        assert!(!s.contains(Key::A));
    }
}
