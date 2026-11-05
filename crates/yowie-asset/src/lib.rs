//! Files from directories and pack files, one-call loaders, background
//! loading, and hot reload that notices changed files.
//!
//! # Overview
//!
//! - [`Vfs`] mounts directories, pack files and in-memory sets in priority
//!   order and reads, lists and checks paths across them. Paths are
//!   normalised to forward slashes with no leading slash.
//! - [`PackWriter`], [`pack_dir`] and the `yowie-pack` binary write pack
//!   files; [`PackSource`] reads them. See the [`pack`] module for the format.
//! - [`Assets`] holds one [`Loader`] per asset type and returns typed
//!   [`Handle`]s, loaded on the calling thread ([`Assets::load_now`]) or on
//!   a worker pool ([`Assets::load`]) with [`Progress`] reporting.
//!
//! # Loading screen
//!
//! ```
//! # use yowie_asset::*;
//! # struct Text;
//! # impl Loader for Text { type Asset = String;
//! #   fn load(&self, b: &[u8], _: &str, _: &LoadContext<'_>) -> std::result::Result<String, BoxError> { Ok(String::from_utf8(b.to_vec())?) } }
//! # let mut vfs = Vfs::new();
//! # vfs.mount_memory(MemorySource::new().with("a.txt", &b"a"[..])?);
//! let assets = Assets::new(vfs, 0)?;
//! assets.register(Text);
//! let a = assets.load::<String>("a.txt");
//! while !assets.progress().is_complete() {
//!     std::thread::yield_now(); // draw a bar from assets.progress().fraction()
//! }
//! for event in assets.poll() { /* react to AssetEvent::Loaded / Failed */ }
//! assert!(a.is_ready());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Hot reload and the renderer
//!
//! [`Assets::enable_hot_reload`] watches every directory mount. Each frame,
//! [`Assets::poll`] reports changed paths as [`AssetEvent::Changed`],
//! re-runs the loader of every live handle that is at, or read through its
//! [`LoadContext`], a changed path, and bumps [`Handle::generation`]. A
//! failed reload keeps the previous value.
//!
//! The renderer replaces GPU objects in place with this hook: for each GPU
//! object it keeps the generation of the handle it was built from. When
//! `handle.generation()` differs, or an [`AssetEvent::Reloaded`] names the
//! path, it calls `handle.get()` for the new CPU data and rewrites the
//! texture, buffer or pipeline contents without changing the object's
//! identity, so every sprite or material that refers to it sees the update.

#![warn(missing_docs)]

mod assets;
mod error;
mod handle;
mod loader;
pub mod pack;
mod path;
mod source;
mod vfs;
mod watch;
mod zstd;

pub use assets::{AssetEvent, Assets, Progress};
pub use error::{BoxError, Error, Result};
pub use handle::{Handle, LoadState};
pub use loader::{LoadContext, Loader};
pub use pack::{Compression, PackSource, PackWriter, pack_dir};
pub use path::{dir_of, extension, join, normalize};
pub use source::{DirEntry, DirSource, MemorySource, Source};
pub use vfs::Vfs;
pub use watch::Watcher;

#[cfg(test)]
mod tests;
