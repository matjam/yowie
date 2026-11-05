//! What game callbacks receive: graphics, input, time and window controls.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::event_loop::EventLoopProxy;
use winit::window::Window;
use yowie_core::{Rect, UVec2};
use yowie_gfx::Graphics;
use yowie_input::InputState;

use crate::clipboard::Clipboard;
use crate::icon::Cursor;
use crate::overlay::Overlay;
use crate::services::Services;
use crate::time::Time;
use crate::{Config, Error, Result};

/// A message the engine sends its own event loop from any thread.
#[derive(Clone, Copy, Debug)]
pub(crate) enum UserEvent {
    Wake,
}

/// How the cursor is held by the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorGrab {
    /// The cursor moves freely.
    #[default]
    None,
    /// The cursor cannot leave the window.
    Confined,
    /// The cursor stays where it is; read movement from
    /// [`InputState::mouse_motion`](yowie_input::InputState::mouse_motion).
    Locked,
}

/// Wakes a sleeping turn-based loop from any thread, for a network reply or
/// a finished background load. Each wake runs one turn.
#[derive(Clone, Debug)]
pub struct Waker {
    proxy: Option<EventLoopProxy<UserEvent>>,
}

impl Waker {
    /// Ends the loop's wait and runs a turn. Does nothing once the loop
    /// has ended, and in headless runs.
    pub fn wake(&self) {
        if let Some(p) = &self.proxy {
            // A closed loop has no turn left to run, so the error is moot.
            let _ = p.send_event(UserEvent::Wake);
        }
    }
}

/// The window behind a context, or the stand-in for a headless run.
#[derive(Debug)]
pub(crate) enum Host {
    Window {
        window: Arc<Window>,
        proxy: EventLoopProxy<UserEvent>,
    },
    Headless {
        title: String,
        fullscreen: bool,
        size: UVec2,
    },
}

/// What the game asked the loop to do after the current callback.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Requests {
    pub(crate) quit: bool,
    pub(crate) redraw: bool,
    pub(crate) wake_at: Option<Instant>,
    /// The cursor changed and the window has not shown it yet.
    pub(crate) cursor_dirty: bool,
}

/// Everything a game callback works with. `gfx`, `input` and `time` are
/// public fields so they can be borrowed at the same time:
/// `ctx.gfx.draw_texture(&tex, ctx.input.mouse())`.
pub struct Context {
    /// The renderer: drawing, textures, fonts, shaders.
    pub gfx: Graphics,
    /// The keyboard, pointer, text and gamepads.
    pub input: InputState,
    /// Game time.
    pub time: Time,
    /// The immediate-mode interface, drawn in `draw` with
    /// `ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| ...)`.
    #[cfg(feature = "ui")]
    pub ui: yowie_ui::Ui,
    /// The debug console; it does nothing unless
    /// [`Config::console`](crate::Config::console) is set.
    #[cfg(feature = "console")]
    pub console: yowie_console::Console,
    pub(crate) hook: crate::ui_hook::Hook,
    /// The audio mixer. Windowed runs play it on the default output device;
    /// headless runs, runs with [`Config::audio`](crate::Config) off and
    /// machines without a device get a mixer that is rendered each frame
    /// and heard by no one, so voices still start, finish and call back.
    #[cfg(feature = "audio")]
    pub audio: yowie_audio::Mixer,
    /// The assets: directories and packs behind one file system, typed
    /// loaders and background loading. Use the `load_*` methods of the
    /// context for GPU objects that reload when their files change.
    pub assets: yowie_asset::Assets,
    pub(crate) host: Host,
    pub(crate) requests: Requests,
    pub(crate) clipboard: Clipboard,
    pub(crate) overlay: Overlay,
    pub(crate) focused: bool,
    pub(crate) text_input: bool,
    pub(crate) cursor: Cursor,
    pub(crate) always_on_top: bool,
    pub(crate) services: Services,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("gfx", &self.gfx)
            .field("time", &self.time)
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

impl Context {
    pub(crate) fn new(gfx: Graphics, host: Host, config: &Config) -> Result<Self> {
        let clipboard = match host {
            Host::Window { .. } => Clipboard::system(),
            Host::Headless { .. } => Clipboard::memory(),
        };
        let windowed = matches!(host, Host::Window { .. });
        #[cfg(feature = "audio")]
        let (audio, backend) = crate::audio_backend::start(config.audio && windowed)?;
        #[cfg(feature = "audio")]
        let rate = Some(audio.rate());
        #[cfg(not(feature = "audio"))]
        let rate = None;
        let assets = crate::content::build_assets(config, rate)?;
        Ok(Self {
            gfx,
            input: InputState::new(),
            time: Time::default(),
            #[cfg(feature = "ui")]
            ui: yowie_ui::Ui::default(),
            #[cfg(feature = "console")]
            console: yowie_console::Console::disabled(),
            hook: crate::ui_hook::Hook::default(),
            #[cfg(feature = "audio")]
            audio,
            assets,
            host,
            requests: Requests::default(),
            clipboard,
            overlay: Overlay::new(config.debug_overlay),
            focused: true,
            text_input: false,
            cursor: Cursor::default(),
            always_on_top: config.always_on_top,
            services: Services::new(
                #[cfg(feature = "audio")]
                backend,
            ),
        })
    }

    /// Ends the loop after the current callback.
    pub fn quit(&mut self) {
        self.requests.quit = true;
    }

    /// Asks for another frame without waiting for input. In a turn-based
    /// loop the frame runs a turn; call it each frame while an animation
    /// that spans turns plays.
    pub fn request_redraw(&mut self) {
        self.requests.redraw = true;
    }

    /// Wakes a turn-based loop after `delay` to run a turn, for timers. The
    /// earliest pending wake wins.
    pub fn wake_after(&mut self, delay: Duration) {
        let at = Instant::now() + delay;
        self.requests.wake_at = Some(self.requests.wake_at.map_or(at, |w| w.min(at)));
    }

    /// A handle that wakes the loop from any thread.
    pub fn waker(&self) -> Waker {
        Waker {
            proxy: match &self.host {
                Host::Window { proxy, .. } => Some(proxy.clone()),
                Host::Headless { .. } => None,
            },
        }
    }

    /// Whether the run has no window.
    pub fn headless(&self) -> bool {
        matches!(self.host, Host::Headless { .. })
    }

    /// Whether the window has keyboard focus. Headless runs are focused.
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// The window title.
    pub fn title(&self) -> String {
        match &self.host {
            Host::Window { window, .. } => window.title(),
            Host::Headless { title, .. } => title.clone(),
        }
    }

    /// Sets the window title.
    pub fn set_title(&mut self, title: &str) {
        match &mut self.host {
            Host::Window { window, .. } => window.set_title(title),
            Host::Headless { title: t, .. } => *t = title.to_owned(),
        }
    }

    /// The window's content size in logical points (pixels in a headless
    /// run).
    pub fn window_size(&self) -> UVec2 {
        match &self.host {
            Host::Window { window, .. } => {
                let s = window.inner_size().to_logical::<f64>(window.scale_factor());
                UVec2::new(s.width.round() as u32, s.height.round() as u32)
            }
            Host::Headless { size, .. } => *size,
        }
    }

    /// Asks for a new content size in logical points. The desktop may
    /// adjust it; the new size arrives as an [`Event::Resized`](crate::Event::Resized).
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        if let Host::Window { window, .. } = &self.host {
            let _immediate = window.request_inner_size(winit::dpi::LogicalSize::new(width, height));
        }
    }

    /// Pixels per logical point of the window's display.
    pub fn scale_factor(&self) -> f32 {
        match &self.host {
            Host::Window { window, .. } => window.scale_factor() as f32,
            Host::Headless { .. } => 1.0,
        }
    }

    /// Whether the window is full screen, read back from the window system
    /// so it follows the player's own full-screen button.
    pub fn fullscreen(&self) -> bool {
        match &self.host {
            Host::Window { window, .. } => window.fullscreen().is_some(),
            Host::Headless { fullscreen, .. } => *fullscreen,
        }
    }

    /// Makes the window borderless full screen on its current display, or
    /// returns it to a normal window.
    pub fn set_fullscreen(&mut self, on: bool) {
        match &mut self.host {
            Host::Window { window, .. } => {
                window.set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)))
            }
            Host::Headless { fullscreen, .. } => *fullscreen = on,
        }
    }

    /// Shows or hides the cursor over the window.
    pub fn set_cursor_visible(&mut self, visible: bool) {
        if let Host::Window { window, .. } = &self.host {
            window.set_cursor_visible(visible);
        }
    }

    /// Confines or locks the cursor. Platforms that lack the requested mode
    /// fall back to the other one; an error means neither is available.
    pub fn set_cursor_grab(&mut self, grab: CursorGrab) -> Result<()> {
        let Host::Window { window, .. } = &self.host else {
            return Ok(());
        };
        use winit::window::CursorGrabMode as M;
        let (first, second) = match grab {
            CursorGrab::None => (M::None, M::None),
            CursorGrab::Confined => (M::Confined, M::Locked),
            CursorGrab::Locked => (M::Locked, M::Confined),
        };
        window
            .set_cursor_grab(first)
            .or_else(|_| window.set_cursor_grab(second))
            .map_err(|e| Error::Window(e.to_string()))
    }

    /// The clipboard's text, if it holds any. Headless runs keep a private
    /// clipboard in memory.
    pub fn clipboard_text(&mut self) -> Option<String> {
        self.clipboard.get()
    }

    /// Puts text on the clipboard.
    pub fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        self.clipboard.set(text).map_err(Error::Window)
    }

    /// Turns text input on or off. While on, the platform's input method
    /// can compose text (shown through
    /// [`InputState::composition`](yowie_input::InputState::composition));
    /// while off, every key press arrives as a key event, which games want
    /// during play. Typed text arrives either way.
    pub fn set_text_input(&mut self, on: bool) {
        self.text_input = on;
        if let Host::Window { window, .. } = &self.host {
            window.set_ime_allowed(on);
        }
    }

    /// Tells the input method where the text field is, in view units, so
    /// its candidate window appears beside it.
    pub fn set_text_input_area(&mut self, area: Rect) {
        if let Host::Window { window, .. } = &self.host {
            let m = self.gfx.view_mapping();
            let px = m.rect_to_pixels(area);
            window.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(px.x, px.y),
                winit::dpi::PhysicalSize::new(px.w, px.h),
            );
        }
    }

    /// Shows or hides the frame-timing overlay (F3 toggles it too).
    pub fn set_debug_overlay(&mut self, on: bool) {
        self.overlay.enabled = on;
    }

    /// Whether the frame-timing overlay is shown.
    pub fn debug_overlay(&self) -> bool {
        self.overlay.enabled
    }
}
