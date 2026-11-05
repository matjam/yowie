//! JSON saves and settings stored in the platform's data directory.
//!
//! A [`Store`] is a directory of named slots. Each slot is one JSON file,
//! `<name>.json`, holding `{"version": N, "data": ...}`. Writes are atomic:
//! the data goes to a temporary file in the same directory, is synced, and
//! is renamed over the slot, so a crash leaves either the old save or the
//! new one, never a torn file.
//!
//! - [`Store::open`] uses the platform data directory for an application
//!   (`~/.local/share`, `~/Library/Application Support`, `%APPDATA%`).
//! - [`Store::write_async`] saves on a worker thread and returns a
//!   [`SaveHandle`] to poll, so a large save does not stall a frame.
//! - [`Store::with_version`] and [`Store::migrate_with`] upgrade saves
//!   written by older versions of the game.
//!
//! Settings are an ordinary slot; [`Store::load_or`] returns defaults when
//! the slot does not exist yet.
//!
//! ```
//! use serde::{Deserialize, Serialize};
//! use yowie_save::Store;
//!
//! #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
//! struct Settings { volume: f32 }
//!
//! # let dir = tempfile::tempdir().unwrap();
//! let store = Store::at(dir.path()).unwrap();
//! store.write("settings", &Settings { volume: 0.5 }).unwrap();
//! let s: Settings = store.load_or("settings", Settings::default()).unwrap();
//! assert_eq!(s.volume, 0.5);
//! ```

#![warn(missing_docs)]

mod error;
mod migrate;
mod store;
mod worker;

pub use error::SaveError;
pub use migrate::Migration;
pub use store::Store;
pub use worker::SaveHandle;
