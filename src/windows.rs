//! Extra windows: their settings, the registry the context keeps, and the
//! requests that open and close them.
//!
//! A game opens a window with [`Context::open_window`] and draws into it
//! with `ctx.gfx.draw_in(id, |gfx| ...)`. The window system creates the
//! window right after the callback returns, so the first frame it can show
//! is the next one; until then `draw_in` returns `None`. Headless runs
//! create an offscreen image in its place at once.

use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;
use yowie_core::{Color, IVec2, UVec2, Vec2};
use yowie_gfx::{GraphicsSettings, ScaleMode, ViewSettings};

use crate::{Context, Event, Game, Result};
pub use yowie_gfx::WindowId;

/// What an extra window is when it opens.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowConfig {
    /// The title bar text.
    pub title: String,
    /// Content width in logical points.
    pub width: u32,
    /// Content height in logical points.
    pub height: u32,
    /// Whether the player can resize the window.
    pub resizable: bool,
    /// Whether presentation waits for the display's refresh.
    pub vsync: bool,
    /// A fixed 2D view size, or `None` for the window's size in points.
    pub view_size: Option<UVec2>,
    /// How a fixed view fills the window.
    pub scale_mode: ScaleMode,
    /// The colour each frame starts with, sRGB.
    pub clear_color: Color,
    /// The top-left corner on the desktop in logical points. Wayland
    /// compositors place windows themselves and ignore it.
    pub position: Option<IVec2>,
    /// Whether the window stays above others. Wayland has no protocol for
    /// this and ignores it.
    pub always_on_top: bool,
    /// The window this one belongs to. Closing the parent closes this
    /// window too.
    pub parent: Option<WindowId>,
    /// Whether the game decides what a close request does. When false the
    /// engine closes the window after delivering
    /// [`Event::CloseRequested`](crate::Event::CloseRequested); when true
    /// the game calls [`Context::close_window`] itself.
    pub handle_close: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Yowie".into(),
            width: 480,
            height: 320,
            resizable: true,
            vsync: true,
            view_size: None,
            scale_mode: ScaleMode::Letterbox,
            clear_color: Color::BLACK,
            position: None,
            always_on_top: false,
            parent: None,
            handle_close: false,
        }
    }
}

impl WindowConfig {
    /// A configuration with `title` and the defaults for the rest.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// The same configuration with a content size in logical points.
    pub fn with_size(self, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..self
        }
    }

    /// The same configuration with a parent window.
    pub fn with_parent(self, parent: WindowId) -> Self {
        Self {
            parent: Some(parent),
            ..self
        }
    }

    pub(crate) fn size(&self) -> UVec2 {
        UVec2::new(self.width.max(1), self.height.max(1))
    }

    fn settings(&self) -> GraphicsSettings {
        GraphicsSettings {
            vsync: self.vsync,
            view: ViewSettings {
                fixed: self.view_size,
                scale: self.scale_mode,
            },
            clear_color: self.clear_color,
        }
    }

    fn attributes(&self) -> winit::window::WindowAttributes {
        let size = self.size();
        let mut attrs = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(size.x, size.y))
            .with_resizable(self.resizable);
        if let Some(p) = self.position {
            attrs = attrs.with_position(winit::dpi::LogicalPosition::new(p.x, p.y));
        }
        if self.always_on_top {
            attrs = attrs.with_window_level(winit::window::WindowLevel::AlwaysOnTop);
        }
        attrs
    }
}

/// What the context knows about one window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    /// The window's id.
    pub id: WindowId,
    /// The title bar text.
    pub title: String,
    /// Content size in logical points (pixels in a headless run).
    pub size: UVec2,
    /// Pixels per logical point of the window's display.
    pub scale_factor: f32,
    /// Whether the window has keyboard focus.
    pub focused: bool,
    /// The window it belongs to, if any.
    pub parent: Option<WindowId>,
}

/// One extra window.
#[derive(Debug)]
pub(crate) struct Extra {
    pub(crate) id: WindowId,
    pub(crate) parent: Option<WindowId>,
    pub(crate) title: String,
    pub(crate) size: UVec2,
    pub(crate) scale_factor: f32,
    pub(crate) focused: bool,
    /// The pointer in view units, `None` while it is outside the window.
    pub(crate) pointer: Option<Vec2>,
    pub(crate) handle_close: bool,
    pub(crate) window: Option<Arc<Window>>,
    /// The settings of a window the window system has not created yet.
    pub(crate) pending: Option<WindowConfig>,
}

/// The extra windows of a context and the requests against them.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub(crate) extras: Vec<Extra>,
    pub(crate) closing: Vec<WindowId>,
    pub(crate) main_focused: bool,
}

impl Registry {
    pub(crate) fn get(&self, id: WindowId) -> Option<&Extra> {
        self.extras.iter().find(|e| e.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: WindowId) -> Option<&mut Extra> {
        self.extras.iter_mut().find(|e| e.id == id)
    }

    /// The extra whose window system window is `id`.
    pub(crate) fn by_native(&self, id: winit::window::WindowId) -> Option<WindowId> {
        self.extras
            .iter()
            .find(|e| e.window.as_ref().is_some_and(|w| w.id() == id))
            .map(|e| e.id)
    }

    /// The ids of the extra windows, in the order they were opened.
    pub(crate) fn ids(&self) -> Vec<WindowId> {
        self.extras.iter().map(|e| e.id).collect()
    }

    pub(crate) fn any_focused(&self) -> bool {
        self.main_focused || self.extras.iter().any(|e| e.focused)
    }

    /// Adds `id` and every window below it to the closing list, children
    /// first.
    fn request_close(&mut self, id: WindowId) {
        let children: Vec<WindowId> = self
            .extras
            .iter()
            .filter(|e| e.parent == Some(id))
            .map(|e| e.id)
            .collect();
        for c in children {
            self.request_close(c);
        }
        if self.get(id).is_some() && !self.closing.contains(&id) {
            self.closing.push(id);
        }
    }
}

impl Context {
    /// Opens another window and returns its id at once. The window system
    /// creates the window after the current callback; headless runs get an
    /// offscreen image of the same size.
    ///
    /// Draw into it with `ctx.gfx.draw_in(id, |gfx| ...)`. Keyboard input
    /// from any window arrives in [`Context::input`]; the events of each
    /// window arrive tagged in [`Game::window_event`](crate::Game::window_event).
    pub fn open_window(&mut self, config: WindowConfig) -> WindowId {
        let id = self.gfx.reserve_window_id();
        let parent = config
            .parent
            .filter(|p| *p == WindowId::MAIN || self.windows.get(*p).is_some());
        let mut extra = Extra {
            id,
            parent,
            title: config.title.clone(),
            size: config.size(),
            scale_factor: 1.0,
            focused: false,
            pointer: None,
            handle_close: config.handle_close,
            window: None,
            pending: None,
        };
        if self.headless() {
            self.gfx
                .add_offscreen_output(id, config.size(), 1.0, config.settings());
        } else {
            extra.pending = Some(config);
        }
        self.windows.extras.push(extra);
        id
    }

    /// Closes a window and every window opened with it as parent. Closing
    /// [`WindowId::MAIN`] ends the run. The window disappears after the
    /// current callback, and the game then receives
    /// [`Event::Closed`](crate::Event::Closed) for it.
    pub fn close_window(&mut self, id: WindowId) {
        if id == WindowId::MAIN {
            self.quit();
        } else {
            self.windows.request_close(id);
        }
    }

    /// Every open window, the main one first, then the others in the order
    /// they were opened.
    pub fn windows(&self) -> Vec<WindowId> {
        std::iter::once(WindowId::MAIN)
            .chain(self.windows.extras.iter().map(|e| e.id))
            .collect()
    }

    /// What is known about window `id`, or `None` if it does not exist.
    pub fn window_info(&self, id: WindowId) -> Option<WindowInfo> {
        if id == WindowId::MAIN {
            return Some(WindowInfo {
                id,
                title: self.title(),
                size: self.window_size(),
                scale_factor: self.scale_factor(),
                focused: self.windows.main_focused || self.headless(),
                parent: None,
            });
        }
        let e = self.windows.get(id)?;
        Some(WindowInfo {
            id,
            title: e.title.clone(),
            size: e.size,
            scale_factor: e.scale_factor,
            focused: e.focused,
            parent: e.parent,
        })
    }

    /// Sets the title of window `id`.
    pub fn set_window_title_of(&mut self, id: WindowId, title: &str) {
        if id == WindowId::MAIN {
            self.set_title(title);
        } else if let Some(e) = self.windows.get_mut(id) {
            e.title = title.to_owned();
            if let Some(w) = &e.window {
                w.set_title(title);
            }
        }
    }

    /// Asks for a new content size, in logical points, for window `id`.
    /// The desktop may adjust it; the result arrives as
    /// [`Event::Resized`](crate::Event::Resized) tagged with the window.
    pub fn set_window_size_of(&mut self, id: WindowId, width: u32, height: u32) {
        if id == WindowId::MAIN {
            self.set_window_size(width, height);
        } else if let Some(e) = self.windows.get_mut(id) {
            if let Some(w) = &e.window {
                let _immediate =
                    w.request_inner_size(winit::dpi::LogicalSize::new(width.max(1), height.max(1)));
            } else if let Some(p) = &mut e.pending {
                p.width = width;
                p.height = height;
                e.size = UVec2::new(width.max(1), height.max(1));
            } else {
                e.size = UVec2::new(width.max(1), height.max(1));
                self.gfx.resize_window_output(id, e.size, 1.0);
            }
        }
    }

    /// The pointer in window `id`, in that window's view units, or `None`
    /// while it is outside. The main window's pointer is
    /// [`InputState::mouse`](yowie_input::InputState::mouse).
    pub fn window_pointer(&self, id: WindowId) -> Option<Vec2> {
        if id == WindowId::MAIN {
            return self.input.mouse_inside().then(|| self.input.mouse());
        }
        self.windows.get(id)?.pointer
    }

    /// Creates the windows the game asked for. A window that cannot be
    /// created is logged and closed.
    pub(crate) fn create_windows(&mut self, el: &ActiveEventLoop) {
        for i in 0..self.windows.extras.len() {
            let Some(config) = self.windows.extras[i].pending.take() else {
                continue;
            };
            let id = self.windows.extras[i].id;
            let made = el
                .create_window(config.attributes())
                .map_err(|e| e.to_string())
                .and_then(|w| {
                    let w = Arc::new(w);
                    let px = w.inner_size();
                    let scale = w.scale_factor() as f32;
                    self.gfx
                        .add_window_output(
                            id,
                            w.clone(),
                            UVec2::new(px.width.max(1), px.height.max(1)),
                            scale,
                            config.settings(),
                        )
                        .map(|_| (w, scale))
                        .map_err(|e| e.to_string())
                });
            match made {
                Ok((w, scale)) => {
                    let e = &mut self.windows.extras[i];
                    e.scale_factor = scale;
                    e.focused = w.has_focus();
                    e.window = Some(w);
                }
                Err(msg) => {
                    log::warn!("yowie: cannot open window {:?}: {msg}", config.title);
                    self.windows.closing.push(id);
                }
            }
        }
    }
}

/// Removes the windows closed since the last call and tells the game.
pub(crate) fn process_closes<G: Game>(ctx: &mut Context, game: &mut G) -> Result<()> {
    while !ctx.windows.closing.is_empty() {
        let ids = std::mem::take(&mut ctx.windows.closing);
        for id in ids {
            // The surface goes before the window it draws into.
            ctx.gfx.remove_window_output(id);
            let before = ctx.windows.extras.len();
            ctx.windows.extras.retain(|e| e.id != id);
            if ctx.windows.extras.len() == before {
                continue;
            }
            ctx.focused = ctx.windows.any_focused() || ctx.headless();
            game.window_event(ctx, id, &Event::Closed)
                .map_err(crate::Error::Game)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Host;
    use crate::{Config, GameResult};
    use yowie_gfx::Graphics;

    struct Recorder(Vec<(WindowId, Event)>);

    impl Game for Recorder {
        fn init(_: &mut Context) -> GameResult<Self> {
            Ok(Recorder(Vec::new()))
        }
        fn draw(&mut self, _: &mut Context) -> GameResult {
            Ok(())
        }
        fn window_event(&mut self, _: &mut Context, w: WindowId, e: &Event) -> GameResult {
            self.0.push((w, e.clone()));
            Ok(())
        }
    }

    fn context() -> Option<Context> {
        if !yowie_gfx::adapter_available() {
            println!("skipping: no GPU adapter");
            return None;
        }
        let size = UVec2::new(16, 16);
        let gfx = Graphics::new_headless(size, GraphicsSettings::default()).ok()?;
        let host = Host::Headless {
            title: "test".into(),
            fullscreen: false,
            size,
        };
        Context::new(gfx, host, &Config::default()).ok()
    }

    #[test]
    fn a_second_window_opens_draws_and_closes() {
        let Some(mut ctx) = context() else { return };
        let mut game = Recorder(Vec::new());
        let a = ctx.open_window(WindowConfig::new("A").with_size(40, 20));
        let b = ctx.open_window(WindowConfig::new("B").with_parent(a));
        assert_eq!(ctx.windows(), [WindowId::MAIN, a, b]);
        assert_eq!(ctx.window_info(a).map(|i| i.size), Some(UVec2::new(40, 20)));
        assert_eq!(ctx.window_info(b).and_then(|i| i.parent), Some(a));
        let red = Color::rgb8(255, 0, 0);
        let drawn = ctx.gfx.draw_in(a, |g| {
            g.fill_rect(yowie_core::Rect::new(0.0, 0.0, 40.0, 20.0), red);
        });
        assert!(drawn.is_some());
        ctx.gfx.end_window_frame(a).expect("frame");
        let image = ctx.gfx.read_window_image(a).expect("image");
        assert_eq!(image.get_pixel(5, 5).0, [255, 0, 0, 255]);

        // Closing the parent closes the child first.
        ctx.close_window(a);
        process_closes(&mut ctx, &mut game).expect("closes");
        assert_eq!(ctx.windows(), [WindowId::MAIN]);
        let closed: Vec<WindowId> = game.0.iter().map(|(w, _)| *w).collect();
        assert_eq!(closed, [b, a]);
        assert!(ctx.gfx.draw_in(a, |_| ()).is_none());
        assert!(ctx.window_info(a).is_none());
    }

    #[test]
    fn closing_the_main_window_quits() {
        let Some(mut ctx) = context() else { return };
        ctx.close_window(WindowId::MAIN);
        assert!(ctx.requests.quit);
    }
}
