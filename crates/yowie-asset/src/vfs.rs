//! The virtual file system: mounts searched in priority order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::pack::PackSource;
use crate::path::normalize;
use crate::source::{DirEntry, DirSource, MemorySource, Source};

/// A stack of mounted sources that resolves asset paths.
///
/// Mounts are searched in the order they were added, so the first mount has
/// the highest priority: mount a developer's loose directory before the
/// shipped pack and the loose file wins. Only a missing name falls through
/// to the next mount; any other error is returned.
///
/// `Vfs` is cheap to clone (it shares its mounts), so loaders and worker
/// threads can each hold one.
#[derive(Clone, Default)]
pub struct Vfs {
    sources: Vec<Arc<dyn Source>>,
}

impl Vfs {
    /// A file system with no mounts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mounts any [`Source`] below every mount added so far.
    pub fn mount(&mut self, source: impl Source + 'static) -> &mut Self {
        self.sources.push(Arc::new(source));
        self
    }

    /// Mounts a directory on disk. The directory must exist.
    pub fn mount_dir(&mut self, dir: impl AsRef<Path>) -> Result<&mut Self> {
        let source = DirSource::new(dir)?;
        Ok(self.mount(source))
    }

    /// Mounts a pack file written by [`PackWriter`](crate::PackWriter) or
    /// `yowie-pack`. The index is read and verified now; entries are read
    /// on demand.
    pub fn mount_pack(&mut self, file: impl AsRef<Path>) -> Result<&mut Self> {
        let source = PackSource::open(file)?;
        Ok(self.mount(source))
    }

    /// Mounts an in-memory set of files.
    pub fn mount_memory(&mut self, files: MemorySource) -> &mut Self {
        self.mount(files)
    }

    /// Reads the file at `path` from the highest-priority mount that has it.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let path = normalize(path)?;
        for source in &self.sources {
            match source.read(&path) {
                Err(Error::NotFound(_)) => {}
                other => return other,
            }
        }
        Err(Error::NotFound(path))
    }

    /// Reads a file as UTF-8 text. Invalid bytes are an
    /// [`Error::Decode`].
    pub fn read_to_string(&self, path: &str) -> Result<String> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes).map_err(|e| Error::Decode {
            path: path.to_string(),
            source: Box::new(e),
        })
    }

    /// Reports whether any mount has a file or directory at `path`.
    pub fn exists(&self, path: &str) -> bool {
        normalize(path).is_ok_and(|p| self.sources.iter().any(|s| s.exists(&p)))
    }

    /// Lists the directory at `path` across all mounts.
    ///
    /// Entries from every mount are merged and sorted by name; when two
    /// mounts have the same name, the higher-priority mount decides whether
    /// it is a file or a directory. `""` lists the root.
    pub fn list(&self, path: &str) -> Result<Vec<DirEntry>> {
        let path = normalize(path)?;
        let mut merged: BTreeMap<String, DirEntry> = BTreeMap::new();
        let mut found = false;
        for source in &self.sources {
            match source.list(&path) {
                Ok(entries) => {
                    found = true;
                    for e in entries {
                        merged.entry(e.name.clone()).or_insert(e);
                    }
                }
                Err(Error::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        if !found && !path.is_empty() {
            return Err(Error::NotFound(path));
        }
        Ok(merged.into_values().collect())
    }

    /// Returns the loose file that currently provides `path`, or `None` when
    /// the winning mount is a pack or memory set (or the path is missing).
    pub fn real_path(&self, path: &str) -> Option<PathBuf> {
        let path = normalize(path).ok()?;
        self.sources
            .iter()
            .find(|s| s.exists(&path))
            .and_then(|s| s.real_path(&path))
    }

    /// Returns the root of every mounted directory, highest priority first.
    /// A hot reload watcher watches these.
    pub fn dir_roots(&self) -> Vec<PathBuf> {
        self.sources
            .iter()
            .filter_map(|s| s.root_dir().map(Path::to_path_buf))
            .collect()
    }

    /// Returns the number of mounts.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Reports whether nothing is mounted.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

impl std::fmt::Debug for Vfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vfs")
            .field("mounts", &self.sources.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(files: &[(&str, &'static str)]) -> MemorySource {
        let mut m = MemorySource::new();
        for (p, d) in files {
            m.insert(p, d.as_bytes()).unwrap();
        }
        m
    }

    #[test]
    fn first_mount_wins() {
        let mut vfs = Vfs::new();
        vfs.mount_memory(mem(&[("a.txt", "high"), ("only_high.txt", "h")]));
        vfs.mount_memory(mem(&[("a.txt", "low"), ("only_low.txt", "l")]));
        assert_eq!(vfs.read("a.txt").unwrap(), b"high");
        assert_eq!(vfs.read("only_low.txt").unwrap(), b"l");
        assert_eq!(vfs.read("//./a.txt").unwrap(), b"high");
        assert!(vfs.read("none").unwrap_err().is_not_found());
        assert!(vfs.exists("only_high.txt"));
        assert!(!vfs.exists("../x"));
    }

    #[test]
    fn directory_mount_overrides_pack_and_lists_merge() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("d")).unwrap();
        std::fs::write(tmp.path().join("d/x.txt"), b"loose").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_dir(tmp.path()).unwrap();
        vfs.mount_memory(mem(&[("d/x.txt", "packed"), ("d/y.txt", "y")]));
        assert_eq!(vfs.read("d/x.txt").unwrap(), b"loose");
        assert_eq!(vfs.read("d/y.txt").unwrap(), b"y");
        let names: Vec<_> = vfs.list("d").unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["x.txt", "y.txt"]);
        assert!(vfs.real_path("d/x.txt").is_some());
        assert!(vfs.real_path("d/y.txt").is_none());
        assert!(vfs.list("zzz").unwrap_err().is_not_found());
        assert_eq!(vfs.dir_roots().len(), 1);
    }
}
