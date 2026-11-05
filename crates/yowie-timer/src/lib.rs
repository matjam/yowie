//! Timers that run on game time, and step sequences for cutscenes.
//!
//! Nothing here reads a clock. Every type advances by the `dt` (seconds of
//! game time) the caller passes, so timers pause with the game, follow time
//! scaling and replay deterministically.
//!
//! - [`Timer`] is one countdown, one-shot or repeating, that can be paused.
//! - [`Timers`] holds many timers with user data and reports which fired on
//!   each update.
//! - [`Sequence`] runs steps in order: waits, calls, waits for a condition,
//!   and parallel groups.

#![warn(missing_docs)]

mod sequence;
mod timer;
mod timers;

pub use sequence::Sequence;
pub use timer::Timer;
pub use timers::{Fired, TimerId, Timers};
