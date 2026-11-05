//! Reading and writing KTX2 files that hold one 2D block-compressed
//! texture.
//!
//! To read a texture, call [`Ktx2File::parse`] or [`Ktx2File::open`]. The
//! result holds the Vulkan format, the size and the bytes of each mip
//! level, ready to upload to a GPU texture of the same format. The
//! renderer (`yowie-gfx`) uploads them; `yowie-tex` writes them.
#![warn(missing_docs)]

pub mod container;
mod error;
pub mod format;

pub use container::Ktx2File;
pub use error::{Error, Result};
pub use format::{BlockFormat, mip_count, mip_size};
