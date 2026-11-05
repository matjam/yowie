//! Particle systems simulated on the CPU, with emitters described in JSON.
//!
//! An [`Emitter`] is plain data: how particles are born ([`Shape`], rate,
//! bursts), how they move (speed, gravity, drag, [`Attractor`]s) and how
//! they look over their life (size, colour gradient, alpha, frames,
//! [`Blend`]). It reads from and writes to JSON, so effects can be tuned in
//! a file. A [`System`] runs one emitter: [`System::update`] simulates and
//! [`System::draw`] queues every particle through the renderer's sprite
//! batch as one draw call.
//!
//! ```
//! use yowie_core::vec2;
//! use yowie_particle::{Emitter, System};
//!
//! let emitter = Emitter::from_json(
//!     r#"{ "rate": 100, "lifetime": [0.5, 1.0], "speed": 60,
//!          "shape": { "kind": "circle", "radius": 6 },
//!          "gravity": [0, 90], "blend": "additive" }"#,
//! )?;
//! let mut system = System::new(emitter);
//! system.set_position(vec2(200.0, 120.0));
//! for _ in 0..60 {
//!     system.update(1.0 / 60.0);
//! }
//! assert!(system.alive() > 40);
//! # Ok::<(), yowie_particle::ParticleError>(())
//! ```
//!
//! [`presets`] has a fire, smoke, sparks, rain and confetti to start from.
//! The GPU-instanced path comes later and will share [`Emitter`] and the
//! birth code of the CPU system.

#![warn(missing_docs)]

mod curve;
mod draw;
mod emitter;
mod error;
pub mod presets;
mod spawn;
mod system;

pub use curve::{ColorKey, Curve, Range, gradient_at};
pub use emitter::{Attractor, Blend, Burst, Emitter, Shape};
pub use error::{ParticleError, Result};
pub use spawn::Particle;
pub use system::System;

#[cfg(test)]
mod tests;
