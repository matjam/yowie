//! A file watcher over directory mounts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crossbeam_channel::{Receiver, unbounded};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::error::Result;
use crate::vfs::Vfs;

/// Reports asset paths whose loose files changed on disk.
///
/// A watcher covers directory mounts only; packs and in-memory sources
/// cannot change. Events arrive from a background thread owned by the
/// operating system's notification service; call [`poll`](Watcher::poll)
/// from the game thread to collect them. Editors often save by writing a
/// temporary file and renaming it, so one save can report the path more
/// than once across polls.
pub struct Watcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    roots: Vec<PathBuf>,
}

impl Watcher {
    /// Watches every directory mounted in `vfs`.
    pub fn for_vfs(vfs: &Vfs) -> Result<Self> {
        Self::new(vfs.dir_roots())
    }

    /// Watches the given directories recursively. Each must exist and
    /// should be canonical (as [`Vfs::dir_roots`] returns them) so event
    /// paths can be made relative to it.
    pub fn new(roots: Vec<PathBuf>) -> Result<Self> {
        let (tx, rx) = unbounded();
        let mut watcher = notify::recommended_watcher(move |res| {
            // The receiver is gone only when the Watcher is being dropped.
            let _ = tx.send(res);
        })?;
        for root in &roots {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }
        Ok(Self {
            _watcher: watcher,
            rx,
            roots,
        })
    }

    /// Returns the asset paths changed since the last call, sorted and
    /// without duplicates. Created, modified, renamed and removed files
    /// are all reported; directories are not.
    pub fn poll(&self) -> Vec<String> {
        let mut changed = BTreeSet::new();
        while let Ok(res) = self.rx.try_recv() {
            let event = match res {
                Ok(event) => event,
                Err(e) => {
                    log::warn!("file watcher error: {e}");
                    continue;
                }
            };
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                continue;
            }
            for path in &event.paths {
                if path.is_dir() {
                    continue;
                }
                if let Some(asset) = self.asset_path(path) {
                    changed.insert(asset);
                }
            }
        }
        changed.into_iter().collect()
    }

    fn asset_path(&self, path: &Path) -> Option<String> {
        let root = self.roots.iter().find(|r| path.starts_with(r))?;
        let rel = path.strip_prefix(root).ok()?;
        let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
        let joined = parts?.join("/");
        (!joined.is_empty()).then_some(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Polls until `want` is reported or five seconds pass.
    fn wait_for(w: &Watcher, want: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if w.poll().iter().any(|p| p == want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn reports_created_and_modified_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_dir(tmp.path()).unwrap();
        let watcher = Watcher::for_vfs(&vfs).unwrap();
        std::fs::write(tmp.path().join("sub/a.txt"), b"1").unwrap();
        assert!(wait_for(&watcher, "sub/a.txt"), "create not reported");
        std::fs::write(tmp.path().join("sub/a.txt"), b"22").unwrap();
        assert!(wait_for(&watcher, "sub/a.txt"), "modify not reported");
    }
}
