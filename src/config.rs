//! What the window and loop are when a game starts.

use std::path::PathBuf;

use yowie_core::{Color, IVec2, UVec2};
use yowie_gfx::{ScaleMode, ViewSettings};

use crate::icon::Icon;

/// How the loop advances game time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// `update` runs `hz` times per second of wall time, catching up after
    /// a slow frame up to [`Config::max_updates_per_frame`]; `draw` runs
    /// once per displayed frame. A zero `hz` means 60.
    FixedStep {
        /// Updates per second.
        hz: u32,
    },
    /// The process sleeps in the operating system until something happens:
    /// input, a gamepad change, a [`Context::wake_after`](crate::Context::wake_after)
    /// deadline, a [`Waker`](crate::Waker) or a
    /// [`Context::request_redraw`](crate::Context::request_redraw). Each of
    /// those runs one `update` and one `draw`. The first frame draws
    /// without an update.
    TurnBased,
}

impl Default for LoopMode {
    fn default() -> Self {
        LoopMode::FixedStep { hz: 60 }
    }
}

impl LoopMode {
    /// Seconds per update: the fixed step, or a sixtieth of a second for a
    /// turn-based loop running under a fixed clock.
    pub fn step(&self) -> f64 {
        match self {
            LoopMode::FixedStep { hz } if *hz > 0 => 1.0 / f64::from(*hz),
            _ => 1.0 / 60.0,
        }
    }
}

/// The window and loop as they start. Fields change nothing after
/// [`run`](crate::run) begins; use [`Context`](crate::Context) methods to
/// change the window while the game runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The window title.
    pub title: String,
    /// The window's initial content width in logical points, and the
    /// headless image width in pixels.
    pub width: u32,
    /// The window's initial content height in logical points, and the
    /// headless image height in pixels.
    pub height: u32,
    /// Whether the player can resize the window.
    pub resizable: bool,
    /// Whether presentation waits for the display's refresh.
    pub vsync: bool,
    /// Whether the window starts full screen (borderless on the current
    /// display).
    pub fullscreen: bool,
    /// A fixed view size in view units for a virtual resolution, or `None`
    /// for the window's own size in logical points.
    pub view_size: Option<UVec2>,
    /// How a fixed view fills the window.
    pub scale_mode: ScaleMode,
    /// How game time advances.
    pub loop_mode: LoopMode,
    /// The most updates one frame may run to catch up after a stall; the
    /// rest of the backlog is dropped. Zero means 1.
    pub max_updates_per_frame: u32,
    /// The colour each frame starts with.
    pub clear_color: Color,
    /// Whether updates stop while the window lacks keyboard focus. Frames
    /// still draw.
    pub pause_when_unfocused: bool,
    /// Whether the frame-timing overlay starts shown. F3 toggles it.
    pub debug_overlay: bool,
    /// Whether a close request is left to the game, which sees
    /// [`Event::CloseRequested`](crate::Event::CloseRequested) and calls
    /// [`Context::quit`](crate::Context::quit) when ready. Otherwise the
    /// loop ends on the request.
    pub handle_close: bool,
    /// The window icon. See [`Icon`] for which platforms show it.
    pub icon: Option<Icon>,
    /// The smallest content size the player can resize the window to, in
    /// logical points.
    pub min_size: Option<UVec2>,
    /// The largest content size the player can resize the window to, in
    /// logical points.
    pub max_size: Option<UVec2>,
    /// The window's initial top-left corner in logical points on the
    /// desktop. Wayland compositors place windows themselves and ignore
    /// it.
    pub position: Option<IVec2>,
    /// Whether the window stays above other windows. Wayland has no
    /// protocol for this, so it has no effect there.
    pub always_on_top: bool,
    /// Whether windowed runs open the default audio device. When false, or
    /// when no device opens, [`Context::audio`](crate::Context) is a mixer
    /// that plays to no device.
    #[cfg(feature = "audio")]
    pub audio: bool,
    /// A directory whose files take priority over the default asset
    /// locations (the `assets` directory in the working directory and
    /// beside the executable).
    pub asset_dir: Option<PathBuf>,
    /// Pack files to mount behind the asset directories, in priority order.
    /// `assets.pak` beside the executable and in the working directory is
    /// mounted without being listed.
    pub packs: Vec<PathBuf>,
    /// Whether changed asset files reload live. Defaults to on in debug
    /// builds.
    pub hot_reload: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: "Yowie".into(),
            width: 1280,
            height: 720,
            resizable: true,
            vsync: true,
            fullscreen: false,
            view_size: None,
            scale_mode: ScaleMode::Letterbox,
            loop_mode: LoopMode::default(),
            max_updates_per_frame: 8,
            clear_color: Color::BLACK,
            pause_when_unfocused: false,
            debug_overlay: false,
            handle_close: false,
            icon: None,
            min_size: None,
            max_size: None,
            position: None,
            always_on_top: false,
            #[cfg(feature = "audio")]
            audio: true,
            asset_dir: None,
            packs: Vec::new(),
            hot_reload: cfg!(debug_assertions),
        }
    }
}

impl Config {
    /// The default configuration with a title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// The same configuration with another initial size in logical points.
    pub fn with_size(self, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..self
        }
    }

    /// The same configuration with a fixed view of `width` by `height`
    /// view units, scaled into the window by `mode`.
    pub fn with_view(self, width: u32, height: u32, mode: ScaleMode) -> Self {
        Self {
            view_size: Some(UVec2::new(width, height)),
            scale_mode: mode,
            ..self
        }
    }

    pub(crate) fn view_settings(&self) -> ViewSettings {
        ViewSettings {
            fixed: self.view_size,
            scale: self.scale_mode,
        }
    }

    pub(crate) fn size(&self) -> UVec2 {
        UVec2::new(self.width.max(1), self.height.max(1))
    }
}
