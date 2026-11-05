//! The keyboard, pointer, text and gamepads as a game sees them.

use yowie_core::Vec2;

use crate::edges::{Edges, Phase};
use crate::gamepad::{MAX_GAMEPADS, PadSlot};
use crate::key::KeySet;
use crate::{Gamepad, GamepadButton, GamepadId, InputEvent, Key, Modifiers, MouseButton};

/// How close in time two presses must be to count as a double click, in
/// seconds of updates.
pub const DOUBLE_CLICK_TIME: f32 = 0.4;
/// How close in view units two presses must be to count as a double click.
pub const DOUBLE_CLICK_DISTANCE: f32 = 6.0;

static IDLE_PAD: PadSlot = PadSlot {
    connected: false,
    name: String::new(),
    buttons: [false; GamepadButton::COUNT],
    axes: [0.0; crate::GamepadAxis::COUNT],
    prev_axes: [0.0; crate::GamepadAxis::COUNT],
};
static NO_BUTTONS: [bool; GamepadButton::COUNT] = [false; GamepadButton::COUNT];

/// What is held, what changed, where the pointer is and what was typed.
///
/// Level queries ([`key_down`](Self::key_down), [`mouse_down`](Self::mouse_down))
/// report the state now. Edge queries ([`key_pressed`](Self::key_pressed),
/// [`mouse_released`](Self::mouse_released), [`text`](Self::text) and the
/// rest) report changes since the previous fixed update: an event that
/// arrives between updates is kept until the next update sees it, and when
/// several updates run in one frame only the first sees it. While the
/// engine draws, the edge queries instead cover the whole drawn frame, so
/// an interface built during drawing sees every change even when the frame
/// ran several updates or none.
///
/// The engine feeds events through [`apply`](Self::apply) and calls
/// [`end_update`](Self::end_update), [`begin_draw`](Self::begin_draw) and
/// [`end_draw`](Self::end_draw); games only read.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    down: KeySet,
    held: Vec<f32>,
    mods: Modifiers,
    mouse_view: Vec2,
    mouse_window: Vec2,
    mouse_inside: bool,
    mouse_down: [bool; MouseButton::COUNT],
    composition: String,
    composition_cursor: Option<(usize, usize)>,
    pads: [PadSlot; MAX_GAMEPADS],
    update: Edges,
    frame: Edges,
    phase: Phase,
    step: f32,
    clock: f32,
    last_click: [Option<(f32, Vec2)>; MouseButton::COUNT],
}

impl InputState {
    /// An idle state: nothing held, the pointer at the origin.
    pub fn new() -> Self {
        Self::default()
    }

    fn edges(&self) -> &Edges {
        match self.phase {
            Phase::Update => &self.update,
            Phase::Draw => &self.frame,
        }
    }

    /// Whether the key is held.
    pub fn key_down(&self, key: Key) -> bool {
        self.down.contains(key)
    }

    /// Whether the key went down since the previous update (or, while
    /// drawing, since the previous frame). Auto-repeats do not count; see
    /// [`key_repeated`](Self::key_repeated).
    pub fn key_pressed(&self, key: Key) -> bool {
        self.edges().pressed.contains(key)
    }

    /// Whether the key went up since the previous update (or, while
    /// drawing, since the previous frame).
    pub fn key_released(&self, key: Key) -> bool {
        self.edges().released.contains(key)
    }

    /// Whether the operating system sent an auto-repeat for the key since
    /// the previous update. For a menu that steps while a key is held,
    /// test `key_pressed(k) || key_repeated(k)`.
    pub fn key_repeated(&self, key: Key) -> bool {
        self.edges().repeated.contains(key)
    }

    /// How long the key has been held, in seconds of updates; zero when it
    /// is up.
    pub fn key_held(&self, key: Key) -> f32 {
        self.held.get(key.index()).copied().unwrap_or(0.0)
    }

    /// Every held key, in [`Key::ALL`] order.
    pub fn keys_down(&self) -> impl Iterator<Item = Key> + '_ {
        self.down.iter()
    }

    /// The held modifiers.
    pub fn modifiers(&self) -> Modifiers {
        self.mods
    }

    /// The pointer position in view units, the game's 2D coordinates. With
    /// a fixed view, a position over the letterbox bars lies outside the
    /// view rectangle.
    pub fn mouse(&self) -> Vec2 {
        self.mouse_view
    }

    /// The pointer position in window pixels from the top-left corner.
    pub fn mouse_window(&self) -> Vec2 {
        self.mouse_window
    }

    /// Whether the pointer is over the window.
    pub fn mouse_inside(&self) -> bool {
        self.mouse_inside
    }

    /// Pointer movement in view units since the previous update (or, while
    /// drawing, since the previous frame).
    pub fn mouse_delta(&self) -> Vec2 {
        self.edges().mouse_delta
    }

    /// Raw device motion since the previous update, which keeps arriving
    /// while the cursor is grabbed. Use it for a first-person camera.
    pub fn mouse_motion(&self) -> Vec2 {
        self.edges().motion
    }

    /// Whether the button is held.
    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.mouse_down[button.index()]
    }

    /// Whether the button went down since the previous update (or, while
    /// drawing, since the previous frame).
    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.edges().mouse_pressed[button.index()]
    }

    /// Whether the button went up since the previous update (or, while
    /// drawing, since the previous frame).
    pub fn mouse_released(&self, button: MouseButton) -> bool {
        self.edges().mouse_released[button.index()]
    }

    /// Whether this press was the second of two within
    /// [`DOUBLE_CLICK_TIME`] and [`DOUBLE_CLICK_DISTANCE`].
    pub fn mouse_double_clicked(&self, button: MouseButton) -> bool {
        self.edges().double_clicked[button.index()]
    }

    /// Wheel movement in lines since the previous update; +Y is away from
    /// the user. A trackpad's pixel scrolling is converted to lines by the
    /// engine.
    pub fn wheel(&self) -> Vec2 {
        self.edges().wheel
    }

    /// Text committed since the previous update (or, while drawing, since
    /// the previous frame): typed characters and finished input-method
    /// compositions, without control characters.
    pub fn text(&self) -> &str {
        &self.edges().text
    }

    /// The input method's uncommitted text; empty when nothing is being
    /// composed. Text fields show it after the committed text.
    pub fn composition(&self) -> &str {
        &self.composition
    }

    /// The caret or selection inside [`composition`](Self::composition), as
    /// a byte range.
    pub fn composition_cursor(&self) -> Option<(usize, usize)> {
        self.composition_cursor
    }

    /// A view of controller `id`. A disconnected or out-of-range slot reads
    /// as idle.
    pub fn gamepad(&self, id: GamepadId) -> Gamepad<'_> {
        let i = id.index();
        let edges = self.edges();
        match self.pads.get(i) {
            Some(slot) => Gamepad {
                id,
                slot,
                pressed: &edges.pad_pressed[i],
                released: &edges.pad_released[i],
                just_connected: edges.pad_connected[i],
                just_disconnected: edges.pad_disconnected[i],
            },
            None => Gamepad {
                id,
                slot: &IDLE_PAD,
                pressed: &NO_BUTTONS,
                released: &NO_BUTTONS,
                just_connected: false,
                just_disconnected: false,
            },
        }
    }

    /// Every connected controller.
    pub fn gamepads(&self) -> impl Iterator<Item = Gamepad<'_>> + '_ {
        (0..MAX_GAMEPADS as u8)
            .map(|i| self.gamepad(GamepadId(i)))
            .filter(|g| g.connected())
    }

    /// Records one event into the level state and both edge sets. The
    /// engine calls this as events arrive.
    pub fn apply(&mut self, event: &InputEvent) {
        if self.held.len() != Key::COUNT {
            self.held = vec![0.0; Key::COUNT];
        }
        match event {
            InputEvent::KeyDown { key, repeat } => {
                if *repeat && self.down.contains(*key) {
                    self.both(|e| e.repeated.set(*key, true));
                } else if !self.down.contains(*key) {
                    self.down.set(*key, true);
                    self.both(|e| e.pressed.set(*key, true));
                }
            }
            InputEvent::KeyUp { key } => {
                if self.down.contains(*key) {
                    self.down.set(*key, false);
                    self.both(|e| e.released.set(*key, true));
                }
            }
            InputEvent::Modifiers(m) => self.mods = *m,
            InputEvent::Text(t) => {
                let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                self.both(|e| e.text.push_str(&clean));
            }
            InputEvent::Composition { text, cursor } => {
                self.composition.clone_from(text);
                self.composition_cursor = *cursor;
            }
            InputEvent::MouseMoved { view, window } => {
                if self.mouse_inside {
                    let d = *view - self.mouse_view;
                    self.both(|e| e.mouse_delta += d);
                }
                self.mouse_view = *view;
                self.mouse_window = *window;
                self.mouse_inside = true;
            }
            InputEvent::MouseMotion { delta } => self.both(|e| e.motion += *delta),
            InputEvent::MouseLeft => self.mouse_inside = false,
            InputEvent::MouseButton { button, pressed } => self.mouse_button(*button, *pressed),
            InputEvent::Wheel { lines } => self.both(|e| e.wheel += *lines),
            InputEvent::GamepadConnected { id, name } => {
                if let Some(slot) = self.pads.get_mut(id.index()) {
                    *slot = PadSlot {
                        connected: true,
                        name: name.clone(),
                        ..PadSlot::default()
                    };
                    let i = id.index();
                    self.both(|e| e.pad_connected[i] = true);
                }
            }
            InputEvent::GamepadDisconnected { id } => {
                if let Some(slot) = self.pads.get_mut(id.index()) {
                    *slot = PadSlot::default();
                    let i = id.index();
                    self.both(|e| e.pad_disconnected[i] = true);
                }
            }
            InputEvent::GamepadButton {
                id,
                button,
                pressed,
            } => self.pad_button(*id, *button, *pressed),
            InputEvent::GamepadAxis { id, axis, value } => {
                if let Some(slot) = self.pads.get_mut(id.index()) {
                    slot.axes[axis.index()] = value.clamp(-1.0, 1.0);
                }
            }
            InputEvent::FocusLost => self.release_all(),
        }
    }

    fn both(&mut self, mut f: impl FnMut(&mut Edges)) {
        f(&mut self.update);
        f(&mut self.frame);
    }

    fn mouse_button(&mut self, button: MouseButton, pressed: bool) {
        let b = button.index();
        if self.mouse_down[b] == pressed {
            return;
        }
        self.mouse_down[b] = pressed;
        if !pressed {
            self.both(|e| e.mouse_released[b] = true);
            return;
        }
        let at = self.mouse_view;
        let double = matches!(self.last_click[b], Some((t, p))
            if self.clock - t <= DOUBLE_CLICK_TIME && p.distance(at) <= DOUBLE_CLICK_DISTANCE);
        // A third click starts a new pair rather than counting again.
        self.last_click[b] = if double { None } else { Some((self.clock, at)) };
        self.both(|e| {
            e.mouse_pressed[b] = true;
            e.double_clicked[b] |= double;
        });
    }

    fn pad_button(&mut self, id: GamepadId, button: GamepadButton, pressed: bool) {
        let i = id.index();
        let Some(slot) = self.pads.get_mut(i) else {
            return;
        };
        let b = button.index();
        if slot.buttons[b] == pressed {
            return;
        }
        slot.buttons[b] = pressed;
        self.both(|e| {
            if pressed {
                e.pad_pressed[i][b] = true;
            } else {
                e.pad_released[i][b] = true;
            }
        });
    }

    fn release_all(&mut self) {
        let held: Vec<Key> = self.down.iter().collect();
        for key in held {
            self.apply(&InputEvent::KeyUp { key });
        }
        for button in MouseButton::ALL {
            self.mouse_button(button, false);
        }
        self.mods = Modifiers::NONE;
    }

    /// Sets how many seconds each update covers, for held times and double
    /// clicks. The engine sets it from the fixed step; zero or less means a
    /// sixtieth of a second.
    pub fn set_update_step(&mut self, seconds: f32) {
        self.step = seconds;
    }

    /// Clears the update's edges and advances held times by one step. The
    /// engine calls it after each update.
    pub fn end_update(&mut self) {
        let step = if self.step > 0.0 {
            self.step
        } else {
            1.0 / 60.0
        };
        self.clock += step;
        if self.held.len() != Key::COUNT {
            self.held = vec![0.0; Key::COUNT];
        }
        for key in Key::ALL {
            let h = &mut self.held[key.index()];
            *h = if self.down.contains(*key) {
                *h + step
            } else {
                0.0
            };
        }
        for slot in &mut self.pads {
            slot.prev_axes = slot.axes;
        }
        self.update.clear();
    }

    /// Switches the edge queries to the drawn frame's set. The engine calls
    /// it before the game draws.
    pub fn begin_draw(&mut self) {
        self.phase = Phase::Draw;
    }

    /// Clears the frame's edges and switches the queries back to the update
    /// set. The engine calls it after the game draws.
    pub fn end_draw(&mut self) {
        self.frame.clear();
        self.phase = Phase::Update;
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
