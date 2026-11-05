//! The drawing context: device, output, and the frame being recorded.

use std::sync::Arc;

use image::RgbaImage;
use yowie_core::{Color, Rect, UVec2, Vec2};

use crate::batch::{DrawList, FrameTables, ListTarget};
use crate::blend::BlendMode;
use crate::device::Gpu;
use crate::engine_shaders::EngineShaders;
use crate::error::Result;
use crate::frame::FrameBuffers;
use crate::output::Output;
use crate::pipelines::{PipelineCache, trusted_module};
use crate::shader::{DEFAULT_FRAGMENT, compose};
use crate::shared::Shared;
use crate::state::Cursor;
use crate::text::TextSystem;
use crate::texture::{Purpose, Texture, TextureOptions};
use crate::view::{ViewMapping, ViewSettings};

/// Settings fixed when the renderer is created.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicsSettings {
    /// Whether presentation waits for the display's refresh.
    pub vsync: bool,
    /// The game's 2D coordinate space.
    pub view: ViewSettings,
    /// The colour each frame starts with, sRGB.
    pub clear_color: Color,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            vsync: true,
            view: ViewSettings::default(),
            clear_color: Color::BLACK,
        }
    }
}

/// What the last frame cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// 2D draw calls after batching.
    pub draw_calls: u32,
    /// Vertices uploaded for 2D drawing.
    pub vertices: u32,
    /// Indices drawn for 2D drawing.
    pub indices: u32,
    /// Outputs rendered: the screen plus each render texture drawn to.
    pub outputs: u32,
    /// 3D draw calls after instancing, shadow maps and resolves included.
    pub draw_calls_3d: u32,
    /// 3D mesh draws recorded; each instance counts once.
    pub draws_3d: u32,
    /// 3D draws skipped because they were outside the camera's view.
    pub culled_3d: u32,
    /// Point and spot lights kept after culling against the view.
    pub lights: u32,
    /// Lights refused for exceeding a limit, and cluster entries dropped
    /// from full clusters.
    pub lights_dropped: u32,
    /// Instances drawn into shadow maps.
    pub shadow_instances: u32,
}

/// The renderer for one window or headless image.
///
/// Every `draw_*`, `fill_*` and `stroke_*` call queues 2D work; the frame
/// is rendered when [`end_frame`](Self::end_frame) runs. Coordinates are
/// view units with the origin at the top-left and +Y down; angles are
/// radians, clockwise on screen. Colours are sRGB with straight alpha. See
/// the crate documentation for the frame's passes.
pub struct Graphics {
    pub(crate) shared: Shared,
    pub(crate) output: Output,
    view_settings: ViewSettings,
    scale_factor: f32,
    mapping: ViewMapping,
    clear_color: Color,
    pub(crate) time: f32,
    pub(crate) lists: Vec<DrawList>,
    pub(crate) list_stack: Vec<usize>,
    pub(crate) encode_order: Vec<usize>,
    pub(crate) tables: FrameTables,
    pub(crate) cursor: Cursor,
    pub(crate) white: Texture,
    pub(crate) pipelines: PipelineCache,
    pub(crate) default_module: wgpu::ShaderModule,
    pub(crate) engine_shaders: EngineShaders,
    pub(crate) path_cache: crate::path::PathCache,
    pub(crate) buffers: FrameBuffers,
    pub(crate) text: TextSystem,
    pub(crate) capture_requested: bool,
    pub(crate) capture: Option<Result<RgbaImage>>,
    pub(crate) stats: FrameStats,
    pub(crate) frame_number: u64,
    /// The 3D renderer, created by the first scene.
    pub(crate) scenes: Option<Box<crate::scene::SceneRenderer>>,
    pub(crate) lit: crate::lights::LitState,
    pub(crate) extensions: parking_lot::Mutex<
        std::collections::HashMap<std::any::TypeId, Arc<dyn std::any::Any + Send + Sync>>,
    >,
}

impl std::fmt::Debug for Graphics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graphics")
            .field("output_size", &self.output.size())
            .field("mapping", &self.mapping)
            .finish_non_exhaustive()
    }
}

impl Graphics {
    /// Creates a renderer that presents to a window. `size` is the window's
    /// drawable size in pixels and `scale_factor` its pixels per logical
    /// point. Blocks while the GPU device is created.
    pub fn new_windowed(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        size: UVec2,
        scale_factor: f32,
        settings: GraphicsSettings,
    ) -> Result<Graphics> {
        let target = target.into();
        let (gpu, surface) = pollster::block_on(Gpu::new(
            |instance| {
                instance
                    .create_surface(target)
                    .map_err(|e| crate::GfxError::CreateSurface(e.to_string()))
            },
            |s| Some(s),
        ))?;
        let output = Output::surface(&gpu, surface, size, settings.vsync)?;
        Graphics::with_output(gpu, output, settings, scale_factor)
    }

    /// Creates a renderer with no window that renders into an image of
    /// `size` pixels, for screenshots, tests and servers. Blocks while the
    /// GPU device is created.
    pub fn new_headless(size: UVec2, settings: GraphicsSettings) -> Result<Graphics> {
        let gpu = pollster::block_on(Gpu::headless())?;
        let output = Output::headless(&gpu, size);
        Graphics::with_output(gpu, output, settings, 1.0)
    }

    fn with_output(
        gpu: Gpu,
        output: Output,
        settings: GraphicsSettings,
        scale_factor: f32,
    ) -> Result<Graphics> {
        let shared = Shared::new(gpu);
        let engine_shaders = EngineShaders::new(shared.device());
        let default_module =
            trusted_module(shared.device(), "yowie sprite", &compose(DEFAULT_FRAGMENT));
        let white = Texture::create(
            &shared,
            UVec2::ONE,
            crate::output::OFFSCREEN_FORMAT,
            TextureOptions::default(),
            Purpose::Sampled,
        )?;
        white.write_level0(&shared, 0, 0, 1, 1, &[255; 4], 4);
        let text = TextSystem::new(&shared)?;
        let buffers = FrameBuffers::new(&shared);
        let mapping = ViewMapping::new(settings.view, output.size(), scale_factor);
        let mut gfx = Graphics {
            shared,
            output,
            view_settings: settings.view,
            scale_factor,
            mapping,
            clear_color: settings.clear_color,
            time: 0.0,
            lists: Vec::new(),
            list_stack: Vec::new(),
            encode_order: Vec::new(),
            tables: FrameTables::default(),
            cursor: Cursor::default(),
            white,
            pipelines: PipelineCache::default(),
            default_module,
            engine_shaders,
            path_cache: Default::default(),
            buffers,
            text,
            capture_requested: false,
            capture: None,
            stats: FrameStats::default(),
            frame_number: 0,
            scenes: None,
            lit: Default::default(),
            extensions: Default::default(),
        };
        gfx.begin_frame();
        Ok(gfx)
    }

    /// Resizes the output to `size` pixels with `scale_factor` pixels per
    /// logical point, after the window changed size or moved to another
    /// display.
    pub fn resize(&mut self, size: UVec2, scale_factor: f32) {
        if size != self.output.size() {
            self.output.resize(&self.shared.gpu, size);
        }
        self.scale_factor = scale_factor;
        self.mapping = ViewMapping::new(self.view_settings, self.output.size(), scale_factor);
    }

    /// Turns waiting for the display's refresh on or off.
    pub fn set_vsync(&mut self, vsync: bool) {
        self.output.set_vsync(&self.shared.gpu, vsync);
    }

    /// Changes the game's 2D coordinate space.
    pub fn set_view_settings(&mut self, view: ViewSettings) {
        self.view_settings = view;
        self.mapping = ViewMapping::new(view, self.output.size(), self.scale_factor);
    }

    /// The colour each frame starts with.
    pub fn clear_color(&self) -> Color {
        self.clear_color
    }

    /// Sets the colour the next frame starts with.
    pub fn set_clear_color(&mut self, color: Color) {
        self.clear_color = color;
    }

    /// Sets the time in seconds that shaders read as `input.time`. The
    /// engine sets it from game time before each frame.
    pub fn set_time(&mut self, seconds: f32) {
        self.time = seconds;
    }

    /// Starts recording a frame, dropping anything queued since the last
    /// one. The engine calls it before the game draws.
    pub fn begin_frame(&mut self) {
        self.lists.clear();
        self.list_stack.clear();
        self.encode_order.clear();
        self.tables.clear();
        if let Some(scenes) = &mut self.scenes {
            scenes.begin_frame();
        }
        self.lit.begin_frame();
        self.cursor = Cursor::default();
        let m = self.mapping;
        let bars = m.letterboxed();
        let load = if bars { Color::BLACK } else { self.clear_color };
        self.lists
            .push(DrawList::new(ListTarget::Screen, m, Some(load)));
        self.list_stack.push(0);
        if bars {
            // The bars stay black; the view area takes the clear colour.
            let clear = self.clear_color;
            self.with_layer(i32::MIN, |g| {
                g.with_blend(BlendMode::Replace, |g| {
                    g.fill_rect(Rect::new(0.0, 0.0, m.view_size.x, m.view_size.y), clear);
                });
            });
        }
    }

    /// Asks for the next frame to be copied back; collect it with
    /// [`take_capture`](Self::take_capture) after [`end_frame`](Self::end_frame).
    pub fn request_capture(&mut self) {
        self.capture_requested = true;
    }

    /// The frame captured by the last [`end_frame`](Self::end_frame) after a
    /// [`request_capture`](Self::request_capture), as sRGB RGBA pixels.
    pub fn take_capture(&mut self) -> Option<Result<RgbaImage>> {
        self.capture.take()
    }

    /// What the last frame cost.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// How the screen's view maps into the output.
    pub fn view_mapping(&self) -> ViewMapping {
        self.mapping
    }

    /// The size of the current output's view in view units: the screen's,
    /// or the render texture's inside [`draw_to`](Self::draw_to).
    pub fn view_size(&self) -> Vec2 {
        self.current_list().mapping.view_size
    }

    /// Output pixels per view unit for the current output.
    pub fn pixels_per_unit(&self) -> f32 {
        self.current_list().mapping.pixels_per_unit().x
    }

    /// Converts a position in window pixels to view units, through the
    /// letterbox of a fixed view.
    pub fn window_to_view(&self, pixels: Vec2) -> Vec2 {
        self.mapping.pixels_to_view(pixels)
    }

    /// The output's size in pixels.
    pub fn output_size(&self) -> UVec2 {
        self.output.size()
    }

    /// The texture format frames are rendered in.
    pub fn output_format(&self) -> wgpu::TextureFormat {
        self.output.format()
    }

    /// The wgpu instance, for creating surfaces for more windows.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.shared.gpu.instance
    }

    /// The wgpu device, for renderer extensions.
    pub fn device(&self) -> &wgpu::Device {
        self.shared.device()
    }

    /// The wgpu queue, for renderer extensions.
    pub fn queue(&self) -> &wgpu::Queue {
        self.shared.queue()
    }

    /// The adapter's name, backend and type.
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.shared.gpu.adapter.get_info()
    }

    pub(crate) fn current_index(&self) -> usize {
        self.list_stack.last().copied().unwrap_or(0)
    }

    pub(crate) fn current_list(&self) -> &DrawList {
        // begin_frame always leaves the screen list at index 0.
        &self.lists[self.current_index()]
    }
}
