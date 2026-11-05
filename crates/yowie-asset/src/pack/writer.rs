//! Writing pack files.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use super::{
    Compression, FOOTER_MAGIC, HEADER_MAGIC, METHOD_STORED, METHOD_ZSTD, VERSION, crc32,
    is_precompressed,
};
use crate::error::{Error, Result};
use crate::path::normalize;

struct IndexEntry {
    path: String,
    offset: u64,
    stored: u64,
    size: u64,
    method: u8,
    crc: u32,
}

/// Streams assets into a pack file.
///
/// ```
/// use yowie_asset::{Compression, PackWriter};
/// // Any `Write` works; pass a `File` to write a pack to disk.
/// let mut pack = PackWriter::new(Vec::new(), Compression::default())?;
/// pack.add("levels/1.json", b"{}")?;
/// let bytes = pack.finish()?;
/// assert!(!bytes.is_empty());
/// # Ok::<(), yowie_asset::Error>(())
/// ```
pub struct PackWriter<W: Write> {
    out: W,
    compression: Compression,
    pos: u64,
    entries: Vec<IndexEntry>,
    seen: HashSet<String>,
}

impl<W: Write> PackWriter<W> {
    /// Starts a pack on `out` and writes the header.
    pub fn new(mut out: W, compression: Compression) -> Result<Self> {
        let io = |e| Error::io("<pack>", e);
        out.write_all(HEADER_MAGIC).map_err(io)?;
        out.write_all(&VERSION.to_le_bytes()).map_err(io)?;
        Ok(Self {
            out,
            compression,
            pos: super::HEADER_LEN,
            entries: Vec::new(),
            seen: HashSet::new(),
        })
    }

    /// Adds an entry using the writer's compression setting.
    pub fn add(&mut self, path: &str, data: &[u8]) -> Result<()> {
        self.add_with(path, data, self.compression)
    }

    /// Adds an entry with its own compression setting, for example
    /// [`Compression::None`] for a file that is already compressed.
    ///
    /// `path` is normalised. An empty path, one that escapes the root, or a
    /// path already added is an error.
    pub fn add_with(&mut self, path: &str, data: &[u8], compression: Compression) -> Result<()> {
        let path = normalize(path)?;
        if path.is_empty() || path.len() > u16::MAX as usize {
            return Err(Error::InvalidPath(path));
        }
        if !self.seen.insert(path.clone()) {
            return Err(Error::DuplicateEntry(path));
        }
        let io = |e| Error::io("<pack>", e);
        let compressed = match compression {
            Compression::None => None,
            Compression::Zstd(level) => {
                crate::zstd::compress(data, level.clamp(1, 22)).filter(|c| c.len() < data.len())
            }
        };
        let (method, stored): (u8, &[u8]) = match &compressed {
            Some(c) => (METHOD_ZSTD, c),
            None => (METHOD_STORED, data),
        };
        self.out.write_all(stored).map_err(io)?;
        self.entries.push(IndexEntry {
            path,
            offset: self.pos,
            stored: stored.len() as u64,
            size: data.len() as u64,
            method,
            crc: crc32(data),
        });
        self.pos += stored.len() as u64;
        Ok(())
    }

    /// Writes the index and footer and returns the underlying writer.
    pub fn finish(mut self) -> Result<W> {
        let mut index = Vec::new();
        for e in &self.entries {
            index.extend_from_slice(&(e.path.len() as u16).to_le_bytes());
            index.extend_from_slice(e.path.as_bytes());
            index.extend_from_slice(&e.offset.to_le_bytes());
            index.extend_from_slice(&e.stored.to_le_bytes());
            index.extend_from_slice(&e.size.to_le_bytes());
            index.push(e.method);
            index.extend_from_slice(&e.crc.to_le_bytes());
        }
        let io = |e| Error::io("<pack>", e);
        self.out.write_all(&index).map_err(io)?;
        let mut footer = Vec::with_capacity(super::FOOTER_LEN as usize);
        footer.extend_from_slice(&self.pos.to_le_bytes());
        footer.extend_from_slice(&(index.len() as u64).to_le_bytes());
        footer.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        footer.extend_from_slice(&crc32(&index).to_le_bytes());
        footer.extend_from_slice(FOOTER_MAGIC);
        self.out.write_all(&footer).map_err(io)?;
        self.out.flush().map_err(io)?;
        Ok(self.out)
    }
}

/// Packs every file under `dir` into a pack file at `out`.
///
/// Entry paths are relative to `dir`, with forward slashes, and sorted so
/// the output is reproducible. Files with names starting with a dot are
/// skipped. Files in formats that are already compressed (images, audio,
/// video, archives) are stored as they are; the rest use `compression`.
/// The pack is written to `out` with `.tmp` appended, then renamed.
pub fn pack_dir(
    dir: impl AsRef<Path>,
    out: impl AsRef<Path>,
    compression: Compression,
) -> Result<()> {
    let (dir, out) = (dir.as_ref(), out.as_ref());
    let mut files = Vec::new();
    collect(dir, "", &mut files)?;
    files.sort();
    let tmp = out.with_extension("pak.tmp");
    let file = File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    let mut pack = PackWriter::new(BufWriter::new(file), compression)?;
    for rel in &files {
        let full = dir.join(rel);
        let data = fs::read(&full).map_err(|e| Error::io(&full, e))?;
        let method = if is_precompressed(rel) {
            Compression::None
        } else {
            compression
        };
        pack.add_with(rel, &data, method)?;
    }
    let mut writer = pack.finish()?;
    writer.flush().map_err(|e| Error::io(&tmp, e))?;
    drop(writer);
    fs::rename(&tmp, out).map_err(|e| Error::io(out, e))
}

fn collect(root: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    let here = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    for entry in fs::read_dir(&here).map_err(|e| Error::io(&here, e))? {
        let entry = entry.map_err(|e| Error::io(&here, e))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let kind = entry.file_type().map_err(|e| Error::io(entry.path(), e))?;
        if kind.is_dir() {
            collect(root, &child, out)?;
        } else if kind.is_file() {
            out.push(child);
        }
    }
    Ok(())
}
