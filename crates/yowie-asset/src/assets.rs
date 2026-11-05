//! The asset manager: loader registry, background loading and hot reload.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::{Mutex, RwLock};

use crate::error::{Error, Result};
use crate::handle::{Handle, Reload, Slot};
use crate::loader::Loader;
use crate::path::normalize;
use crate::vfs::Vfs;
use crate::watch::Watcher;

/// Something that happened to an asset, collected by [`Assets::poll`].
#[derive(Clone, Debug)]
pub enum AssetEvent {
    /// A background load finished and the handle is ready.
    Loaded {
        /// The asset path.
        path: String,
        /// The Rust type name of the asset.
        type_name: &'static str,
    },
    /// A background load failed.
    Failed {
        /// The asset path.
        path: String,
        /// The Rust type name of the asset.
        type_name: &'static str,
        /// Why it failed.
        error: Arc<Error>,
    },
    /// A watched file changed on disk, whether or not a handle uses it.
    Changed {
        /// The asset path.
        path: String,
    },
    /// An asset was reloaded; its handle's generation has increased.
    Reloaded {
        /// The asset path.
        path: String,
        /// The Rust type name of the asset.
        type_name: &'static str,
    },
    /// A reload failed; the handle keeps its previous value.
    ReloadFailed {
        /// The asset path.
        path: String,
        /// The Rust type name of the asset.
        type_name: &'static str,
        /// Why it failed.
        error: Arc<Error>,
    },
}

/// How much background loading has finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Loads finished, whether they succeeded or failed.
    pub done: usize,
    /// Loads submitted since the manager was created or progress was reset.
    pub total: usize,
}

impl Progress {
    /// The finished share from 0.0 to 1.0, or 1.0 when nothing was submitted.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f32 / self.total as f32
        }
    }

    /// Reports whether every submitted load has finished.
    pub fn is_complete(&self) -> bool {
        self.done >= self.total
    }
}

struct CacheEntry {
    any: Weak<dyn Any + Send + Sync>,
    reload: Weak<dyn Reload>,
}

struct Inner {
    vfs: Vfs,
    loaders: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    cache: Mutex<HashMap<(TypeId, String), CacheEntry>>,
    pool: rayon::ThreadPool,
    done: AtomicUsize,
    total: AtomicUsize,
    events_tx: Sender<AssetEvent>,
    events_rx: Receiver<AssetEvent>,
    watcher: Mutex<Option<Watcher>>,
}

/// Loads typed assets from a [`Vfs`], in the background or on demand.
///
/// `Assets` is cheap to clone; clones share loaders, handles and workers.
///
/// ```
/// use yowie_asset::{Assets, BoxError, LoadContext, Loader, MemorySource, Vfs};
///
/// struct Text;
/// impl Loader for Text {
///     type Asset = String;
///     fn load(&self, b: &[u8], _: &str, _: &LoadContext<'_>) -> Result<String, BoxError> {
///         Ok(String::from_utf8(b.to_vec())?)
///     }
/// }
///
/// let mut vfs = Vfs::new();
/// vfs.mount_memory(MemorySource::new().with("hello.txt", &b"hi"[..])?);
/// let assets = Assets::new(vfs, 2)?;
/// assets.register(Text);
/// let hello = assets.load::<String>("hello.txt");
/// assert_eq!(*hello.wait()?, "hi");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct Assets {
    inner: Arc<Inner>,
}

type LoaderBox<T> = Arc<dyn Loader<Asset = T>>;

impl Assets {
    /// Creates a manager over `vfs` with `workers` background threads;
    /// 0 means one per CPU core.
    pub fn new(vfs: Vfs, workers: usize) -> Result<Self> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|i| format!("yowie-asset-{i}"))
            .build()
            .map_err(|e| Error::ThreadPool(e.to_string()))?;
        let (events_tx, events_rx) = unbounded();
        Ok(Self {
            inner: Arc::new(Inner {
                vfs,
                loaders: RwLock::new(HashMap::new()),
                cache: Mutex::new(HashMap::new()),
                pool,
                done: AtomicUsize::new(0),
                total: AtomicUsize::new(0),
                events_tx,
                events_rx,
                watcher: Mutex::new(None),
            }),
        })
    }

    /// The file system assets load from.
    pub fn vfs(&self) -> &Vfs {
        &self.inner.vfs
    }

    /// Registers the loader for `L::Asset`, replacing any earlier one.
    /// Handles already created keep the loader they started with.
    pub fn register<L: Loader>(&self, loader: L) {
        let arc: LoaderBox<L::Asset> = Arc::new(loader);
        self.inner
            .loaders
            .write()
            .insert(TypeId::of::<L::Asset>(), Box::new(arc));
    }

    fn slot<T: Send + Sync + 'static>(&self, path: &str) -> (Arc<Slot<T>>, bool) {
        let path = match normalize(path) {
            Ok(p) if !p.is_empty() => p,
            _ => {
                let err = Error::InvalidPath(path.to_string());
                return (
                    Slot::failed(path.to_string(), self.inner.vfs.clone(), err),
                    false,
                );
            }
        };
        let key = (TypeId::of::<T>(), path.clone());
        let mut cache = self.inner.cache.lock();
        if let Some(entry) = cache.get(&key)
            && let Some(any) = entry.any.upgrade()
            && let Ok(slot) = any.downcast::<Slot<T>>()
        {
            return (slot, false);
        }
        let loader = self
            .inner
            .loaders
            .read()
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<LoaderBox<T>>())
            .cloned();
        let Some(loader) = loader else {
            let err = Error::NoLoader(std::any::type_name::<T>());
            return (Slot::failed(path, self.inner.vfs.clone(), err), false);
        };
        let slot = Slot::new(path, self.inner.vfs.clone(), loader);
        let any: Arc<dyn Any + Send + Sync> = slot.clone();
        let reload: Arc<dyn Reload> = slot.clone();
        cache.insert(
            key,
            CacheEntry {
                any: Arc::downgrade(&any),
                reload: Arc::downgrade(&reload),
            },
        );
        (slot, true)
    }

    /// Starts loading `path` as a `T` on a worker thread and returns at
    /// once. Loading the same path and type again returns the same asset.
    ///
    /// A missing loader or invalid path gives a handle already in the
    /// failed state. Poll [`Assets::progress`] and [`Assets::poll`] to follow
    /// the load.
    pub fn load<T: Send + Sync + 'static>(&self, path: &str) -> Handle<T> {
        let (slot, fresh) = self.slot::<T>(path);
        if fresh {
            self.inner.total.fetch_add(1, Ordering::AcqRel);
            let (inner, job) = (self.inner.clone(), slot.clone());
            self.inner.pool.spawn(move || {
                let result = job.run();
                let (path, type_name) = (job.path().to_string(), job.type_name());
                inner.done.fetch_add(1, Ordering::AcqRel);
                let event = match result {
                    Ok(()) => AssetEvent::Loaded { path, type_name },
                    Err(error) => AssetEvent::Failed {
                        path,
                        type_name,
                        error,
                    },
                };
                // The receiver lives in `inner`, so the send cannot fail.
                let _ = inner.events_tx.send(event);
            });
        }
        Handle { slot }
    }

    /// Loads `path` as a `T` on the calling thread and returns the ready
    /// handle, or the error that stopped it. A path already loaded or
    /// loading is waited for rather than loaded twice.
    pub fn load_now<T: Send + Sync + 'static>(&self, path: &str) -> Result<Handle<T>> {
        let (slot, fresh) = self.slot::<T>(path);
        let handle = Handle { slot };
        if fresh {
            handle.slot.run().map_err(arc_to_error)?;
        } else {
            handle.wait().map_err(arc_to_error)?;
        }
        Ok(handle)
    }

    /// Returns how many background loads have finished out of those submitted.
    pub fn progress(&self) -> Progress {
        Progress {
            done: self.inner.done.load(Ordering::Acquire),
            total: self.inner.total.load(Ordering::Acquire),
        }
    }

    /// Zeroes the progress counters, for the start of the next loading
    /// screen. Call it only while no background load is running.
    pub fn reset_progress(&self) {
        self.inner.done.store(0, Ordering::Release);
        self.inner.total.store(0, Ordering::Release);
    }

    /// Starts watching directory mounts for changes. After this, each call
    /// to [`Assets::poll`] reloads the assets whose files changed.
    pub fn enable_hot_reload(&self) -> Result<()> {
        let watcher = Watcher::for_vfs(&self.inner.vfs)?;
        *self.inner.watcher.lock() = Some(watcher);
        Ok(())
    }

    /// Reloads every live asset that is at, or loaded a file at, `path`.
    /// Reloads run on the workers; their results arrive as events.
    pub fn reload(&self, path: &str) {
        let Ok(path) = normalize(path) else { return };
        let targets: Vec<Arc<dyn Reload>> = {
            let mut cache = self.inner.cache.lock();
            cache.retain(|_, e| e.any.strong_count() > 0);
            cache
                .values()
                .filter_map(|e| e.reload.upgrade())
                .filter(|r| r.depends_on(&path))
                .collect()
        };
        for target in targets {
            let inner = self.inner.clone();
            self.inner.pool.spawn(move || {
                let (path, type_name) = (target.path().to_string(), target.type_name());
                let event = match target.reload() {
                    Ok(()) => AssetEvent::Reloaded { path, type_name },
                    Err(error) => {
                        log::warn!("reloading {path} failed: {error}");
                        AssetEvent::ReloadFailed {
                            path,
                            type_name,
                            error,
                        }
                    }
                };
                let _ = inner.events_tx.send(event);
            });
        }
    }

    /// Collects events for the game thread; call it once per frame.
    ///
    /// With hot reload enabled it first asks the watcher for changed
    /// files and starts reloading the affected assets. It then returns
    /// every event gathered since the last call: finished loads, changed
    /// files and completed reloads.
    pub fn poll(&self) -> Vec<AssetEvent> {
        let changed = self
            .inner
            .watcher
            .lock()
            .as_ref()
            .map(Watcher::poll)
            .unwrap_or_default();
        for path in changed {
            let _ = self
                .inner
                .events_tx
                .send(AssetEvent::Changed { path: path.clone() });
            self.reload(&path);
        }
        self.inner.events_rx.try_iter().collect()
    }
}

fn arc_to_error(e: Arc<Error>) -> Error {
    Arc::try_unwrap(e).unwrap_or_else(|shared| Error::Decode {
        path: String::new(),
        source: shared.to_string().into(),
    })
}
