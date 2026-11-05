//! Named actions bound to keys, buttons and axes.

use std::collections::HashMap;

use yowie_core::Vec2;

use crate::action::{AXIS_PRESS_THRESHOLD, PadFilter, Reading, read};
use crate::{
    Binding, Binding2, GamepadAxis, GamepadButton, GamepadId, InputState, Key, MouseButton, Source,
};

/// The default stick travel ignored around centre by action maps.
pub const DEFAULT_ACTION_DEAD_ZONE: f32 = 0.2;

/// What one action is bound to.
#[derive(Clone, Debug, PartialEq)]
pub enum Bindings {
    /// A one-axis action: a button such as "jump", or an axis such as
    /// "throttle" whose value sums its sources.
    Axis(Vec<Binding>),
    /// A two-axis action such as "move", read with
    /// [`ActionMap::vec2`].
    Vec2(Vec<Binding2>),
}

impl Bindings {
    fn sources(&self) -> Vec<(Source, Side)> {
        match self {
            Bindings::Axis(b) => b.iter().map(|b| (b.source, Side::of(b.scale))).collect(),
            Bindings::Vec2(b) => b.iter().map(|b| (b.source, Side::Either)).collect(),
        }
    }
}

/// Which part of an axis counts as active.
#[derive(Clone, Copy, Debug)]
enum Side {
    Positive,
    Negative,
    Either,
}

impl Side {
    fn of(scale: f32) -> Side {
        if scale < 0.0 {
            Side::Negative
        } else {
            Side::Positive
        }
    }

    fn active(self, source: Source, level: f32) -> bool {
        if !matches!(source, Source::PadAxis(_)) {
            return level > 0.5;
        }
        match self {
            Side::Positive => level >= AXIS_PRESS_THRESHOLD,
            Side::Negative => -level >= AXIS_PRESS_THRESHOLD,
            Side::Either => level.abs() >= AXIS_PRESS_THRESHOLD,
        }
    }
}

/// A resolved handle to one named action. It names the action rather than
/// its sources, so it stays valid across rebinding and
/// [`ActionMap::load_from`]. A handle from a different map reads that
/// map's action at the same position, or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActionId(pub(crate) u32);

/// Something that names an action: its name or an [`ActionId`].
pub trait ActionRef {
    /// The action's position in `map`, if it exists.
    fn resolve(&self, map: &ActionMap) -> Option<usize>;
}

impl ActionRef for ActionId {
    fn resolve(&self, map: &ActionMap) -> Option<usize> {
        let i = self.0 as usize;
        (i < map.entries.len()).then_some(i)
    }
}

impl ActionRef for &str {
    fn resolve(&self, map: &ActionMap) -> Option<usize> {
        map.index.get(*self).copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) bindings: Bindings,
}

/// Named actions ("jump", "fire", "move") bound to any number of keys,
/// buttons and axes, so game code asks for actions and a settings screen
/// rebinds them.
///
/// A one-axis action fires from any of its sources and its value sums
/// them, clamped to -1..1. A two-axis action sums each source's push and
/// limits the result to unit length, which turns WASD and a stick into one
/// `Vec2`. Bindings save and load through serde as
/// `{"jump": ["key:Space", "pad:South"]}`.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionMap {
    pub(crate) entries: Vec<Entry>,
    pub(crate) index: HashMap<String, usize>,
    gamepad: Option<GamepadId>,
    dead_zone: f32,
}

impl Default for ActionMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionMap {
    /// An empty map that reads every connected gamepad with the default
    /// dead zone.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
            gamepad: None,
            dead_zone: DEFAULT_ACTION_DEAD_ZONE,
        }
    }

    /// The handle for `name`, creating an unbound one-axis action if the
    /// name is new.
    pub fn action(&mut self, name: &str) -> ActionId {
        if let Some(&i) = self.index.get(name) {
            return ActionId(i as u32);
        }
        let i = self.entries.len();
        self.entries.push(Entry {
            name: name.to_owned(),
            bindings: Bindings::Axis(Vec::new()),
        });
        self.index.insert(name.to_owned(), i);
        ActionId(i as u32)
    }

    /// The handle for `name` if the map has it.
    pub fn find(&self, name: &str) -> Option<ActionId> {
        self.index.get(name).map(|&i| ActionId(i as u32))
    }

    /// Adds sources to a one-axis action. An action that was two-axis
    /// becomes one-axis with only these sources.
    pub fn bind<I, B>(&mut self, name: &str, bindings: I) -> ActionId
    where
        I: IntoIterator<Item = B>,
        B: Into<Binding>,
    {
        let id = self.action(name);
        let entry = &mut self.entries[id.0 as usize];
        let new = bindings.into_iter().map(Into::into);
        match &mut entry.bindings {
            Bindings::Axis(list) => list.extend(new),
            other => *other = Bindings::Axis(new.collect()),
        }
        id
    }

    /// Adds sources to a two-axis action. An action that was one-axis
    /// becomes two-axis with only these sources.
    pub fn bind_vec2<I>(&mut self, name: &str, bindings: I) -> ActionId
    where
        I: IntoIterator<Item = Binding2>,
    {
        let id = self.action(name);
        let entry = &mut self.entries[id.0 as usize];
        match &mut entry.bindings {
            Bindings::Vec2(list) => list.extend(bindings),
            other => *other = Bindings::Vec2(bindings.into_iter().collect()),
        }
        id
    }

    /// Replaces every source of a one-axis action with `binding`. Does
    /// nothing for an unknown action.
    pub fn rebind(&mut self, action: impl ActionRef, binding: Binding) {
        if let Some(i) = action.resolve(self) {
            self.entries[i].bindings = Bindings::Axis(vec![binding]);
        }
    }

    /// Replaces the source of the `index`th binding of a two-axis action,
    /// keeping its direction. Returns false when there is no such binding.
    pub fn rebind_vec2(&mut self, action: impl ActionRef, index: usize, source: Source) -> bool {
        let Some(i) = action.resolve(self) else {
            return false;
        };
        match &mut self.entries[i].bindings {
            Bindings::Vec2(list) => match list.get_mut(index) {
                Some(b) => {
                    b.source = source;
                    true
                }
                None => false,
            },
            Bindings::Axis(_) => false,
        }
    }

    /// Removes every source from an action; its handle stays valid.
    pub fn unbind(&mut self, action: impl ActionRef) {
        if let Some(i) = action.resolve(self) {
            match &mut self.entries[i].bindings {
                Bindings::Axis(l) => l.clear(),
                Bindings::Vec2(l) => l.clear(),
            }
        }
    }

    /// What an action is bound to.
    pub fn bindings(&self, action: impl ActionRef) -> Option<&Bindings> {
        action.resolve(self).map(|i| &self.entries[i].bindings)
    }

    /// Every action name, in the order the actions were created.
    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// Chooses which gamepad pad sources read; `None` reads every
    /// connected one.
    pub fn set_gamepad(&mut self, gamepad: Option<GamepadId>) {
        self.gamepad = gamepad;
    }

    /// The gamepad pad sources read; `None` means every connected one.
    pub fn gamepad(&self) -> Option<GamepadId> {
        self.gamepad
    }

    /// Sets the stick travel ignored around centre, `0..1`.
    pub fn set_dead_zone(&mut self, dead_zone: f32) {
        self.dead_zone = dead_zone.clamp(0.0, 0.99);
    }

    /// The stick travel ignored around centre.
    pub fn dead_zone(&self) -> f32 {
        self.dead_zone
    }

    /// Replaces the bindings of every action `saved` names, creating
    /// actions it has that this map lacks. Handles stay valid, so load a
    /// settings file into a new map and pass it here.
    pub fn load_from(&mut self, saved: &ActionMap) {
        for e in &saved.entries {
            let id = self.action(&e.name);
            self.entries[id.0 as usize].bindings = e.bindings.clone();
        }
    }

    fn readings(&self, action: impl ActionRef, input: &InputState) -> Vec<(Source, Side, Reading)> {
        let Some(i) = action.resolve(self) else {
            return Vec::new();
        };
        let pads = PadFilter(self.gamepad);
        self.entries[i]
            .bindings
            .sources()
            .into_iter()
            .map(|(s, side)| (s, side, read(s, input, pads, self.dead_zone)))
            .collect()
    }

    /// The action's value: for a one-axis action the sum of its sources
    /// clamped to -1..1, for a two-axis action the length of its vector.
    pub fn value(&self, input: &InputState, action: impl ActionRef) -> f32 {
        let Some(i) = action.resolve(self) else {
            return 0.0;
        };
        match &self.entries[i].bindings {
            Bindings::Axis(list) => {
                let pads = PadFilter(self.gamepad);
                let sum: f32 = list
                    .iter()
                    .map(|b| {
                        let level = read(b.source, input, pads, self.dead_zone).level;
                        if !matches!(b.source, Source::PadAxis(_)) {
                            return level * b.scale;
                        }
                        // An axis contributes only on the side its scale selects.
                        let sign = if b.scale < 0.0 { -1.0 } else { 1.0 };
                        sign * (level * sign).max(0.0) * b.scale.abs()
                    })
                    .sum();
                sum.clamp(-1.0, 1.0)
            }
            Bindings::Vec2(_) => self.vec2(input, ActionId(i as u32)).length(),
        }
    }

    /// The action's vector: for a two-axis action the sum of each source's
    /// push, limited to unit length; for a one-axis action `(value, 0)`.
    pub fn vec2(&self, input: &InputState, action: impl ActionRef) -> Vec2 {
        let Some(i) = action.resolve(self) else {
            return Vec2::ZERO;
        };
        match &self.entries[i].bindings {
            Bindings::Vec2(list) => {
                let pads = PadFilter(self.gamepad);
                let sum: Vec2 = list
                    .iter()
                    .map(|b| b.direction * read(b.source, input, pads, self.dead_zone).level)
                    .sum();
                sum.clamp_length_max(1.0)
            }
            Bindings::Axis(_) => Vec2::new(self.value(input, ActionId(i as u32)), 0.0),
        }
    }

    /// Whether any source is held: a key or button down, or an axis past
    /// [`AXIS_PRESS_THRESHOLD`](crate::AXIS_PRESS_THRESHOLD) on its side.
    pub fn down(&self, input: &InputState, action: impl ActionRef) -> bool {
        self.readings(action, input)
            .iter()
            .any(|(s, side, r)| side.active(*s, r.level))
    }

    /// Whether the action became held since the previous update (or, while
    /// drawing, since the previous frame). A second source going down
    /// while another is already held does not count.
    pub fn pressed(&self, input: &InputState, action: impl ActionRef) -> bool {
        let rs = self.readings(action, input);
        let was = rs.iter().any(|(s, side, r)| side.active(*s, r.before));
        let edge = rs.iter().any(|(s, side, r)| {
            r.pressed || side.active(*s, r.level) && !side.active(*s, r.before)
        });
        edge && !was
    }

    /// Whether the action stopped being held since the previous update
    /// (or, while drawing, since the previous frame).
    pub fn released(&self, input: &InputState, action: impl ActionRef) -> bool {
        let rs = self.readings(action, input);
        let now = rs.iter().any(|(s, side, r)| side.active(*s, r.level));
        let edge = rs.iter().any(|(s, side, r)| {
            r.released || side.active(*s, r.before) && !side.active(*s, r.level)
        });
        edge && !now
    }

    /// The first key, mouse button, gamepad button or stick flick since the
    /// previous update, for a rebinding screen. A stick flick returns the
    /// side it moved to, negated with [`Source::negative`] for the negative side.
    pub fn listen(&self, input: &InputState) -> Option<Binding> {
        if let Some(k) = Key::ALL.iter().find(|k| input.key_pressed(**k)) {
            return Some(Binding::from(*k));
        }
        if let Some(b) = MouseButton::ALL.iter().find(|b| input.mouse_pressed(**b)) {
            return Some(Binding::from(*b));
        }
        let pads = PadFilter(self.gamepad);
        for b in GamepadButton::ALL {
            if read(Source::PadButton(*b), input, pads, self.dead_zone).pressed {
                return Some(Binding::from(*b));
            }
        }
        for a in GamepadAxis::ALL {
            let r = read(Source::PadAxis(*a), input, pads, self.dead_zone);
            for side in [Side::Positive, Side::Negative] {
                let src = Source::PadAxis(*a);
                if side.active(src, r.level) && !side.active(src, r.before) {
                    return Some(match side {
                        Side::Negative => src.negative(),
                        _ => Binding::from(src),
                    });
                }
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "action_map_tests.rs"]
mod tests;
