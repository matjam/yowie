//! The changes accumulated between two clears: one set covers an update,
//! another covers a drawn frame.

use yowie_core::Vec2;

use crate::gamepad::MAX_GAMEPADS;
use crate::key::KeySet;
use crate::{GamepadButton, MouseButton};

/// Everything that changed since the set was last cleared.
#[derive(Clone, Debug, Default)]
pub(crate) struct Edges {
    pub(crate) pressed: KeySet,
    pub(crate) released: KeySet,
    pub(crate) repeated: KeySet,
    pub(crate) mouse_pressed: [bool; MouseButton::COUNT],
    pub(crate) mouse_released: [bool; MouseButton::COUNT],
    pub(crate) double_clicked: [bool; MouseButton::COUNT],
    pub(crate) wheel: Vec2,
    pub(crate) mouse_delta: Vec2,
    pub(crate) motion: Vec2,
    pub(crate) text: String,
    pub(crate) pad_pressed: [[bool; GamepadButton::COUNT]; MAX_GAMEPADS],
    pub(crate) pad_released: [[bool; GamepadButton::COUNT]; MAX_GAMEPADS],
    pub(crate) pad_connected: [bool; MAX_GAMEPADS],
    pub(crate) pad_disconnected: [bool; MAX_GAMEPADS],
}

impl Edges {
    /// Empties the set, keeping the text buffer's allocation.
    pub(crate) fn clear(&mut self) {
        let mut text = std::mem::take(&mut self.text);
        text.clear();
        *self = Edges {
            text,
            ..Edges::default()
        };
    }
}

/// Which edge set the accessors read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Accessors report changes since the previous update.
    #[default]
    Update,
    /// Accessors report changes since the previous drawn frame.
    Draw,
}
