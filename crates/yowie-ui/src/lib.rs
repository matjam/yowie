//! Immediate-mode widgets, containers, themes, skins, keyboard and gamepad
//! navigation, and an accessibility tree.
//!
//! The interface is built again every frame. A [`Ui`] keeps what must
//! survive between frames (focus, scroll offsets, editors, window
//! rectangles); each frame, [`Ui::draw`] hands a [`Frame`] to a closure,
//! and widgets are methods on it that return a [`Response`] saying what
//! happened:
//!
//! ```no_run
//! use yowie_core::Rect;
//! use yowie_ui::Ui;
//!
//! fn frame(ui: &mut Ui, gfx: &mut yowie_gfx::Graphics, input: &yowie_input::InputState,
//!          volume: &mut f32, name: &mut String) {
//!     ui.draw(gfx, input, |f| {
//!         f.panel(Some("Options"), Rect::new(20.0, 20.0, 280.0, 200.0), |f| {
//!             f.slider("Volume", volume, 0.0..=1.0);
//!             f.text_field("Name", name);
//!             if f.button("Reset").clicked {
//!                 *volume = 0.5;
//!             }
//!         });
//!     });
//! }
//! ```
//!
//! # Identity
//!
//! A widget's [`WidgetId`] comes from its label and the containers around
//! it, so it is the same every frame. Widgets with the same label in one
//! container are told apart by call order; add a `##suffix` to a label
//! (drawn without it) or wrap the widgets in [`Frame::with_id`] when the
//! order can change.
//!
//! # Drawing
//!
//! Widgets draw through the [`Painter`] trait. [`Ui::draw`] uses
//! [`GfxPainter`], which feeds the 2D batch on
//! [`layers::UI`](yowie_gfx::layers::UI) in screen space. Within a window, backgrounds,
//! images and text are drawn in separate passes, so consecutive widgets
//! share textures and batch into a few draw calls.
//!
//! # Input
//!
//! Frames read the pointer, keyboard and gamepads from an
//! [`InputState`](yowie_input::InputState) while the engine draws, when its
//! edge queries cover the whole frame. Tab and Shift+Tab move keyboard
//! focus in the order widgets were added; the arrow keys and the d-pad move
//! it to the nearest widget in that direction unless the focused widget
//! uses them; Enter, Space and a gamepad's south button activate; Escape and
//! the east button cancel popups and dialogs. Lists, tables, tabs and
//! trees are one stop each and use the arrows inside. Focus moving into
//! a scroll area scrolls it to show the widget.
//!
//! [`Ui::accessibility`] lists the last frame's widgets with roles, names,
//! values and states.

#![warn(missing_docs)]

mod access;
mod clipboard;
mod combo;
mod containers;
mod dragdrop;
mod draw;
pub mod edit;
mod frame;
mod id;
pub mod layout;
mod menu;
mod painter;
mod response;
mod scroll;
mod select;
mod table;
mod textfield;
mod theme;
mod ui;
mod widgets_basic;
mod window;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;

pub use access::{AccessNode, AccessState, AccessTree, Role};
pub use clipboard::{Clipboard, MemoryClipboard};
pub use frame::Frame;
pub use id::{WidgetId, visible_label};
pub use layout::{
    Align, Anchor, Size, anchored, padded, split_horizontal, split_vertical, stretched,
};
pub use painter::{Face, GfxPainter, Painter};
pub use response::Response;
pub use table::{Column, TableResponse};
pub use textfield::TextOptions;
pub use theme::{PALETTES, Palette, Skin, Theme, theme_names};
pub use ui::Ui;
pub use window::WindowOptions;

pub use yowie_gfx::NineSlice;
