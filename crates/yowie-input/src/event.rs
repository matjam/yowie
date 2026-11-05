//! Input events in Yowie's own terms, which the engine translates from the
//! window system and the gamepad library and feeds to
//! [`InputState::apply`](crate::InputState::apply).

use yowie_core::Vec2;

use crate::{GamepadAxis, GamepadButton, GamepadId, Key, Modifiers, MouseButton};

/// One change to the keyboard, pointer, text or gamepads.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// A key went down. `repeat` is true for the operating system's
    /// auto-repeat while the key is held.
    KeyDown {
        /// The physical key.
        key: Key,
        /// Whether this is an auto-repeat rather than the first press.
        repeat: bool,
    },
    /// A key went up.
    KeyUp {
        /// The physical key.
        key: Key,
    },
    /// The held modifiers changed.
    Modifiers(Modifiers),
    /// Committed text: typed characters with the layout and modifiers
    /// applied, or an input method's finished composition. Control
    /// characters are not included.
    Text(String),
    /// The input method's text in progress changed; empty text ends the
    /// composition. `cursor` is a byte range within `text`.
    Composition {
        /// The uncommitted text.
        text: String,
        /// The selected or caret byte range within `text`, if any.
        cursor: Option<(usize, usize)>,
    },
    /// The pointer moved.
    MouseMoved {
        /// The position in view units (the game's 2D coordinates).
        view: Vec2,
        /// The position in window pixels from the top-left corner.
        window: Vec2,
    },
    /// Raw relative pointer motion from the device, in device units. It
    /// keeps arriving while the cursor is grabbed.
    MouseMotion {
        /// The movement since the previous motion event.
        delta: Vec2,
    },
    /// The pointer left the window.
    MouseLeft,
    /// A mouse button went down or up.
    MouseButton {
        /// The button.
        button: MouseButton,
        /// True for down, false for up.
        pressed: bool,
    },
    /// The wheel turned, in lines; +Y scrolls up, away from the user.
    Wheel {
        /// Lines scrolled on each axis.
        lines: Vec2,
    },
    /// A controller connected in the given slot.
    GamepadConnected {
        /// The slot.
        id: GamepadId,
        /// The device's name.
        name: String,
    },
    /// The controller in the given slot disconnected.
    GamepadDisconnected {
        /// The slot.
        id: GamepadId,
    },
    /// A controller button went down or up.
    GamepadButton {
        /// The slot.
        id: GamepadId,
        /// The button.
        button: GamepadButton,
        /// True for down, false for up.
        pressed: bool,
    },
    /// A controller axis changed.
    GamepadAxis {
        /// The slot.
        id: GamepadId,
        /// The axis.
        axis: GamepadAxis,
        /// The new value, -1 to 1 for sticks and 0 to 1 for triggers.
        value: f32,
    },
    /// The window lost keyboard focus. Every held key and button is
    /// released, because their key-up events will not arrive.
    FocusLost,
}
