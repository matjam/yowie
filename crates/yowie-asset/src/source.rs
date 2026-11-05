//! Mountable sources of asset bytes: directories and in-memory sets.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::path::normalize;

/// One entry of a directory listing.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirEntry {
    /// The entry's file name, without its directory.
    pub name: String,
    /// True when the entry is a directory.
    pub is_dir: bool,
}

/// Something a [`Vfs`](crate::Vfs) can mount. Paths are normalised before
/// they reach a source: forward slashes, no leading slash, `""` for the root.
pub trait Source: Send + Sync {
    /// Returns the bytes at `path`, or [`Error::NotFound`].
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    /// Reports whether `path` names a file or directory.
    fn exists(&self, path: &str) -> bool;
    /// Lists the directory at `path`, or [`Error::NotFound`] when it is not one.
    fn list(&self, path: &str) -> Result<Vec<DirEntry>>;
    /// Returns the loose file behind `path`, for sources backed by a directory.
    fn real_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
    /// Returns the directory this source reads from, if it is one.
    fn root_dir(&self) -> Option<&Path> {
        None
    }
}

/// A directory on disk, mounted at the root of the asset tree.
#[derive(Debug)]
pub struct DirSource {
    root: PathBuf,
}

impl DirSource {
    /// Opens `root`. The directory must exist; its path is canonicalised so
    /// file watcher events can be mapped back to asset paths.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let root = fs::canonicalize(root).map_err(|e| Error::io(root, e))?;
        if !root.is_dir() {
            return Err(Error::io(
                root,
                std::io::Error::new(ErrorKind::NotADirectory, "not a directory"),
            ));
        }
        Ok(Self { root })
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let mut full = self.root.clone();
        full.extend(path.split('/').filter(|p| !p.is_empty()));
        full
    }
}

impl Source for DirSource {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        let full = self.resolve(path);
        fs::read(&full).map_err(|e| match e.kind() {
            ErrorKind::NotFound | ErrorKind::NotADirectory | ErrorKind::IsADirectory => {
                Error::NotFound(path.to_string())
            }
            // Windows reports a directory opened as a file as access denied.
            _ if full.is_dir() => Error::NotFound(path.to_string()),
            _ => Error::io(full, e),
        })
    }

    fn exists(&self, path: &str) -> bool {
        self.resolve(path).exists()
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>> {
        let full = self.resolve(path);
        let read = fs::read_dir(&full).map_err(|e| match e.kind() {
            ErrorKind::NotFound | ErrorKind::NotADirectory => Error::NotFound(path.to_string()),
            _ => Error::io(&full, e),
        })?;
        let mut out = Vec::new();
        for entry in read {
            let entry = entry.map_err(|e| Error::io(&full, e))?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            out.push(DirEntry { name, is_dir });
        }
        out.sort();
        Ok(out)
    }

    fn real_path(&self, path: &str) -> Option<PathBuf> {
        let full = self.resolve(path);
        full.is_file().then_some(full)
    }

    fn root_dir(&self) -> Option<&Path> {
        Some(&self.root)
    }
}

/// A set of named byte blobs held in memory, for assets embedded in the
/// executable with `include_bytes!` or generated at run time.
#[derive(Debug, Default)]
pub struct MemorySource {
    files: BTreeMap<String, Cow<'static, [u8]>>,
}

impl MemorySource {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file and returns the set. `path` is normalised; an invalid
    /// path is an error.
    pub fn with(mut self, path: &str, data: impl Into<Cow<'static, [u8]>>) -> Result<Self> {
        self.insert(path, data)?;
        Ok(self)
    }

    /// Adds or replaces a file. `path` is normalised.
    pub fn insert(&mut self, path: &str, data: impl Into<Cow<'static, [u8]>>) -> Result<()> {
        let path = normalize(path)?;
        if path.is_empty() {
            return Err(Error::InvalidPath(path));
        }
        self.files.insert(path, data.into());
        Ok(())
    }
}

impl Source for MemorySource {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.files
            .get(path)
            .map(|d| d.to_vec())
            .ok_or_else(|| Error::NotFound(path.to_string()))
    }

    fn exists(&self, path: &str) -> bool {
        path.is_empty() || self.files.contains_key(path) || is_dir_of(self.files.keys(), path)
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>> {
        list_names(self.files.keys().map(String::as_str), path)
    }
}

/// Reports whether any of `names` lies below the directory `dir`.
pub(crate) fn is_dir_of<'a, S: AsRef<str> + 'a>(
    names: impl IntoIterator<Item = S>,
    dir: &str,
) -> bool {
    let prefix = format!("{dir}/");
    names.into_iter().any(|n| n.as_ref().starts_with(&prefix))
}

/// Lists the direct children of `dir` among slash-separated `names`.
pub(crate) fn list_names<'a>(
    names: impl Iterator<Item = &'a str>,
    dir: &str,
) -> Result<Vec<DirEntry>> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut seen = BTreeSet::new();
    let mut any = dir.is_empty();
    for name in names {
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        any = true;
        match rest.split_once('/') {
            Some((d, _)) => seen.insert(DirEntry {
                name: d.to_string(),
                is_dir: true,
            }),
            None => seen.insert(DirEntry {
                name: rest.to_string(),
                is_dir: false,
            }),
        };
    }
    if !any {
        return Err(Error::NotFound(dir.to_string()));
    }
    Ok(seen.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_lists_and_reads() {
        let mem = MemorySource::new()
            .with("a/b.txt", &b"hi"[..])
            .unwrap()
            .with("a/c/d.txt", &b"yo"[..])
            .unwrap()
            .with("top.txt", &b"t"[..])
            .unwrap();
        assert_eq!(mem.read("a/b.txt").unwrap(), b"hi");
        assert!(mem.exists("a/c"));
        assert!(!mem.exists("a/x"));
        let root: Vec<_> = mem
            .list("")
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.is_dir))
            .collect();
        assert_eq!(root, [("a".into(), true), ("top.txt".into(), false)]);
        assert!(mem.list("nope").unwrap_err().is_not_found());
    }

    #[test]
    fn dir_source_reads_and_rejects_missing() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub/f.txt"), b"data").unwrap();
        let dir = DirSource::new(tmp.path()).unwrap();
        assert_eq!(dir.read("sub/f.txt").unwrap(), b"data");
        assert!(dir.read("sub/none").unwrap_err().is_not_found());
        assert!(dir.read("sub").unwrap_err().is_not_found());
        assert!(dir.exists("sub"));
        assert_eq!(dir.list("sub").unwrap()[0].name, "f.txt");
        assert!(DirSource::new(tmp.path().join("missing")).is_err());
    }
}
