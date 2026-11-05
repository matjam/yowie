//! Typed handles to assets that load in the background.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Condvar, Mutex};

use crate::error::Error;
use crate::loader::{LoadContext, Loader, decode_error};
use crate::vfs::Vfs;

/// Where a [`Handle`]'s asset is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// The first load has not finished.
    Loading,
    /// The asset is available. A later failed reload keeps this state and
    /// the previous value.
    Ready,
    /// The first load failed; [`Handle::error`] says why.
    Failed,
}

struct Inner<T> {
    state: LoadState,
    value: Option<Arc<T>>,
    error: Option<Arc<Error>>,
}

/// Shared state behind a [`Handle`].
pub(crate) struct Slot<T: Send + Sync + 'static> {
    path: String,
    vfs: Vfs,
    loader: Option<Arc<dyn Loader<Asset = T>>>,
    inner: Mutex<Inner<T>>,
    done: Condvar,
    generation: AtomicU64,
    deps: Mutex<Vec<String>>,
    run_lock: Mutex<()>,
}

impl<T: Send + Sync + 'static> Slot<T> {
    pub(crate) fn new(path: String, vfs: Vfs, loader: Arc<dyn Loader<Asset = T>>) -> Arc<Self> {
        Arc::new(Self::build(
            path,
            vfs,
            Some(loader),
            LoadState::Loading,
            None,
        ))
    }

    pub(crate) fn failed(path: String, vfs: Vfs, error: Error) -> Arc<Self> {
        Arc::new(Self::build(
            path,
            vfs,
            None,
            LoadState::Failed,
            Some(Arc::new(error)),
        ))
    }

    fn build(
        path: String,
        vfs: Vfs,
        loader: Option<Arc<dyn Loader<Asset = T>>>,
        state: LoadState,
        error: Option<Arc<Error>>,
    ) -> Self {
        Self {
            path,
            vfs,
            loader,
            inner: Mutex::new(Inner {
                state,
                value: None,
                error,
            }),
            done: Condvar::new(),
            generation: AtomicU64::new(0),
            deps: Mutex::new(Vec::new()),
            run_lock: Mutex::new(()),
        }
    }

    /// Reads the file and runs the loader, publishing the result. Runs
    /// are serialised, so a reload never overtakes an earlier load.
    pub(crate) fn run(&self) -> Result<(), Arc<Error>> {
        let _serial = self.run_lock.lock();
        let result = self.load_once();
        let mut inner = self.inner.lock();
        let out = match result {
            Ok((value, deps)) => {
                inner.value = Some(Arc::new(value));
                inner.state = LoadState::Ready;
                inner.error = None;
                *self.deps.lock() = deps;
                self.generation.fetch_add(1, Ordering::Release);
                Ok(())
            }
            Err(e) => {
                let e = Arc::new(e);
                inner.error = Some(e.clone());
                if inner.value.is_none() {
                    inner.state = LoadState::Failed;
                }
                Err(e)
            }
        };
        self.done.notify_all();
        out
    }

    fn load_once(&self) -> Result<(T, Vec<String>), Error> {
        let loader = self.loader.as_ref().ok_or(Error::Closed)?;
        let bytes = self.vfs.read(&self.path)?;
        let ctx = LoadContext::new(&self.vfs, &self.path);
        let value = loader
            .load(&bytes, &self.path, &ctx)
            .map_err(|e| decode_error(&self.path, e))?;
        Ok((value, ctx.into_reads()))
    }
}

/// Type-erased view of a slot, used to reload it by path.
pub(crate) trait Reload: Send + Sync {
    fn path(&self) -> &str;
    fn type_name(&self) -> &'static str;
    fn depends_on(&self, path: &str) -> bool;
    fn reload(&self) -> Result<(), Arc<Error>>;
}

impl<T: Send + Sync + 'static> Reload for Slot<T> {
    fn path(&self) -> &str {
        &self.path
    }
    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
    fn depends_on(&self, path: &str) -> bool {
        self.path == path || self.deps.lock().iter().any(|d| d == path)
    }
    fn reload(&self) -> Result<(), Arc<Error>> {
        self.run()
    }
}

/// A reference to an asset that may still be loading.
///
/// Handles are cheap to clone and share one asset: loading the same path
/// and type twice gives handles to the same slot. The asset is freed when
/// the last handle drops.
///
/// To notice a hot reload, remember [`generation`](Handle::generation) and
/// compare it on the next frame; it increases every time a new value is
/// published.
pub struct Handle<T: Send + Sync + 'static> {
    pub(crate) slot: Arc<Slot<T>>,
}

impl<T: Send + Sync + 'static> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot.clone(),
        }
    }
}

impl<T: Send + Sync + 'static> Handle<T> {
    /// The normalised asset path.
    pub fn path(&self) -> &str {
        &self.slot.path
    }

    /// The load state.
    pub fn state(&self) -> LoadState {
        self.slot.inner.lock().state
    }

    /// Reports whether a value is available.
    pub fn is_ready(&self) -> bool {
        self.state() == LoadState::Ready
    }

    /// Returns the current value, or `None` while loading or after the
    /// first load failed. The returned `Arc` stays valid across reloads.
    pub fn get(&self) -> Option<Arc<T>> {
        self.slot.inner.lock().value.clone()
    }

    /// Returns the error of the most recent load or reload, cleared by the
    /// next success.
    pub fn error(&self) -> Option<Arc<Error>> {
        self.slot.inner.lock().error.clone()
    }

    /// The number of values published so far: 0 before the first load
    /// finishes, 1 after it, and one more for each successful reload.
    pub fn generation(&self) -> u64 {
        self.slot.generation.load(Ordering::Acquire)
    }

    /// Blocks until the first load finishes and returns its value or error.
    pub fn wait(&self) -> Result<Arc<T>, Arc<Error>> {
        let mut inner = self.slot.inner.lock();
        while inner.state == LoadState::Loading {
            self.slot.done.wait(&mut inner);
        }
        match (&inner.value, &inner.error) {
            (Some(v), _) => Ok(v.clone()),
            (None, Some(e)) => Err(e.clone()),
            (None, None) => Err(Arc::new(Error::Closed)),
        }
    }
}

impl<T: Send + Sync + 'static> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("path", &self.slot.path)
            .field("state", &self.state())
            .field("generation", &self.generation())
            .finish()
    }
}
