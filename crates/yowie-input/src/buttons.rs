//! Modifier keys and mouse buttons.

use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::str::FromStr;

use crate::InputError;

/// The modifier keys held at a moment, as a set of bits.
///
/// Use [`Modifiers::contains`] to test for one or more modifiers:
/// `mods.contains(Modifiers::CONTROL | Modifiers::SHIFT)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Either Shift key.
    pub const SHIFT: Self = Self(1);
    /// Either Control key.
    pub const CONTROL: Self = Self(1 << 1);
    /// Either Alt key (Option on macOS).
    pub const ALT: Self = Self(1 << 2);
    /// Either Super key (Command on macOS, the Windows key elsewhere).
    pub const SUPER: Self = Self(1 << 3);

    /// Whether every modifier in `other` is held.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no modifier is held.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The set with `other` added or removed.
    pub const fn with(self, other: Self, on: bool) -> Self {
        if on {
            Self(self.0 | other.0)
        } else {
            Self(self.0 & !other.0)
        }
    }

    /// The platform's shortcut modifier: Super (Command) on macOS, Control
    /// elsewhere. Use it for copy and paste shortcuts.
    pub const fn shortcut() -> Self {
        if cfg!(target_os = "macos") {
            Self::SUPER
        } else {
            Self::CONTROL
        }
    }
}

impl BitOr for Modifiers {
    type Output = Modifiers;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// A mouse button. Left, Right and Middle are the usual three; `Back` and
/// `Forward` are the side buttons many mice have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    /// The primary button.
    Left,
    /// The secondary button.
    Right,
    /// The wheel button.
    Middle,
    /// The side button that navigates back.
    Back,
    /// The side button that navigates forward.
    Forward,
}

impl MouseButton {
    /// Every mouse button, in declaration order.
    pub const ALL: [MouseButton; 5] = [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
    ];

    /// The number of mouse buttons.
    pub const COUNT: usize = Self::ALL.len();

    /// The stable name used when bindings are saved.
    pub const fn name(self) -> &'static str {
        match self {
            MouseButton::Left => "Left",
            MouseButton::Right => "Right",
            MouseButton::Middle => "Middle",
            MouseButton::Back => "Back",
            MouseButton::Forward => "Forward",
        }
    }

    /// The button's position in [`MouseButton::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for MouseButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for MouseButton {
    type Err = InputError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MouseButton::ALL
            .into_iter()
            .find(|b| b.name().eq_ignore_ascii_case(s))
            .ok_or_else(|| InputError::UnknownName {
                kind: "mouse button",
                name: s.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_sets() {
        let m = Modifiers::CONTROL | Modifiers::SHIFT;
        assert!(m.contains(Modifiers::CONTROL));
        assert!(m.contains(Modifiers::CONTROL | Modifiers::SHIFT));
        assert!(!m.contains(Modifiers::ALT));
        assert!(m.with(Modifiers::CONTROL, false) == Modifiers::SHIFT);
        assert!(Modifiers::NONE.is_empty());
    }

    #[test]
    fn mouse_names_round_trip() {
        for b in MouseButton::ALL {
            assert_eq!(b.name().parse::<MouseButton>().ok(), Some(b));
        }
    }
}
