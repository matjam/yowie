//! The engine's asset file system and the loaders it registers.
//!
//! Loaders run on worker threads, so they produce CPU data only: decoded
//! images, font bytes, shader source, decoded sounds and maps. The
//! `load_*` methods of [`Context`](crate::Context) turn that data into GPU
//! objects on the game thread and keep them current when files change.

use std::path::{Path, PathBuf};

use yowie_asset::{Assets, BoxError, LoadContext, Loader, Vfs};
use yowie_gfx::ImageData;

use crate::{Config, Error, Result};

/// A font file's bytes, the asset type of [`FontLoader`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontData(pub Vec<u8>);

/// WGSL source of a 2D fragment shader, the asset type of [`ShaderLoader`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderSource(pub String);

/// Decodes PNG, JPEG, Radiance HDR and KTX2 files into [`ImageData`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageLoader;

impl Loader for ImageLoader {
    type Asset = ImageData;

    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        _ctx: &LoadContext<'_>,
    ) -> std::result::Result<ImageData, BoxError> {
        Ok(ImageData::decode(bytes)?)
    }
}

/// Keeps a font file's bytes for [`Context::load_font`](crate::Context::load_font).
#[derive(Clone, Copy, Debug, Default)]
pub struct FontLoader;

impl Loader for FontLoader {
    type Asset = FontData;

    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        _ctx: &LoadContext<'_>,
    ) -> std::result::Result<FontData, BoxError> {
        Ok(FontData(bytes.to_vec()))
    }
}

/// Reads a WGSL file as UTF-8 text.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderLoader;

impl Loader for ShaderLoader {
    type Asset = ShaderSource;

    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        _ctx: &LoadContext<'_>,
    ) -> std::result::Result<ShaderSource, BoxError> {
        Ok(ShaderSource(String::from_utf8(bytes.to_vec())?))
    }
}

/// Decodes WAV, Ogg Vorbis, MP3 and FLAC files into sounds at the mixer's
/// sample rate.
#[cfg(feature = "audio")]
#[derive(Clone, Copy, Debug)]
pub struct SoundLoader {
    /// The mixer's sample rate, [`Mixer::rate`](yowie_audio::Mixer::rate).
    pub rate: u32,
}

#[cfg(feature = "audio")]
impl Loader for SoundLoader {
    type Asset = yowie_audio::Sound;

    fn load(
        &self,
        bytes: &[u8],
        _path: &str,
        _ctx: &LoadContext<'_>,
    ) -> std::result::Result<yowie_audio::Sound, BoxError> {
        let pcm = yowie_audio::decode::decode(bytes.to_vec())?;
        Ok(yowie_audio::Sound::new(self.rate, &pcm)?)
    }
}

/// The mounts a configuration asks for, highest priority first: the
/// configured directory, `assets` in the working directory, `assets`
/// beside the executable, the configured packs, then `assets.pak` in the
/// working directory and beside the executable. Missing paths are skipped.
pub(crate) fn mounts(config: &Config, cwd: Option<&Path>, exe_dir: Option<&Path>) -> Mounts {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut packs: Vec<PathBuf> = Vec::new();
    let push = |list: &mut Vec<PathBuf>, p: PathBuf, want_dir: bool| {
        let ok = if want_dir { p.is_dir() } else { p.is_file() };
        let canon = p.canonicalize().unwrap_or(p);
        if ok && !list.contains(&canon) {
            list.push(canon);
        }
    };
    if let Some(d) = &config.asset_dir {
        push(&mut dirs, d.clone(), true);
    }
    for base in [cwd, exe_dir].into_iter().flatten() {
        push(&mut dirs, base.join("assets"), true);
    }
    for p in &config.packs {
        push(&mut packs, p.clone(), false);
    }
    for base in [cwd, exe_dir].into_iter().flatten() {
        push(&mut packs, base.join("assets.pak"), false);
    }
    Mounts { dirs, packs }
}

/// Directories and pack files to mount.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Mounts {
    pub(crate) dirs: Vec<PathBuf>,
    pub(crate) packs: Vec<PathBuf>,
}

/// Builds the asset manager for a run: the default mounts, the loaders for
/// every asset type, and hot reload when the configuration asks for it.
pub(crate) fn build_assets(config: &Config, sound_rate: Option<u32>) -> Result<Assets> {
    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let m = mounts(config, cwd.as_deref(), exe_dir.as_deref());
    let mut vfs = Vfs::new();
    for d in &m.dirs {
        vfs.mount_dir(d).map_err(Error::Asset)?;
    }
    for p in &m.packs {
        if let Err(e) = vfs.mount_pack(p) {
            log::warn!("pack {} not mounted: {e}", p.display());
        }
    }
    let assets = Assets::new(vfs, 0).map_err(Error::Asset)?;
    assets.register(ImageLoader);
    assets.register(FontLoader);
    assets.register(ShaderLoader);
    #[cfg(feature = "audio")]
    if let Some(rate) = sound_rate {
        assets.register(SoundLoader { rate });
    }
    #[cfg(not(feature = "audio"))]
    let _ = sound_rate;
    #[cfg(feature = "tiled")]
    assets.register(yowie_tiled::MapLoader);
    if config.hot_reload
        && !m.dirs.is_empty()
        && let Err(e) = assets.enable_hot_reload()
    {
        log::warn!("asset hot reload is off: {e}");
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_prefer_the_configured_directory_and_skip_missing_paths() {
        let tmp = std::env::temp_dir().join(format!("yowie-mounts-{}", std::process::id()));
        let (cwd, exe) = (tmp.join("cwd"), tmp.join("exe"));
        let custom = tmp.join("custom");
        for d in [cwd.join("assets"), exe.join("assets"), custom.clone()] {
            std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("mkdir: {e}"));
        }
        std::fs::write(exe.join("assets.pak"), b"x").unwrap_or_else(|e| panic!("write: {e}"));
        let config = Config::default()
            .with_asset_dir(&custom)
            .with_pack(tmp.join("missing.pak"));
        let m = mounts(&config, Some(&cwd), Some(&exe));
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|e| panic!("canon: {e}"));
        assert_eq!(
            m.dirs,
            [
                canon(&custom),
                canon(&cwd.join("assets")),
                canon(&exe.join("assets"))
            ]
        );
        assert_eq!(m.packs, [canon(&exe.join("assets.pak"))]);
        let none = mounts(&Config::default(), None, None);
        assert!(none.dirs.is_empty() && none.packs.is_empty());
        let _ = std::fs::remove_dir_all(tmp);
    }
}
