//! Extra outputs: more windows (or offscreen images standing in for them)
//! drawn by the same renderer.
//!
//! The renderer keeps one set of per-frame state for the main output (its
//! draw lists, view, clear colour and surface). An extra output has its
//! own copy of that set. [`Graphics::draw_in`] swaps a window's set in for
//! the duration of a closure, so every `draw_*` call inside it records into
//! that window, and [`Graphics::end_window_frame`] swaps it in again to
//! render and present it. Textures, fonts and shaders belong to the device
//! and work in every window.

use image::RgbaImage;
use yowie_core::UVec2;

use crate::batch::{DrawList, FrameTables};
use crate::error::{GfxError, Result};
use crate::graphics::{FrameStats, Graphics, GraphicsSettings};
use crate::output::Output;
use crate::scene::SceneRecordList;
use crate::state::Cursor;
use crate::view::{ViewMapping, ViewSettings};

/// Names a window the renderer draws into. [`WindowId::MAIN`] is the
/// output the renderer was created with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u32);

impl WindowId {
    /// The main window, or the headless image of a headless run.
    pub const MAIN: WindowId = WindowId(0);
}

/// The per-output state `draw_in` swaps with the renderer's own.
#[derive(Debug)]
struct Target {
    id: WindowId,
    output: Output,
    view_settings: ViewSettings,
    scale_factor: f32,
    mapping: ViewMapping,
    clear_color: yowie_core::Color,
    lists: Vec<DrawList>,
    list_stack: Vec<usize>,
    encode_order: Vec<usize>,
    tables: FrameTables,
    cursor: Cursor,
    scenes: SceneRecordList,
}

/// The extra outputs of a renderer.
#[derive(Debug, Default)]
pub(crate) struct ExtraOutputs {
    targets: Vec<Target>,
    next: u32,
}

impl Graphics {
    fn swap_target(&mut self, t: &mut Target) {
        std::mem::swap(&mut self.output, &mut t.output);
        std::mem::swap(&mut self.view_settings, &mut t.view_settings);
        std::mem::swap(&mut self.scale_factor, &mut t.scale_factor);
        std::mem::swap(&mut self.mapping, &mut t.mapping);
        std::mem::swap(&mut self.clear_color, &mut t.clear_color);
        std::mem::swap(&mut self.lists, &mut t.lists);
        std::mem::swap(&mut self.list_stack, &mut t.list_stack);
        std::mem::swap(&mut self.encode_order, &mut t.encode_order);
        std::mem::swap(&mut self.tables, &mut t.tables);
        std::mem::swap(&mut self.cursor, &mut t.cursor);
    }

    /// Reserves a window id without creating the output, so a window that
    /// the window system creates later can be named at once. Pass it to
    /// [`add_window_output`](Self::add_window_output) or
    /// [`add_offscreen_output`](Self::add_offscreen_output).
    pub fn reserve_window_id(&mut self) -> WindowId {
        self.extras.next += 1;
        WindowId(self.extras.next)
    }

    fn new_target(
        &mut self,
        id: WindowId,
        output: Output,
        scale_factor: f32,
        settings: GraphicsSettings,
    ) -> WindowId {
        self.extras.targets.retain(|t| t.id != id);
        let mut t = Target {
            id,
            mapping: ViewMapping::new(settings.view, output.size(), scale_factor),
            output,
            view_settings: settings.view,
            scale_factor,
            clear_color: settings.clear_color,
            lists: Vec::new(),
            list_stack: Vec::new(),
            encode_order: Vec::new(),
            tables: FrameTables::default(),
            cursor: Cursor::default(),
            scenes: SceneRecordList::default(),
        };
        self.swap_target(&mut t);
        self.begin_frame();
        self.swap_target(&mut t);
        self.extras.targets.push(t);
        id
    }

    /// Adds a window output named `id` (from
    /// [`reserve_window_id`](Self::reserve_window_id)) that presents to
    /// `target`. `size` is the window's drawable size in pixels and
    /// `scale_factor` its pixels per logical point. `settings.vsync`
    /// applies to this window alone.
    pub fn add_window_output(
        &mut self,
        id: WindowId,
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        size: UVec2,
        scale_factor: f32,
        settings: GraphicsSettings,
    ) -> Result<WindowId> {
        let surface = self
            .shared
            .gpu
            .instance
            .create_surface(target)
            .map_err(|e| GfxError::CreateSurface(e.to_string()))?;
        let output = Output::surface(&self.shared.gpu, surface, size, settings.vsync)?;
        Ok(self.new_target(id, output, scale_factor, settings))
    }

    /// Adds an output named `id` that renders into an image of `size`
    /// pixels instead of a window, for headless runs. Read it with
    /// [`read_window_image`](Self::read_window_image).
    pub fn add_offscreen_output(
        &mut self,
        id: WindowId,
        size: UVec2,
        scale_factor: f32,
        settings: GraphicsSettings,
    ) -> WindowId {
        let output = Output::headless(&self.shared.gpu, size);
        self.new_target(id, output, scale_factor, settings)
    }

    /// Removes an extra output and drops its surface. Returns whether it
    /// existed. The main window cannot be removed.
    pub fn remove_window_output(&mut self, id: WindowId) -> bool {
        let before = self.extras.targets.len();
        self.extras.targets.retain(|t| t.id != id);
        self.extras.targets.len() != before
    }

    /// Whether `id` names the main window or an extra output.
    pub fn has_window_output(&self, id: WindowId) -> bool {
        id == WindowId::MAIN || self.extras.targets.iter().any(|t| t.id == id)
    }

    /// Resizes an extra output after its window changed size or moved to a
    /// display of another density.
    pub fn resize_window_output(&mut self, id: WindowId, size: UVec2, scale_factor: f32) {
        if id == WindowId::MAIN {
            self.resize(size, scale_factor);
            return;
        }
        let Some(idx) = self.extras.targets.iter().position(|t| t.id == id) else {
            return;
        };
        let mut t = self.extras.targets.swap_remove(idx);
        if size != t.output.size() {
            t.output.resize(&self.shared.gpu, size);
        }
        t.scale_factor = scale_factor;
        t.mapping = ViewMapping::new(t.view_settings, t.output.size(), scale_factor);
        self.swap_target(&mut t);
        self.begin_frame();
        self.swap_target(&mut t);
        self.extras.targets.push(t);
    }

    /// How an extra output's view maps into its pixels.
    pub fn window_view_mapping(&self, id: WindowId) -> Option<ViewMapping> {
        if id == WindowId::MAIN {
            return Some(self.mapping);
        }
        self.extras
            .targets
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.mapping)
    }

    /// Records drawing into window `id` with `f`. Inside, every `draw_*`
    /// call goes to that window, and [`view_size`](Self::view_size) is its
    /// view. Returns `None` without calling `f` when the window does not
    /// exist. Do not nest `draw_in` scopes. Drawing to the main window
    /// uses plain `draw_*` calls outside any scope.
    pub fn draw_in<R>(&mut self, id: WindowId, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if id == WindowId::MAIN {
            return Some(f(self));
        }
        let idx = self.extras.targets.iter().position(|t| t.id == id)?;
        let mut t = self.extras.targets.swap_remove(idx);
        self.swap_target(&mut t);
        if let Some(s) = self.scenes.as_deref_mut() {
            s.swap_records(&mut t.scenes);
        }
        let r = f(self);
        // A renderer created inside the scope holds the scenes it recorded.
        if let Some(s) = self.scenes.as_deref_mut() {
            s.swap_records(&mut t.scenes);
        }
        self.swap_target(&mut t);
        self.extras.targets.push(t);
        Some(r)
    }

    /// Renders and presents the frame recorded for window `id`, then starts
    /// the next one. Returns what the window's frame cost; the main
    /// window's [`stats`](Self::stats) are unchanged.
    pub fn end_window_frame(&mut self, id: WindowId) -> Result<FrameStats> {
        if id == WindowId::MAIN {
            return self.end_frame();
        }
        let Some(idx) = self.extras.targets.iter().position(|t| t.id == id) else {
            return Ok(FrameStats::default());
        };
        let mut t = self.extras.targets.swap_remove(idx);
        self.swap_target(&mut t);
        if let Some(s) = self.scenes.as_deref_mut() {
            s.swap_records(&mut t.scenes);
        }
        let main_stats = self.stats;
        let capture = std::mem::take(&mut self.capture_requested);
        let result = self.end_frame();
        self.capture_requested = capture;
        self.stats = main_stats;
        if let Some(s) = self.scenes.as_deref_mut() {
            s.swap_records(&mut t.scenes);
        }
        self.swap_target(&mut t);
        self.extras.targets.push(t);
        result
    }

    /// Copies an offscreen output's last frame back, as sRGB RGBA pixels.
    /// A window output's frames cannot be read.
    pub fn read_window_image(&self, id: WindowId) -> Result<RgbaImage> {
        if id == WindowId::MAIN {
            return self.read_output();
        }
        let t = self
            .extras
            .targets
            .iter()
            .find(|t| t.id == id)
            .ok_or(GfxError::FrameState("no such window output"))?;
        match &t.output {
            Output::Headless { texture } => {
                let device = self.shared.device();
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yowie window readback"),
                });
                let pending = crate::readback::record(device, &mut encoder, texture)?;
                self.shared.queue().submit([encoder.finish()]);
                pending.finish(device, false)
            }
            Output::Surface { .. } => Err(GfxError::FrameState(
                "a window's frames cannot be read back",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_tests::headless;
    use yowie_core::{Color, Rect, uvec2};

    #[test]
    fn extra_outputs_draw_independently() {
        let Some(mut gfx) = headless(uvec2(64, 64)) else {
            return;
        };
        let settings = GraphicsSettings {
            clear_color: Color::rgb8(0, 0, 200),
            ..GraphicsSettings::default()
        };
        let id = gfx.reserve_window_id();
        let win = gfx.add_offscreen_output(id, uvec2(32, 16), 1.0, settings);
        assert!(gfx.has_window_output(win));
        let drawn = gfx.draw_in(win, |g| {
            assert_eq!(g.view_size(), yowie_core::vec2(32.0, 16.0));
            g.fill_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Color::rgb8(255, 0, 0));
        });
        assert!(drawn.is_some());
        gfx.fill_rect(Rect::new(0.0, 0.0, 64.0, 64.0), Color::rgb8(0, 255, 0));
        gfx.end_window_frame(win).expect("window frame");
        gfx.end_frame().expect("main frame");
        let w = gfx.read_window_image(win).expect("window image");
        assert_eq!((w.width(), w.height()), (32, 16));
        assert_eq!(w.get_pixel(4, 4).0, [255, 0, 0, 255]);
        assert_eq!(w.get_pixel(24, 4).0, [0, 0, 200, 255]);
        let m = gfx.read_output().expect("main image");
        assert_eq!(m.get_pixel(40, 40).0, [0, 255, 0, 255]);
    }

    #[test]
    fn unknown_and_removed_outputs_draw_nothing() {
        let Some(mut gfx) = headless(uvec2(16, 16)) else {
            return;
        };
        assert!(gfx.draw_in(WindowId(9), |_| ()).is_none());
        let id = gfx.reserve_window_id();
        let win = gfx.add_offscreen_output(id, uvec2(8, 8), 1.0, GraphicsSettings::default());
        assert!(gfx.remove_window_output(win));
        assert!(!gfx.remove_window_output(win));
        assert!(gfx.draw_in(win, |_| ()).is_none());
        assert!(gfx.draw_in(WindowId::MAIN, |_| ()).is_some());
    }

    #[test]
    fn resizing_an_output_updates_its_view() {
        let Some(mut gfx) = headless(uvec2(16, 16)) else {
            return;
        };
        let id = gfx.reserve_window_id();
        let win = gfx.add_offscreen_output(id, uvec2(8, 8), 1.0, GraphicsSettings::default());
        gfx.resize_window_output(win, uvec2(40, 20), 1.0);
        let size = gfx.draw_in(win, |g| g.view_size());
        assert_eq!(size, Some(yowie_core::vec2(40.0, 20.0)));
    }
}
