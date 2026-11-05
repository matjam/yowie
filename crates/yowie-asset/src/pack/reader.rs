//! Reading pack files as a mountable source.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use parking_lot::Mutex;

use super::{
    FOOTER_LEN, FOOTER_MAGIC, HEADER_LEN, HEADER_MAGIC, METHOD_STORED, METHOD_ZSTD, VERSION, crc32,
};
use crate::error::{Error, Result};
use crate::path::normalize;
use crate::source::{DirEntry, Source, is_dir_of, list_names};

/// Largest uncompressed entry a reader accepts, which bounds the memory a
/// corrupt index can make it allocate.
const MAX_ENTRY_SIZE: u64 = 1 << 32;

trait Backing: Read + Seek + Send {}
impl<T: Read + Seek + Send> Backing for T {}

#[derive(Clone, Copy)]
struct Entry {
    offset: u64,
    stored: u64,
    size: u64,
    method: u8,
    crc: u32,
}

/// A pack file mounted as a source.
pub struct PackSource {
    backing: Mutex<Box<dyn Backing>>,
    entries: HashMap<String, Entry>,
}

fn corrupt(msg: impl Into<String>) -> Error {
    Error::CorruptPack(msg.into())
}

impl PackSource {
    /// Opens a pack file and reads its index.
    ///
    /// The magic numbers, version, index checksum and every entry's bounds
    /// are checked; a pack that fails any of them is [`Error::CorruptPack`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|e| Error::io(path, e))?;
        Self::from_reader(file)
    }

    /// Opens a pack held in memory, for example one embedded with
    /// `include_bytes!`.
    pub fn from_bytes(bytes: impl Into<Cow<'static, [u8]>>) -> Result<Self> {
        Self::from_reader(Cursor::new(bytes.into()))
    }

    fn from_reader(mut r: impl Read + Seek + Send + 'static) -> Result<Self> {
        let io = |e| Error::io("<pack>", e);
        let len = r.seek(SeekFrom::End(0)).map_err(io)?;
        if len < HEADER_LEN + FOOTER_LEN {
            return Err(corrupt("file is too short"));
        }
        let mut head = [0u8; HEADER_LEN as usize];
        r.seek(SeekFrom::Start(0)).map_err(io)?;
        r.read_exact(&mut head).map_err(io)?;
        if &head[..4] != HEADER_MAGIC {
            return Err(corrupt("bad magic number"));
        }
        let version = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        if version != VERSION {
            return Err(corrupt(format!("unsupported version {version}")));
        }
        let mut foot = [0u8; FOOTER_LEN as usize];
        r.seek(SeekFrom::Start(len - FOOTER_LEN)).map_err(io)?;
        r.read_exact(&mut foot).map_err(io)?;
        if &foot[24..] != FOOTER_MAGIC {
            return Err(corrupt("bad footer magic"));
        }
        let index_offset =
            u64::from_le_bytes(foot[0..8].try_into().map_err(|_| corrupt("footer"))?);
        let index_len = u64::from_le_bytes(foot[8..16].try_into().map_err(|_| corrupt("footer"))?);
        let count = u32::from_le_bytes(foot[16..20].try_into().map_err(|_| corrupt("footer"))?);
        let index_crc = u32::from_le_bytes(foot[20..24].try_into().map_err(|_| corrupt("footer"))?);
        if index_offset < HEADER_LEN
            || index_offset.checked_add(index_len) != Some(len - FOOTER_LEN)
        {
            return Err(corrupt("index lies outside the file"));
        }
        let mut index = vec![0u8; index_len as usize];
        r.seek(SeekFrom::Start(index_offset)).map_err(io)?;
        r.read_exact(&mut index).map_err(io)?;
        if crc32(&index) != index_crc {
            return Err(corrupt("index fails its checksum"));
        }
        let entries = parse_index(&index, count, index_offset)?;
        Ok(Self {
            backing: Mutex::new(Box::new(r)),
            entries,
        })
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Reports whether the pack has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns every entry path, sorted.
    pub fn paths(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.entries.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    /// Reads every entry and checks its checksum. Use it to validate a
    /// downloaded pack; normal reads check only the entry they return.
    pub fn verify(&self) -> Result<()> {
        for path in self.paths() {
            self.read(path)?;
        }
        Ok(())
    }
}

fn parse_index(index: &[u8], count: u32, data_end: u64) -> Result<HashMap<String, Entry>> {
    let mut pos = 0usize;
    let mut take = |n: usize| -> Result<&[u8]> {
        let end = pos.checked_add(n).filter(|&e| e <= index.len());
        let end = end.ok_or_else(|| corrupt("index entry is truncated"))?;
        let s = &index[pos..end];
        pos = end;
        Ok(s)
    };
    let mut entries = HashMap::with_capacity(count.min(1 << 16) as usize);
    for _ in 0..count {
        let plen = u16::from_le_bytes(take(2)?.try_into().map_err(|_| corrupt("index"))?) as usize;
        let path = std::str::from_utf8(take(plen)?)
            .map_err(|_| corrupt("entry path is not UTF-8"))?
            .to_string();
        let mut u64_at = || -> Result<u64> {
            Ok(u64::from_le_bytes(
                take(8)?.try_into().map_err(|_| corrupt("index"))?,
            ))
        };
        let (offset, stored, size) = (u64_at()?, u64_at()?, u64_at()?);
        let method = take(1)?[0];
        let crc = u32::from_le_bytes(take(4)?.try_into().map_err(|_| corrupt("index"))?);
        if normalize(&path).ok().as_deref() != Some(path.as_str()) || path.is_empty() {
            return Err(corrupt(format!("entry path {path:?} is not normalised")));
        }
        if offset < HEADER_LEN || offset.checked_add(stored).is_none_or(|e| e > data_end) {
            return Err(corrupt(format!("entry {path} lies outside the data area")));
        }
        if size > MAX_ENTRY_SIZE || (method == METHOD_STORED && size != stored) {
            return Err(corrupt(format!("entry {path} has an impossible size")));
        }
        if method != METHOD_STORED && method != METHOD_ZSTD {
            return Err(corrupt(format!("entry {path} has unknown method {method}")));
        }
        let entry = Entry {
            offset,
            stored,
            size,
            method,
            crc,
        };
        if entries.insert(path.clone(), entry).is_some() {
            return Err(corrupt(format!("duplicate entry {path}")));
        }
    }
    if pos != index.len() {
        return Err(corrupt("index has trailing bytes"));
    }
    Ok(entries)
}

impl Source for PackSource {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let e = *self
            .entries
            .get(path)
            .ok_or_else(|| Error::NotFound(path.to_string()))?;
        let mut stored = vec![0u8; e.stored as usize];
        {
            let mut r = self.backing.lock();
            let io = |err| Error::io("<pack>", err);
            r.seek(SeekFrom::Start(e.offset)).map_err(io)?;
            r.read_exact(&mut stored).map_err(io)?;
        }
        let data = if e.method == METHOD_ZSTD {
            crate::zstd::decompress(&stored, e.size as usize).map_err(|_| Error::Checksum {
                path: path.to_string(),
            })?
        } else {
            stored
        };
        if data.len() as u64 != e.size || crc32(&data) != e.crc {
            return Err(Error::Checksum {
                path: path.to_string(),
            });
        }
        Ok(data)
    }

    fn exists(&self, path: &str) -> bool {
        path.is_empty() || self.entries.contains_key(path) || is_dir_of(self.entries.keys(), path)
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>> {
        list_names(self.entries.keys().map(String::as_str), path)
    }
}
