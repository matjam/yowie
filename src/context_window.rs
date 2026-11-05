//! Window controls beyond the basics: cursor, icon, size limits, position
//! and stacking.

use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::WindowLevel;
use yowie_core::{IVec2, UVec2};

use crate::context::Host;
use crate::icon::{Cursor, Icon};
use crate::{Context, Result};

impl Context {
    /// Sets how the pointer looks over the window. A [`Cursor::Custom`]
    /// image is shown from the next event-loop turn.
    pub fn set_cursor(&mut self, cursor: Cursor) {
        if self.cursor != cursor {
            self.cursor = cursor;
            self.requests.cursor_dirty = true;
        }
    }

    /// The cursor last set; [`Cursor::Arrow`] at the start.
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Sets the window icon, or restores the default with `None`. See
    /// [`Icon`] for which platforms show it.
    pub fn set_window_icon(&mut self, icon: Option<&Icon>) -> Result<()> {
        if let Host::Window { window, .. } = &self.host {
            window.set_window_icon(icon.map(Icon::to_winit).transpose()?);
        }
        Ok(())
    }

    /// Keeps the window above other windows, or returns it to the normal
    /// stacking order. Wayland has no protocol for this and the call has
    /// no effect there; the compositor decides stacking (most offer a
    /// "keep above" action in the window menu).
    pub fn set_always_on_top(&mut self, on: bool) {
        self.always_on_top = on;
        if let Host::Window { window, .. } = &self.host {
            window.set_window_level(if on {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            });
        }
    }

    /// Whether [`set_always_on_top`](Context::set_always_on_top) asked for
    /// the window to stay above others.
    pub fn always_on_top(&self) -> bool {
        self.always_on_top
    }

    /// Limits how small the player can resize the window, in logical
    /// points of content size; `None` removes the limit.
    pub fn set_min_window_size(&mut self, size: Option<UVec2>) {
        if let Host::Window { window, .. } = &self.host {
            window.set_min_inner_size(size.map(|s| LogicalSize::new(s.x.max(1), s.y.max(1))));
        }
    }

    /// Limits how large the player can resize the window, in logical
    /// points of content size; `None` removes the limit.
    pub fn set_max_window_size(&mut self, size: Option<UVec2>) {
        if let Host::Window { window, .. } = &self.host {
            window.set_max_inner_size(size.map(|s| LogicalSize::new(s.x.max(1), s.y.max(1))));
        }
    }

    /// The window's top-left corner on the desktop in logical points, or
    /// `None` in a headless run and where the window system does not tell
    /// clients (Wayland).
    pub fn window_position(&self) -> Option<IVec2> {
        let Host::Window { window, .. } = &self.host else {
            return None;
        };
        let p = window
            .outer_position()
            .ok()?
            .to_logical::<f64>(window.scale_factor());
        Some(IVec2::new(p.x.round() as i32, p.y.round() as i32))
    }

    /// Moves the window's top-left corner to logical desktop coordinates.
    /// Wayland compositors place windows themselves and ignore the
    /// request.
    pub fn set_window_position(&mut self, x: i32, y: i32) {
        if let Host::Window { window, .. } = &self.host {
            window.set_outer_position(LogicalPosition::new(x, y));
        }
    }

    /// Whether the window is minimised, where the window system says.
    pub fn minimized(&self) -> bool {
        match &self.host {
            Host::Window { window, .. } => window.is_minimized().unwrap_or(false),
            Host::Headless { .. } => false,
        }
    }
}
