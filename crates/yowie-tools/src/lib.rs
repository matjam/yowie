//! Command-line tools for Yowie and the code they share.
//!
//! | Tool | Library part |
//! |---|---|
//! | `yowie-tex` | [`encode`], [`mips`], [`container`] |
//! | `yowie-info` | [`info`] |
//! | `yowie-bundle` | [`bundle`], [`icns`] |
//! | `yowie-play` | [`play`] |
//!
//! To read a KTX2 texture in a renderer, call [`container::Ktx2File::parse`]
//! or [`container::Ktx2File::open`]. The result holds the Vulkan format, the
//! size and the bytes of each mip level, ready to upload.
#![warn(missing_docs)]

pub mod bundle;
mod compress;
pub mod container;
pub mod encode;
mod error;
pub mod format;
pub mod icns;
pub mod info;
pub mod mips;
pub mod play;

pub use container::Ktx2File;
pub use error::{Error, Result};
pub use format::BlockFormat;
