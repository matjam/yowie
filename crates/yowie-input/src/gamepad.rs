//! Gamepad buttons, axes and per-controller state.

use std::fmt;
use std::str::FromStr;

use crate::InputError;

/// How many controllers the input state tracks at once.
pub const MAX_GAMEPADS: usize = 4;

/// Axis values strictly between `-AXIS_DEAD_ZONE` and `AXIS_DEAD_ZONE` read
/// as zero through [`Gamepad::axis`].
pub const AXIS_DEAD_ZONE: f32 = 0.08;

/// A controller slot, `0..MAX_GAMEPADS`. The engine gives each connected
/// controller the lowest free slot and keeps it until it disconnects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GamepadId(pub u8);

impl GamepadId {
    /// The slot as an index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

macro_rules! named_enum {
    ($(#[$meta:meta])* $ty:ident, $kind:literal, { $($(#[$vdoc:meta])* $variant:ident = $name:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $ty {
            $($(#[$vdoc])* $variant,)*
        }

        impl $ty {
            /// Every value, in declaration order.
            pub const ALL: &'static [$ty] = &[$($ty::$variant,)*];
            /// The number of values.
            pub const COUNT: usize = Self::ALL.len();

            /// The stable name used when bindings are saved.
            pub const fn name(self) -> &'static str {
                match self {
                    $($ty::$variant => $name,)*
                }
            }

            /// The value's position in `ALL`.
            pub const fn index(self) -> usize {
                self as usize
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.name())
            }
        }

        impl FromStr for $ty {
            type Err = InputError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|v| v.name().eq_ignore_ascii_case(s))
                    .ok_or_else(|| InputError::UnknownName { kind: $kind, name: s.to_owned() })
            }
        }
    };
}

named_enum! {
    /// A button on a standard gamepad. Face buttons are named by position:
    /// `South` is A on Xbox controllers and Cross on PlayStation ones.
    GamepadButton, "gamepad button", {
        /// The bottom face button.
        South = "South",
        /// The right face button.
        East = "East",
        /// The left face button.
        West = "West",
        /// The top face button.
        North = "North",
        /// The left shoulder button.
        LeftShoulder = "LeftShoulder",
        /// The right shoulder button.
        RightShoulder = "RightShoulder",
        /// The left trigger read as a button.
        LeftTrigger = "LeftTrigger",
        /// The right trigger read as a button.
        RightTrigger = "RightTrigger",
        /// Pressing the left stick in.
        LeftStick = "LeftStick",
        /// Pressing the right stick in.
        RightStick = "RightStick",
        /// Start, Menu or Options.
        Start = "Start",
        /// Select, Back, View or Share.
        Select = "Select",
        /// The vendor button in the middle of the pad.
        Mode = "Mode",
        /// The directional pad's up direction.
        DPadUp = "DPadUp",
        /// The directional pad's down direction.
        DPadDown = "DPadDown",
        /// The directional pad's left direction.
        DPadLeft = "DPadLeft",
        /// The directional pad's right direction.
        DPadRight = "DPadRight",
    }
}

named_enum! {
    /// An analogue input on a standard gamepad. Sticks run from -1 to 1
    /// with +X right and +Y up, as the hardware reports them; triggers run
    /// from 0 (released) to 1 (pressed).
    GamepadAxis, "gamepad axis", {
        /// The left stick's horizontal axis.
        LeftX = "LeftX",
        /// The left stick's vertical axis, positive up.
        LeftY = "LeftY",
        /// The right stick's horizontal axis.
        RightX = "RightX",
        /// The right stick's vertical axis, positive up.
        RightY = "RightY",
        /// The left trigger's travel.
        LeftTrigger = "LeftTrigger",
        /// The right trigger's travel.
        RightTrigger = "RightTrigger",
    }
}

/// One controller slot's level state: what is held and where the sticks
/// are. Edges live in the input state's edge sets.
#[derive(Clone, Debug, Default)]
pub(crate) struct PadSlot {
    pub(crate) connected: bool,
    pub(crate) name: String,
    pub(crate) buttons: [bool; GamepadButton::COUNT],
    pub(crate) axes: [f32; GamepadAxis::COUNT],
    /// The axes at the end of the previous update, for axis edges.
    pub(crate) prev_axes: [f32; GamepadAxis::COUNT],
}

/// A view of one controller during the current update or frame.
///
/// A disconnected slot reads as idle: nothing held, every axis at zero.
#[derive(Clone, Copy, Debug)]
pub struct Gamepad<'a> {
    pub(crate) id: GamepadId,
    pub(crate) slot: &'a PadSlot,
    pub(crate) pressed: &'a [bool; GamepadButton::COUNT],
    pub(crate) released: &'a [bool; GamepadButton::COUNT],
    pub(crate) just_connected: bool,
    pub(crate) just_disconnected: bool,
}

impl Gamepad<'_> {
    /// The slot this view reads.
    pub fn id(&self) -> GamepadId {
        self.id
    }

    /// Whether a controller is in this slot.
    pub fn connected(&self) -> bool {
        self.slot.connected
    }

    /// The controller's name as the device reports it; empty when
    /// disconnected.
    pub fn name(&self) -> &str {
        &self.slot.name
    }

    /// Whether the button is held.
    pub fn down(&self, button: GamepadButton) -> bool {
        self.slot.buttons[button.index()]
    }

    /// Whether the button went down since the previous update (or, during
    /// drawing, since the previous frame).
    pub fn pressed(&self, button: GamepadButton) -> bool {
        self.pressed[button.index()]
    }

    /// Whether the button went up since the previous update (or, during
    /// drawing, since the previous frame).
    pub fn released(&self, button: GamepadButton) -> bool {
        self.released[button.index()]
    }

    /// The axis value with [`AXIS_DEAD_ZONE`] applied: values strictly
    /// inside it read as zero and the rest are not rescaled.
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        let v = self.slot.axes[axis.index()];
        if v.abs() < AXIS_DEAD_ZONE { 0.0 } else { v }
    }

    /// The axis value as the device reported it, with no dead zone.
    pub fn raw_axis(&self, axis: GamepadAxis) -> f32 {
        self.slot.axes[axis.index()]
    }

    /// The axis value at the end of the previous update, with no dead zone.
    pub fn previous_axis(&self, axis: GamepadAxis) -> f32 {
        self.slot.prev_axes[axis.index()]
    }

    /// Whether the controller connected since the previous update.
    pub fn just_connected(&self) -> bool {
        self.just_connected
    }

    /// Whether the controller disconnected since the previous update.
    pub fn just_disconnected(&self) -> bool {
        self.just_disconnected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for b in GamepadButton::ALL {
            assert_eq!(b.name().parse::<GamepadButton>().ok(), Some(*b));
        }
        for a in GamepadAxis::ALL {
            assert_eq!(a.name().parse::<GamepadAxis>().ok(), Some(*a));
        }
        assert!("Turbo".parse::<GamepadButton>().is_err());
    }

    #[test]
    fn axis_dead_zone() {
        let mut slot = PadSlot::default();
        slot.axes[GamepadAxis::LeftX.index()] = 0.05;
        slot.axes[GamepadAxis::LeftY.index()] = -0.5;
        let none = [false; GamepadButton::COUNT];
        let pad = Gamepad {
            id: GamepadId(0),
            slot: &slot,
            pressed: &none,
            released: &none,
            just_connected: false,
            just_disconnected: false,
        };
        assert_eq!(pad.axis(GamepadAxis::LeftX), 0.0);
        assert_eq!(pad.raw_axis(GamepadAxis::LeftX), 0.05);
        assert_eq!(pad.axis(GamepadAxis::LeftY), -0.5);
    }
}
