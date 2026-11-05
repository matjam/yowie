//! The [`Loader`] trait and the context a loader runs in.

use parking_lot::Mutex;

use crate::error::{BoxError, Error, Result};
use crate::path;
use crate::vfs::Vfs;

/// Turns the bytes of a file into a typed asset.
///
/// Register one loader per asset type with
/// [`Assets::register`](crate::Assets::register). `load` runs on a worker
/// thread for background loads, so it must not touch thread-bound resources
/// such as a GPU device; return CPU data and let the renderer upload it on
/// its own thread after the handle is ready.
///
/// ```
/// use yowie_asset::{BoxError, LoadContext, Loader};
///
/// struct Text;
/// impl Loader for Text {
///     type Asset = String;
///     fn load(&self, bytes: &[u8], _path: &str, _ctx: &LoadContext<'_>) -> Result<String, BoxError> {
///         Ok(String::from_utf8(bytes.to_vec())?)
///     }
/// }
/// ```
pub trait Loader: Send + Sync + 'static {
    /// The type this loader produces.
    type Asset: Send + Sync + 'static;

    /// Builds an asset from the file at `path` whose contents are `bytes`.
    ///
    /// `path` is the normalised asset path. To read files the asset refers
    /// to, such as a map's tilesets, use `ctx`; files read through it are
    /// recorded so that changing one reloads this asset.
    fn load(
        &self,
        bytes: &[u8],
        path: &str,
        ctx: &LoadContext<'_>,
    ) -> std::result::Result<Self::Asset, BoxError>;
}

/// Access to the file system for a running [`Loader`].
pub struct LoadContext<'a> {
    vfs: &'a Vfs,
    dir: &'a str,
    reads: Mutex<Vec<String>>,
}

impl<'a> LoadContext<'a> {
    pub(crate) fn new(vfs: &'a Vfs, asset_path: &'a str) -> Self {
        Self {
            vfs,
            dir: path::dir_of(asset_path),
            reads: Mutex::new(Vec::new()),
        }
    }

    /// The file system the asset is being loaded from.
    pub fn vfs(&self) -> &Vfs {
        self.vfs
    }

    /// Reads a file by asset path and records it as a dependency.
    pub fn read(&self, asset_path: &str) -> Result<Vec<u8>> {
        let norm = path::normalize(asset_path)?;
        self.reads.lock().push(norm.clone());
        self.vfs.read(&norm)
    }

    /// Reads a file named relative to the asset's own directory, such as
    /// `../tiles/ground.png` from `maps/level1.tmx`, and records it as a
    /// dependency.
    pub fn read_relative(&self, rel: &str) -> Result<Vec<u8>> {
        self.read(&self.resolve(rel)?)
    }

    /// Returns the asset path that `rel` names relative to the asset's
    /// directory.
    pub fn resolve(&self, rel: &str) -> Result<String> {
        path::join(self.dir, rel)
    }

    pub(crate) fn into_reads(self) -> Vec<String> {
        self.reads.into_inner()
    }
}

pub(crate) fn decode_error(path: &str, source: BoxError) -> Error {
    Error::Decode {
        path: path.to_string(),
        source,
    }
}
