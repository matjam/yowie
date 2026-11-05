//! Yowie: a game library for real-time and turn-based games that put 2D
//! sprites and 3D models on one screen.
//!
//! This crate is the engine: [`run`] opens the window, creates the
//! renderer and drives a [`Game`] through its callbacks with a
//! [`Context`]. Every subsystem is re-exported as a module (`yowie::gfx`,
//! `yowie::input` and the rest), and the shared maths, colour and geometry
//! types from `yowie-core` are re-exported at the root.
//!
//! ```no_run
//! use yowie::gfx::{Sprite, Texture, TextureOptions};
//! use yowie::input::Key;
//! use yowie::{Color, Config, Context, Game, GameResult, vec2};
//!
//! struct Demo {
//!     tex: Texture,
//!     x: f32,
//! }
//!
//! impl Game for Demo {
//!     fn init(ctx: &mut Context) -> GameResult<Self> {
//!         let tex = Texture::from_pixels(&ctx.gfx, 1, 1, &[255; 4], TextureOptions::default())?;
//!         Ok(Demo { tex, x: 0.0 })
//!     }
//!
//!     fn update(&mut self, ctx: &mut Context) -> GameResult {
//!         if ctx.input.key_down(Key::Right) {
//!             self.x += 200.0 * ctx.time.delta();
//!         }
//!         if ctx.input.key_pressed(Key::Escape) {
//!             ctx.quit();
//!         }
//!         Ok(())
//!     }
//!
//!     fn draw(&mut self, ctx: &mut Context) -> GameResult {
//!         let sprite = Sprite::at(vec2(self.x, 100.0)).size(vec2(32.0, 32.0)).tint(Color::ORANGE);
//!         ctx.gfx.draw_sprite(&self.tex, &sprite);
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> Result<(), yowie::Error> {
//!     yowie::run::<Demo>(Config::new("Demo"))
//! }
//! ```
//!
//! # The loop
//!
//! In [`LoopMode::FixedStep`], `update` runs at a fixed rate from an
//! accumulator of wall time (capped by [`Config::max_updates_per_frame`]
//! after a stall) and `draw` runs once per displayed frame;
//! [`Time::alpha`] says how far the frame falls between updates. In
//! [`LoopMode::TurnBased`] the process sleeps in the operating system until
//! input, a gamepad change, a [`Context::wake_after`] deadline, a
//! [`Waker`] or a [`Context::request_redraw`] runs a turn.
//!
//! Input edges belong to updates: a press between updates is seen by the
//! next update, and only the first of several updates in one frame sees it.
//! During `draw` the edges cover the whole frame instead.
//!
//! # Verifying a game without a window
//!
//! The engine reads three things from the command line and environment,
//! ignoring other arguments so games can have their own:
//!
//! - `--shot <png>` runs with no window (a GPU device with no surface),
//!   renders under the fixed clock and writes the last frame.
//! - `--seconds <n>` runs n seconds of game time, then exits; with
//!   `--shot` it renders that many seconds headless first.
//! - `YOWIE_FIXED_CLOCK=1` advances exactly one fixed step per frame, so a
//!   given frame is the same picture on every machine.
//!
//! # Audio
//!
//! `ctx.audio` is a [`audio::Mixer`]. Windowed runs open the default output
//! device unless [`Config::audio`] is off; headless runs, runs without a
//! device and runs with audio off get an offline mixer that the engine
//! renders every frame into nothing, so voices start, finish and call back
//! exactly as with a device and no sound is made. The engine calls
//! `Mixer::update` each frame.
//!
//! # Assets and hot reload
//!
//! `ctx.assets` reads from the `assets` directory in the working directory
//! and beside the executable, from [`Config::asset_dir`], and from pack
//! files (`assets.pak` in those places and [`Config::packs`]). Load GPU
//! objects with [`Context::load_texture`], [`Context::load_texture_async`],
//! [`Context::load_shader`], [`Context::load_font`] and
//! [`Context::load_sound`]. With [`Config::hot_reload`] (on in debug builds)
//! a changed file rewrites the texture or shader in place: the objects the
//! game already holds, and every sprite using them, show the new contents.
//! Textures load from PNG, JPEG, Radiance HDR and KTX2 (BC1, BC3, BC4, BC5
//! and BC7 where [`gfx::Graphics::supports_bc`] says so).
//!
//! # Window extras
//!
//! [`Config`] sets an icon, minimum and maximum size, position and
//! always-on-top; [`Context`] changes them while running and sets the
//! cursor ([`Cursor`]). Wayland compositors place windows and order them
//! themselves, so position and always-on-top have no effect there, and the
//! icon is ignored on macOS and Wayland. Drag and drop arrives as
//! [`Event::FileHovered`], [`Event::FileDropped`] and
//! [`Event::FileHoverCancelled`]; focus as [`Event::Focused`]. Frames are
//! not drawn while the window is occluded or minimised.
//!
//! F3 toggles a frame-timing overlay. A logger (`env_logger`, configured by
//! `RUST_LOG`) is installed when [`run`] starts unless one exists.

#![warn(missing_docs)]

mod app;
mod args;
#[cfg(feature = "audio")]
mod audio_backend;
mod cameras;
mod clipboard;
mod clock;
mod config;
mod config_window;
pub mod content;
mod context;
mod context_content;
mod context_window;
pub mod ecs_glue;
mod error;
mod game;
mod gamepad;
mod headless;
mod icon;
mod keymap;
mod overlay;
#[cfg(feature = "phys")]
mod phys_debug;
mod services;
#[cfg(test)]
mod services_tests;
mod stepper;
mod time;
mod window;

pub use cameras::{FlyCamera, OrbitCamera};
pub use config::{Config, LoopMode};
pub use context::{Context, CursorGrab, Waker};
pub use context_content::TextureLoad;
pub use error::{BoxError, Error, GameResult, Result};
pub use game::{Event, Game};
pub use icon::{Cursor, CursorImage, Icon};
#[cfg(feature = "phys")]
pub use phys_debug::SceneDebugDraw;
pub use time::Time;
pub use yowie_gfx::ScaleMode;

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

/// Runs `G` until it quits, its window closes, or `--seconds` of game time
/// pass. Returns the first error a callback, the renderer or the window
/// system produced.
///
/// Call it from `main`: window systems deliver events only to the thread
/// that created the connection, which on macOS must be the main thread.
pub fn run<G: Game>(config: Config) -> Result<()> {
    install_logger();
    let args = args::RunArgs::from_env().map_err(Error::Argument)?;
    if args.headless() {
        headless::run_headless::<G>(config, args)
    } else {
        app::run_windowed::<G>(config, args)
    }
}

/// Installs `env_logger` unless the program already set a logger.
fn install_logger() {
    let env = env_logger::Env::default().default_filter_or("warn,yowie=info,yowie_gfx=info");
    // An existing logger is the game's choice and stays.
    let _ = env_logger::Builder::from_env(env).try_init();
}
