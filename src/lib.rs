//! Yowie: a game library for real-time and turn-based games that put 2D
//! sprites and 3D models on one screen.
//!
//! This crate is the engine (the run loop, [`Context`] and headless mode)
//! and re-exports every subsystem as a module. Shared maths, colour and
//! geometry types from `yowie-core` are re-exported at the root.

pub use yowie_core::*;

pub use yowie_anim as anim;
pub use yowie_asset as asset;
pub use yowie_ecs as ecs;
pub use yowie_gfx as gfx;
pub use yowie_grid as grid;
pub use yowie_input as input;
pub use yowie_locale as locale;
pub use yowie_orbit as orbit;
pub use yowie_rng as rng;
pub use yowie_save as save;
pub use yowie_timer as timer;
pub use yowie_tween as tween;

#[cfg(feature = "audio")]
pub use yowie_audio as audio;
#[cfg(feature = "console")]
pub use yowie_console as console;
#[cfg(feature = "net")]
pub use yowie_net as net;
#[cfg(feature = "particle")]
pub use yowie_particle as particle;
#[cfg(feature = "phys")]
pub use yowie_phys as phys;
#[cfg(feature = "tiled")]
pub use yowie_tiled as tiled;
#[cfg(feature = "ui")]
pub use yowie_ui as ui;
