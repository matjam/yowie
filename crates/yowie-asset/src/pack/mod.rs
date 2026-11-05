//! Yowie pack files: many assets in one file, each optionally compressed.
//!
//! # Format
//!
//! All integers are little-endian.
//!
//! ```text
//! header   "YWPK", u32 version (1)
//! data     each entry's stored bytes, back to back
//! index    per entry: u16 path length, path (UTF-8), u64 offset,
//!          u64 stored length, u64 uncompressed length,
//!          u8 method (0 stored, 1 zstd), u32 CRC-32 of the uncompressed bytes
//! footer   u64 index offset, u64 index length, u32 entry count,
//!          u32 CRC-32 of the index, "YWPE"
//! ```
//!
//! The index sits at the end so a writer can stream entries without seeking.
//! A reader checks the magic numbers, the index checksum and that every
//! entry lies inside the data area when the pack is opened, and checks an
//! entry's checksum each time it is read.

mod reader;
mod writer;

pub use reader::PackSource;
pub use writer::{PackWriter, pack_dir};

pub(crate) const HEADER_MAGIC: &[u8; 4] = b"YWPK";
pub(crate) const FOOTER_MAGIC: &[u8; 4] = b"YWPE";
pub(crate) const VERSION: u32 = 1;
pub(crate) const HEADER_LEN: u64 = 8;
pub(crate) const FOOTER_LEN: u64 = 28;
pub(crate) const METHOD_STORED: u8 = 0;
pub(crate) const METHOD_ZSTD: u8 = 1;

/// How a [`PackWriter`] compresses entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// Store every entry as it is.
    None,
    /// Compress each entry as a zstd frame at this level (1 to 22; higher
    /// is smaller and slower to write, with similar read speed). An entry
    /// that does not shrink is stored uncompressed.
    Zstd(i32),
}

impl Default for Compression {
    /// Level 19, suited to an offline packing tool.
    fn default() -> Self {
        Self::Zstd(19)
    }
}

/// Returns the CRC-32 of `data`.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(data);
    crc.sum()
}

/// Reports whether a file name has an extension of an already compressed
/// format, which [`pack_dir`] stores without compressing again.
pub(crate) fn is_precompressed(path: &str) -> bool {
    matches!(
        crate::path::extension(path).as_deref(),
        Some(
            "png"
                | "jpg"
                | "jpeg"
                | "webp"
                | "ogg"
                | "mp3"
                | "flac"
                | "mp4"
                | "webm"
                | "zip"
                | "zst"
                | "gz"
                | "ktx2"
                | "avif"
        )
    )
}

#[cfg(test)]
mod tests;
