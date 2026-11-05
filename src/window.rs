//! One window's state: the winit window, its context and game, and the
//! translation of its events. The app holds one today; more windows would
//! each get their own.

use std::sync::Arc;

use winit::event::{ElementState, Ime, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::PhysicalKey;
use winit::window::{Fullscreen, Window};
use yowie_core::{UVec2, Vec2, vec2};
use yowie_gfx::{Graphics, GraphicsSettings};
use yowie_input::InputEvent;

use crate::context::{Host, UserEvent};
use crate::icon::Cursor;
use crate::stepper::{Stepper, Tick};
use crate::{Config, Context, Error, Event, Game, Result, WindowId, keymap};

/// Logical points of pixel scrolling that count as one line.
const POINTS_PER_LINE: f64 = 20.0;

/// A window and the game drawing into it.
pub(crate) struct WindowState<G: Game> {
    pub(crate) window: Arc<Window>,
    pub(crate) ctx: Context,
    pub(crate) game: G,
    stepper: Stepper,
    pub(crate) occluded: bool,
    /// Whether something happened that runs a turn in a turn-based loop.
    pub(crate) turn_pending: bool,
    pub(crate) handle_close: bool,
}

fn window_attributes(config: &Config) -> winit::window::WindowAttributes {
    let mut attrs = Window::default_attributes()
        .with_title(config.title.clone())
        .with_inner_size(winit::dpi::LogicalSize::new(
            config.width.max(1),
            config.height.max(1),
        ))
        .with_resizable(config.resizable)
        .with_fullscreen(config.fullscreen.then_some(Fullscreen::Borderless(None)));
    if let Some(icon) = &config.icon {
        match icon.to_winit() {
            Ok(icon) => attrs = attrs.with_window_icon(Some(icon)),
            Err(e) => log::warn!("window icon not set: {e}"),
        }
    }
    if let Some(s) = config.min_size {
        attrs = attrs.with_min_inner_size(winit::dpi::LogicalSize::new(s.x.max(1), s.y.max(1)));
    }
    if let Some(s) = config.max_size {
        attrs = attrs.with_max_inner_size(winit::dpi::LogicalSize::new(s.x.max(1), s.y.max(1)));
    }
    if let Some(p) = config.position {
        attrs = attrs.with_position(winit::dpi::LogicalPosition::new(p.x, p.y));
    }
    if config.always_on_top {
        attrs = attrs.with_window_level(winit::window::WindowLevel::AlwaysOnTop);
    }
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    let attrs = {
        // Wayland compositors match the app id to a desktop entry; X11 uses
        // the same name for WM_CLASS.
        let id: String = config
            .title
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, id.clone(), id)
    };
    attrs
}

impl<G: Game> WindowState<G> {
    /// Opens the window, creates its renderer and context, and builds the
    /// game.
    pub(crate) fn create(
        el: &ActiveEventLoop,
        config: &Config,
        fixed_clock: bool,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self> {
        let window = Arc::new(
            el.create_window(window_attributes(config))
                .map_err(|e| Error::CreateWindow(e.to_string()))?,
        );
        let size = window.inner_size();
        let gfx = Graphics::new_windowed(
            window.clone(),
            UVec2::new(size.width.max(1), size.height.max(1)),
            window.scale_factor() as f32,
            GraphicsSettings {
                vsync: config.vsync,
                view: config.view_settings(),
                clear_color: config.clear_color,
            },
        )?;
        let host = Host::Window {
            window: window.clone(),
            proxy,
        };
        let mut ctx = Context::new(gfx, host, config)?;
        ctx.focused = window.has_focus();
        ctx.windows.main_focused = ctx.focused;
        crate::ui_hook::setup(&mut ctx, config);
        let game = G::init(&mut ctx).map_err(Error::Game)?;
        window.request_redraw();
        Ok(Self {
            window,
            ctx,
            game,
            stepper: Stepper::new(config, fixed_clock),
            occluded: false,
            turn_pending: false,
            handle_close: config.handle_close,
        })
    }

    /// Whether the window cannot be seen, so frames update without drawing.
    pub(crate) fn hidden(&self) -> bool {
        let s = self.window.inner_size();
        self.occluded
            || s.width == 0
            || s.height == 0
            || self.window.is_minimized().unwrap_or(false)
    }

    /// Applies what the game asked of the window during a callback and
    /// needs the event loop to carry out: a changed cursor.
    pub(crate) fn apply_requests(&mut self, el: &ActiveEventLoop) {
        if !std::mem::take(&mut self.ctx.requests.cursor_dirty) {
            return;
        }
        match &self.ctx.cursor {
            Cursor::Custom(image) => match image.to_source() {
                Ok(source) => self.window.set_cursor(el.create_custom_cursor(source)),
                Err(e) => log::warn!("cursor not set: {e}"),
            },
            shape => self
                .window
                .set_cursor(shape.icon().unwrap_or(winit::window::CursorIcon::Default)),
        }
    }

    /// Seconds per update.
    pub(crate) fn step(&self) -> f64 {
        self.stepper.step()
    }

    /// Runs one frame: the updates due, and the draw unless `draw` is false.
    pub(crate) fn frame(&mut self, draw: bool) -> Result<()> {
        let turn = std::mem::take(&mut self.turn_pending);
        self.stepper
            .tick(&mut self.ctx, &mut self.game, Tick { turn, draw })
    }

    /// Shows an event of window `id` to the game.
    pub(crate) fn deliver_to(&mut self, id: WindowId, event: Event) -> Result<()> {
        self.game
            .window_event(&mut self.ctx, id, &event)
            .map_err(Error::Game)
    }

    /// Applies an input event to the input state and shows it to the game.
    pub(crate) fn input(&mut self, event: InputEvent) -> Result<()> {
        self.input_for(WindowId::MAIN, event)
    }

    /// Like [`input`](Self::input) for window `id`. Pointer events of extra
    /// windows only reach the game: the input state's pointer belongs to
    /// the main window.
    pub(crate) fn input_for(&mut self, id: WindowId, event: InputEvent) -> Result<()> {
        let pointer = matches!(
            event,
            InputEvent::MouseMoved { .. } | InputEvent::MouseLeft | InputEvent::MouseButton { .. }
        );
        if id == WindowId::MAIN || !pointer {
            self.ctx.input.apply(&event);
        }
        self.turn_pending = true;
        if self.ctx.input_captured() {
            return Ok(());
        }
        self.deliver_to(id, Event::Input(event))
    }

    fn pointer(&mut self, id: WindowId, pos: winit::dpi::PhysicalPosition<f64>) -> InputEvent {
        let window = vec2(pos.x as f32, pos.y as f32);
        let view = if id == WindowId::MAIN {
            self.ctx.gfx.window_to_view(window)
        } else {
            let view = self
                .ctx
                .gfx
                .window_view_mapping(id)
                .map_or(window, |m| m.pixels_to_view(window));
            if let Some(e) = self.ctx.windows.get_mut(id) {
                e.pointer = Some(view);
            }
            view
        };
        InputEvent::MouseMoved { view, window }
    }

    /// Handles every main-window event except redraw requests.
    pub(crate) fn window_event(&mut self, event: WindowEvent) -> Result<()> {
        self.window_event_for(WindowId::MAIN, event)
    }

    /// Handles an event of window `id`.
    pub(crate) fn window_event_for(&mut self, id: WindowId, event: WindowEvent) -> Result<()> {
        match event {
            WindowEvent::CloseRequested => self.on_close(id)?,
            WindowEvent::Resized(size) => self.on_resized(id, size)?,
            WindowEvent::Focused(focused) => self.on_focus(id, focused)?,
            WindowEvent::Occluded(occluded) => self.on_occluded(id, occluded)?,
            WindowEvent::HoveredFile(path) => {
                self.turn_pending = true;
                self.deliver_to(id, Event::FileHovered(path))?;
            }
            WindowEvent::HoveredFileCancelled => {
                self.turn_pending = true;
                self.deliver_to(id, Event::FileHoverCancelled)?;
            }
            WindowEvent::DroppedFile(path) => {
                self.turn_pending = true;
                self.deliver_to(id, Event::FileDropped(path))?;
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // Synthetic presses replay held keys when focus returns; the
                // release on focus loss already reset them.
                if is_synthetic {
                    return Ok(());
                }
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(key) = keymap::key(code)
                {
                    self.input_for(
                        id,
                        if pressed {
                            InputEvent::KeyDown {
                                key,
                                repeat: event.repeat,
                            }
                        } else {
                            InputEvent::KeyUp { key }
                        },
                    )?;
                }
                if pressed
                    && let Some(text) = event.text
                    && text.chars().any(|c| !c.is_control())
                {
                    self.input_for(id, InputEvent::Text(text.to_string()))?;
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.input_for(id, InputEvent::Modifiers(keymap::modifiers(m.state())))?;
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(text, cursor) => {
                    self.input_for(id, InputEvent::Composition { text, cursor })?;
                }
                Ime::Commit(text) => {
                    self.input_for(
                        id,
                        InputEvent::Composition {
                            text: String::new(),
                            cursor: None,
                        },
                    )?;
                    self.input_for(id, InputEvent::Text(text))?;
                }
                Ime::Disabled => {
                    self.input_for(
                        id,
                        InputEvent::Composition {
                            text: String::new(),
                            cursor: None,
                        },
                    )?;
                }
                Ime::Enabled => {}
            },
            WindowEvent::CursorMoved { position, .. } => {
                let ev = self.pointer(id, position);
                self.input_for(id, ev)?;
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(e) = self.ctx.windows.get_mut(id) {
                    e.pointer = None;
                }
                self.input_for(id, InputEvent::MouseLeft)?;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = keymap::mouse_button(button) {
                    self.input_for(
                        id,
                        InputEvent::MouseButton {
                            button,
                            pressed: state == ElementState::Pressed,
                        },
                    )?;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(x, y) => vec2(x, y),
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = self.native(id).map_or(1.0, |w| w.scale_factor());
                        let l = p.to_logical::<f64>(scale);
                        vec2(
                            (l.x / POINTS_PER_LINE) as f32,
                            (l.y / POINTS_PER_LINE) as f32,
                        )
                    }
                };
                self.input_for(id, InputEvent::Wheel { lines })?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Raw pointer motion, reported while the window has focus.
    pub(crate) fn mouse_motion(&mut self, delta: (f64, f64)) -> Result<()> {
        if !self.ctx.focused {
            return Ok(());
        }
        self.input(InputEvent::MouseMotion {
            delta: Vec2::new(delta.0 as f32, delta.1 as f32),
        })
    }
}
