//! Builder methods for the window and content settings of [`Config`].

use std::path::PathBuf;

use yowie_core::{IVec2, UVec2};

use crate::Config;
use crate::icon::Icon;

impl Config {
    /// The same configuration with a window icon.
    pub fn with_icon(self, icon: Icon) -> Self {
        Self {
            icon: Some(icon),
            ..self
        }
    }

    /// The same configuration with a smallest window content size in
    /// logical points.
    pub fn with_min_size(self, width: u32, height: u32) -> Self {
        Self {
            min_size: Some(UVec2::new(width, height)),
            ..self
        }
    }

    /// The same configuration with a largest window content size in
    /// logical points.
    pub fn with_max_size(self, width: u32, height: u32) -> Self {
        Self {
            max_size: Some(UVec2::new(width, height)),
            ..self
        }
    }

    /// The same configuration with an initial window position in logical
    /// points.
    pub fn with_position(self, x: i32, y: i32) -> Self {
        Self {
            position: Some(IVec2::new(x, y)),
            ..self
        }
    }

    /// The same configuration with the window kept above others.
    pub fn with_always_on_top(self, on: bool) -> Self {
        Self {
            always_on_top: on,
            ..self
        }
    }

    /// The same configuration with audio output on or off.
    #[cfg(feature = "audio")]
    pub fn with_audio(self, on: bool) -> Self {
        Self { audio: on, ..self }
    }

    /// The same configuration with an asset directory that takes priority
    /// over the default locations.
    pub fn with_asset_dir(self, dir: impl Into<PathBuf>) -> Self {
        Self {
            asset_dir: Some(dir.into()),
            ..self
        }
    }

    /// The same configuration with one more pack file mounted.
    pub fn with_pack(mut self, pack: impl Into<PathBuf>) -> Self {
        self.packs.push(pack.into());
        self
    }
}
