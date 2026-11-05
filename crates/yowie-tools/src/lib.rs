//! Command-line tools for Yowie and the code they share.
//!
//! | Tool | Library part |
//! |---|---|
//! | `yowie-tex` | [`encode`], [`mips`], [`container`] |
//! | `yowie-info` | [`info`] |
//! | `yowie-bundle` | [`bundle`], [`icns`] |
//! | `yowie-play` | [`play`] |
//!
//! The KTX2 reader and writer live in `yowie-ktx2`, shared with the
//! renderer, and are re-exported here as [`container`] and [`format`](mod@format).
#![warn(missing_docs)]

pub mod bundle;
mod compress;
pub mod encode;
mod error;
pub mod icns;
pub mod info;
pub mod mips;
pub mod play;

pub use error::{Error, Result};
pub use yowie_ktx2::{BlockFormat, Ktx2File, container, format};
