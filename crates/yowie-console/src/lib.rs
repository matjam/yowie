//! The in-game debug console: a drop-down command line, typed variables,
//! key bindings, log capture and a window of panels for the engine's
//! subsystems.
//!
//! The engine builds a [`Console`] when `Config::console` is set, installs
//! the log tee, routes input to it while it is open and draws it last, so
//! a game only registers what it wants to expose:
//!
//! ```ignore
//! fn init(ctx: &mut Context) -> GameResult<Self> {
//!     let speed = ctx.console.float("player.speed", 5.0, "how fast the player runs");
//!     ctx.console.register("give", "give <item>: add an item", |args| {
//!         match args.first() {
//!             Some(item) => Ok(format!("gave {item}")),
//!             None => Err("give: needs an item".into()),
//!         }
//!     });
//!     ctx.console.add_info("seed", || "1234".to_string());
//!     Ok(Game { speed })
//! }
//! ```
//!
//! | Key | Action |
//! |---|---|
//! | `` ` `` | open and close the drop-down |
//! | F4 | open and close the panels |
//! | Up, Down | earlier and later commands |
//! | Tab | complete a command, variable, key or argument |
//! | PageUp, PageDown, wheel | scroll the output |
//! | Enter | run the line |
//! | Escape | close the drop-down |
//!
//! While the drop-down is open it has the keyboard; the engine keeps
//! those keys from reaching the game. Key bindings made with `bind` fire
//! only while it is shut.
//!
//! Panels for the frame timings, the GPU, the input devices and the
//! console's variables are always available. The cargo features `ecs`,
//! `phys` and `audio` add the Entities, Physics and Audio panels; a game
//! that does not use those crates does not compile them. The game feeds
//! them: [`Console::feed_world`], [`Console::feed_physics_2d`] and
//! [`Console::attach_mixer`].

#![warn(missing_docs)]

mod complete;
mod console;
mod dropdown;
mod exec;
#[cfg(test)]
mod gpu_tests;
mod history;
mod logcap;
mod panels;
mod parse;
mod sim;
mod vars;

pub use complete::{Completion, CustomCompleter, Names, complete};
pub use console::{CommandResult, Console, ConsoleOptions, FileReader, FrameTimes, Request};
pub use history::History;
pub use logcap::{LogCapture, LogLine, TeeLogger, install_logger};
pub use parse::{ParseError, last_word, split_words};
pub use vars::{BoolVar, FloatVar, IntVar, StringVar};
