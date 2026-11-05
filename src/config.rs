//! What the window and loop are when a game starts.

use yowie_core::{Color, UVec2};
use yowie_gfx::{ScaleMode, ViewSettings};

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
    /// Whether the debug console is built, tees the log into its output and
    /// opens on `console_key`. Needs the `console` cargo feature, which is
    /// on by default.
    pub console: bool,
    /// The key that opens the console's drop-down; F4 opens its panels.
    pub console_key: yowie_input::Key,
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
            console: false,
            console_key: yowie_input::Key::Grave,
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
