//! The window events whose handling depends on which window they came
//! from, and the creation and removal of extra windows.

use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;
use yowie_core::UVec2;
use yowie_input::InputEvent;

use crate::window::WindowState;
use crate::{Event, Game, Result, WindowId};

impl<G: Game> WindowState<G> {
    /// The window system window of `id`.
    pub(crate) fn native(&self, id: WindowId) -> Option<Arc<Window>> {
        if id == WindowId::MAIN {
            Some(self.window.clone())
        } else {
            self.ctx.windows.get(id)?.window.clone()
        }
    }

    /// The close button was pressed. The main window ends the run unless
    /// the config says the game handles it; an extra window closes unless
    /// its own settings say so.
    pub(crate) fn on_close(&mut self, id: WindowId) -> Result<()> {
        self.deliver_to(id, Event::CloseRequested)?;
        if id == WindowId::MAIN {
            if !self.handle_close {
                self.ctx.quit();
            }
        } else if self.ctx.windows.get(id).is_some_and(|e| !e.handle_close) {
            self.ctx.close_window(id);
        }
        Ok(())
    }

    pub(crate) fn on_resized(&mut self, id: WindowId, size: PhysicalSize<u32>) -> Result<()> {
        let Some(window) = self.native(id) else {
            return Ok(());
        };
        let scale = window.scale_factor() as f32;
        let px = UVec2::new(size.width.max(1), size.height.max(1));
        if id == WindowId::MAIN {
            self.ctx.gfx.resize(px, scale);
        } else {
            self.ctx.gfx.resize_window_output(id, px, scale);
            let logical = size.to_logical::<f64>(f64::from(scale));
            if let Some(e) = self.ctx.windows.get_mut(id) {
                e.scale_factor = scale;
                e.size = UVec2::new(
                    (logical.width.round() as u32).max(1),
                    (logical.height.round() as u32).max(1),
                );
            }
        }
        window.request_redraw();
        self.turn_pending = true;
        self.deliver_to(
            id,
            Event::Resized {
                width: size.width,
                height: size.height,
                scale_factor: scale,
            },
        )
    }

    pub(crate) fn on_focus(&mut self, id: WindowId, focused: bool) -> Result<()> {
        if id == WindowId::MAIN {
            self.ctx.windows.main_focused = focused;
        } else if let Some(e) = self.ctx.windows.get_mut(id) {
            e.focused = focused;
        }
        self.ctx.focused = self.ctx.windows.any_focused();
        if !focused {
            self.input_for(id, InputEvent::FocusLost)?;
        }
        self.turn_pending = true;
        self.deliver_to(id, Event::Focused(focused))
    }

    pub(crate) fn on_occluded(&mut self, id: WindowId, occluded: bool) -> Result<()> {
        if id == WindowId::MAIN {
            self.occluded = occluded;
            if !occluded {
                // Draw the frame that was skipped while hidden.
                self.turn_pending = true;
                self.window.request_redraw();
            }
        }
        self.deliver_to(id, Event::Occluded(occluded))
    }

    /// Creates the windows the game asked for and removes the ones it
    /// closed.
    pub(crate) fn apply_windows(&mut self, el: &ActiveEventLoop) -> Result<()> {
        self.ctx.create_windows(el);
        crate::windows::process_closes(&mut self.ctx, &mut self.game)
    }
}
