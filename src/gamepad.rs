//! Gamepads through gilrs, translated into Yowie input events with each
//! controller in a stable slot.

use std::collections::HashMap;

use gilrs::{Axis, Button, EventType, Gilrs};
use yowie_input::{GamepadAxis, GamepadButton, GamepadId, InputEvent, MAX_GAMEPADS};

/// The gamepad library and the slot each controller occupies.
pub(crate) struct Gamepads {
    gilrs: Gilrs,
    slots: HashMap<gilrs::GamepadId, GamepadId>,
    pending: Vec<InputEvent>,
}

impl std::fmt::Debug for Gamepads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gamepads")
            .field("connected", &self.slots.len())
            .finish()
    }
}

fn button(b: Button) -> Option<GamepadButton> {
    use GamepadButton as G;
    Some(match b {
        Button::South => G::South,
        Button::East => G::East,
        Button::North => G::North,
        Button::West => G::West,
        Button::LeftTrigger => G::LeftShoulder,
        Button::RightTrigger => G::RightShoulder,
        Button::LeftTrigger2 => G::LeftTrigger,
        Button::RightTrigger2 => G::RightTrigger,
        Button::Select => G::Select,
        Button::Start => G::Start,
        Button::Mode => G::Mode,
        Button::LeftThumb => G::LeftStick,
        Button::RightThumb => G::RightStick,
        Button::DPadUp => G::DPadUp,
        Button::DPadDown => G::DPadDown,
        Button::DPadLeft => G::DPadLeft,
        Button::DPadRight => G::DPadRight,
        _ => return None,
    })
}

fn axis(a: Axis) -> Option<GamepadAxis> {
    Some(match a {
        Axis::LeftStickX => GamepadAxis::LeftX,
        Axis::LeftStickY => GamepadAxis::LeftY,
        Axis::RightStickX => GamepadAxis::RightX,
        Axis::RightStickY => GamepadAxis::RightY,
        Axis::LeftZ => GamepadAxis::LeftTrigger,
        Axis::RightZ => GamepadAxis::RightTrigger,
        _ => return None,
    })
}

impl Gamepads {
    /// Opens the platform's gamepad support, or returns `None` (logged) when
    /// it is unavailable. Controllers already connected are reported by the
    /// first [`poll`](Self::poll).
    pub(crate) fn new() -> Option<Self> {
        let gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(gilrs::Error::NotImplemented(g)) => {
                log::info!("yowie: gamepads are not supported on this platform");
                g
            }
            Err(e) => {
                log::warn!("yowie: gamepads unavailable: {e}");
                return None;
            }
        };
        let mut pads = Self {
            gilrs,
            slots: HashMap::new(),
            pending: Vec::new(),
        };
        let present: Vec<(gilrs::GamepadId, String)> = pads
            .gilrs
            .gamepads()
            .map(|(id, g)| (id, g.name().to_owned()))
            .collect();
        for (id, name) in present {
            pads.connect(id, name);
        }
        Some(pads)
    }

    fn connect(&mut self, id: gilrs::GamepadId, name: String) -> Option<GamepadId> {
        if let Some(slot) = self.slots.get(&id) {
            return Some(*slot);
        }
        let free = (0..MAX_GAMEPADS as u8)
            .map(GamepadId)
            .find(|s| !self.slots.values().any(|v| v == s))?;
        self.slots.insert(id, free);
        log::info!("yowie: gamepad {} connected: {name}", free.0);
        self.pending
            .push(InputEvent::GamepadConnected { id: free, name });
        Some(free)
    }

    /// Whether any controller is connected, which bounds a turn-based
    /// loop's sleep because gamepad events do not wake the window system.
    pub(crate) fn any_connected(&self) -> bool {
        !self.slots.is_empty()
    }

    /// Drains the gamepad library's events as Yowie events.
    pub(crate) fn poll(&mut self) -> Vec<InputEvent> {
        let mut out = std::mem::take(&mut self.pending);
        while let Some(ev) = self.gilrs.next_event() {
            let slot = match ev.event {
                EventType::Connected => {
                    let name = self.gilrs.gamepad(ev.id).name().to_owned();
                    self.connect(ev.id, name);
                    out.append(&mut self.pending);
                    continue;
                }
                EventType::Disconnected => {
                    if let Some(id) = self.slots.remove(&ev.id) {
                        out.push(InputEvent::GamepadDisconnected { id });
                    }
                    continue;
                }
                _ => match self.slots.get(&ev.id) {
                    Some(s) => *s,
                    None => continue,
                },
            };
            match ev.event {
                EventType::ButtonPressed(b, _) | EventType::ButtonReleased(b, _) => {
                    if let Some(button) = button(b) {
                        let pressed = matches!(ev.event, EventType::ButtonPressed(..));
                        out.push(InputEvent::GamepadButton {
                            id: slot,
                            button,
                            pressed,
                        });
                    }
                }
                EventType::ButtonChanged(b, value, _) => {
                    // Analogue triggers report their travel as a button value.
                    let axis = match b {
                        Button::LeftTrigger2 => GamepadAxis::LeftTrigger,
                        Button::RightTrigger2 => GamepadAxis::RightTrigger,
                        _ => continue,
                    };
                    out.push(InputEvent::GamepadAxis {
                        id: slot,
                        axis,
                        value,
                    });
                }
                EventType::AxisChanged(a, value, _) => {
                    if let Some(axis) = axis(a) {
                        out.push(InputEvent::GamepadAxis {
                            id: slot,
                            axis,
                            value,
                        });
                    }
                }
                _ => {}
            }
        }
        out
    }
}
