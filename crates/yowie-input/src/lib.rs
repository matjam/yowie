//! Keys, mouse buttons, gamepads, per-update input state and rebindable
//! action maps.
//!
//! [`InputState`] holds every key, button, stick and the pointer as a game
//! reads them through `ctx.input`. Level queries report what is held now;
//! edge queries report what changed since the previous fixed update, so a
//! press is seen exactly once however many frames it spans, and an event
//! that arrives between updates is never lost. While the engine draws, the
//! edge queries cover the drawn frame instead, so an interface built in
//! `draw` sees every change.
//!
//! Keys are physical positions named after a US layout ([`Key::W`] is the
//! key in W's place whatever it prints). Typed text with the layout,
//! modifiers and input method applied comes from [`InputState::text`] and
//! [`InputState::composition`].
//!
//! [`ActionMap`] names what the player does and binds each name to keys,
//! buttons and axes, with two-axis composites (WASD or a stick to a
//! `Vec2`), rebinding through [`ActionMap::listen`] and serde save and load.
//!
//! This crate does not depend on a window system. The engine translates
//! platform events into [`InputEvent`]s and feeds them to
//! [`InputState::apply`].

#![warn(missing_docs)]

mod action;
mod action_map;
mod action_serde;
mod buttons;
mod edges;
mod event;
mod gamepad;
mod key;
mod state;

pub use action::{AXIS_PRESS_THRESHOLD, Binding, Binding2, Source};
pub use action_map::{ActionId, ActionMap, ActionRef, Bindings, DEFAULT_ACTION_DEAD_ZONE};
pub use buttons::{Modifiers, MouseButton};
pub use event::InputEvent;
pub use gamepad::{AXIS_DEAD_ZONE, Gamepad, GamepadAxis, GamepadButton, GamepadId, MAX_GAMEPADS};
pub use key::Key;
pub use state::{DOUBLE_CLICK_DISTANCE, DOUBLE_CLICK_TIME, InputState};

/// Errors from parsing input names and bindings.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum InputError {
    /// A key, button or axis name that does not exist.
    #[error("unknown {kind} {name:?}")]
    UnknownName {
        /// What kind of name was expected.
        kind: &'static str,
        /// The name that was given.
        name: String,
    },
    /// A binding string that is not `kind:Name` with an optional sign and
    /// scale.
    #[error("bad input source {0:?}: expected key:Name, mouse:Name, pad:Name or axis:Name")]
    BadSource(String),
}
