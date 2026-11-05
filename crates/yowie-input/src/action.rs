//! What an action can be bound to, and how each source reads.

use std::fmt;
use std::str::FromStr;

use yowie_core::Vec2;

use crate::{GamepadAxis, GamepadButton, GamepadId, InputError, InputState, Key, MouseButton};

/// Travel past which an axis source counts as held for
/// [`ActionMap::down`](crate::ActionMap::down) and its edges.
pub const AXIS_PRESS_THRESHOLD: f32 = 0.5;

/// One physical input: a key, a mouse button, a gamepad button or a
/// gamepad axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A keyboard key.
    Key(Key),
    /// A mouse button.
    Mouse(MouseButton),
    /// A gamepad button.
    PadButton(GamepadButton),
    /// A gamepad axis.
    PadAxis(GamepadAxis),
}

impl Source {
    /// The binding that contributes the opposite sign: the A key of an A
    /// and D pair, or the left half of a stick's X axis.
    pub fn negative(self) -> Binding {
        Binding {
            source: self,
            scale: -1.0,
        }
    }

    /// The binding that contributes `scale` at full travel.
    pub fn scaled(self, scale: f32) -> Binding {
        Binding {
            source: self,
            scale,
        }
    }

    /// The binding2 that pushes a two-axis action towards `direction`.
    pub fn toward(self, direction: Vec2) -> Binding2 {
        Binding2 {
            source: self,
            direction,
        }
    }
}

impl From<Key> for Source {
    fn from(k: Key) -> Self {
        Source::Key(k)
    }
}

impl From<MouseButton> for Source {
    fn from(b: MouseButton) -> Self {
        Source::Mouse(b)
    }
}

impl From<GamepadButton> for Source {
    fn from(b: GamepadButton) -> Self {
        Source::PadButton(b)
    }
}

impl From<GamepadAxis> for Source {
    fn from(a: GamepadAxis) -> Self {
        Source::PadAxis(a)
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Key(k) => write!(f, "key:{k}"),
            Source::Mouse(b) => write!(f, "mouse:{b}"),
            Source::PadButton(b) => write!(f, "pad:{b}"),
            Source::PadAxis(a) => write!(f, "axis:{a}"),
        }
    }
}

impl FromStr for Source {
    type Err = InputError;

    /// Reads `key:Space`, `mouse:Left`, `pad:South` or `axis:LeftX`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, name) = s
            .split_once(':')
            .ok_or_else(|| InputError::BadSource(s.to_owned()))?;
        Ok(match kind {
            "key" => Source::Key(name.parse()?),
            "mouse" => Source::Mouse(name.parse()?),
            "pad" => Source::PadButton(name.parse()?),
            "axis" => Source::PadAxis(name.parse()?),
            _ => return Err(InputError::BadSource(s.to_owned())),
        })
    }
}

/// A source bound to a one-axis action, with the amount it contributes at
/// full travel. An axis source reads only the side its scale's sign
/// selects, so bind a stick axis twice (once with [`Source::negative`]) to read
/// both directions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binding {
    /// The physical input.
    pub source: Source,
    /// The signed contribution at full travel.
    pub scale: f32,
}

impl<S: Into<Source>> From<S> for Binding {
    fn from(source: S) -> Self {
        Binding {
            source: source.into(),
            scale: 1.0,
        }
    }
}

impl fmt::Display for Binding {
    /// Writes `key:Space`, `-axis:LeftX` for a negative side, and appends
    /// `*0.5` for a magnitude other than one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale < 0.0 {
            f.write_str("-")?;
        }
        write!(f, "{}", self.source)?;
        let m = self.scale.abs();
        if m != 1.0 {
            write!(f, "*{m}")?;
        }
        Ok(())
    }
}

impl FromStr for Binding {
    type Err = InputError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (sign, rest) = match s.strip_prefix('-') {
            Some(rest) => (-1.0, rest),
            None => (1.0, s),
        };
        let (src, magnitude) = match rest.rsplit_once('*') {
            Some((src, m)) => (
                src,
                m.parse::<f32>()
                    .map_err(|_| InputError::BadSource(s.to_owned()))?,
            ),
            None => (rest, 1.0),
        };
        Ok(Binding {
            source: src.parse()?,
            scale: sign * magnitude,
        })
    }
}

/// A source bound to a two-axis action. A key or button pushes the action
/// towards `direction` while held; an axis pushes it by its signed value
/// times `direction`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binding2 {
    /// The physical input.
    pub source: Source,
    /// The push at full travel. For screen movement, up is `(0, -1)`.
    pub direction: Vec2,
}

impl Binding2 {
    /// W, A, S and D pushing up, left, down and right in screen
    /// coordinates (+Y down).
    pub fn wasd() -> [Binding2; 4] {
        Self::four([Key::W, Key::A, Key::S, Key::D].map(Source::Key))
    }

    /// The arrow keys pushing up, left, down and right in screen
    /// coordinates.
    pub fn arrows() -> [Binding2; 4] {
        Self::four([Key::Up, Key::Left, Key::Down, Key::Right].map(Source::Key))
    }

    /// The directional pad pushing up, left, down and right in screen
    /// coordinates.
    pub fn dpad() -> [Binding2; 4] {
        use GamepadButton as B;
        Self::four([B::DPadUp, B::DPadLeft, B::DPadDown, B::DPadRight].map(Source::PadButton))
    }

    /// The left stick in screen coordinates: pushing the stick up gives
    /// negative Y.
    pub fn left_stick() -> [Binding2; 2] {
        Self::stick(GamepadAxis::LeftX, GamepadAxis::LeftY)
    }

    /// The right stick in screen coordinates: pushing the stick up gives
    /// negative Y.
    pub fn right_stick() -> [Binding2; 2] {
        Self::stick(GamepadAxis::RightX, GamepadAxis::RightY)
    }

    fn four([up, left, down, right]: [Source; 4]) -> [Binding2; 4] {
        [
            up.toward(Vec2::NEG_Y),
            left.toward(Vec2::NEG_X),
            down.toward(Vec2::Y),
            right.toward(Vec2::X),
        ]
    }

    fn stick(x: GamepadAxis, y: GamepadAxis) -> [Binding2; 2] {
        [
            Source::PadAxis(x).toward(Vec2::X),
            Source::PadAxis(y).toward(Vec2::NEG_Y),
        ]
    }
}

/// Which gamepads the pad sources read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PadFilter(pub(crate) Option<GamepadId>);

impl PadFilter {
    fn pads<'a>(self, input: &'a InputState) -> impl Iterator<Item = crate::Gamepad<'a>> + 'a {
        let only = self.0;
        input
            .gamepads()
            .filter(move |g| only.is_none_or(|id| id == g.id()))
    }
}

/// How a source reads at one moment and across the current interval.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Reading {
    /// The signed level now: 0 or 1 for buttons, the axis value for axes.
    pub(crate) level: f32,
    /// The signed level at the start of the interval.
    pub(crate) before: f32,
    pub(crate) pressed: bool,
    pub(crate) released: bool,
}

pub(crate) fn read(source: Source, input: &InputState, pads: PadFilter, dead_zone: f32) -> Reading {
    let button = |down: bool, pressed: bool, released: bool| Reading {
        level: f32::from(u8::from(down)),
        // The level before the interval: a press means it was up, a
        // release without a press means it was down.
        before: f32::from(u8::from(!pressed && (released || down))),
        pressed,
        released,
    };
    match source {
        Source::Key(k) => button(
            input.key_down(k),
            input.key_pressed(k),
            input.key_released(k),
        ),
        Source::Mouse(b) => button(
            input.mouse_down(b),
            input.mouse_pressed(b),
            input.mouse_released(b),
        ),
        Source::PadButton(b) => pads.pads(input).fold(Reading::default(), |acc, g| {
            let r = button(g.down(b), g.pressed(b), g.released(b));
            Reading {
                level: acc.level.max(r.level),
                before: acc.before.max(r.before),
                pressed: acc.pressed || r.pressed,
                released: acc.released || r.released,
            }
        }),
        Source::PadAxis(a) => {
            let dz = |v: f32| if v.abs() < dead_zone { 0.0 } else { v };
            pads.pads(input).fold(Reading::default(), |acc, g| {
                let now = dz(g.raw_axis(a));
                let before = dz(g.previous_axis(a));
                Reading {
                    level: if now.abs() > acc.level.abs() {
                        now
                    } else {
                        acc.level
                    },
                    before: if before.abs() > acc.before.abs() {
                        before
                    } else {
                        acc.before
                    },
                    pressed: false,
                    released: false,
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_text_round_trip() {
        let cases = [
            ("key:Space", Binding::from(Key::Space)),
            (
                "-axis:LeftX",
                Source::PadAxis(GamepadAxis::LeftX).negative(),
            ),
            (
                "axis:RightTrigger*0.5",
                Source::PadAxis(GamepadAxis::RightTrigger).scaled(0.5),
            ),
            ("mouse:Left", Binding::from(MouseButton::Left)),
            ("pad:South", Binding::from(GamepadButton::South)),
        ];
        for (text, binding) in cases {
            assert_eq!(binding.to_string(), text);
            assert_eq!(text.parse::<Binding>().ok(), Some(binding), "{text}");
        }
        for bad in ["space", "key:Nope", "foo:Bar", "axis:LeftX*x"] {
            assert!(bad.parse::<Binding>().is_err(), "{bad}");
        }
    }
}
