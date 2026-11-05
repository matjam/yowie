//! Custom GPU work inside the frame: compute dispatches before the passes
//! and draws recorded into the 2D pass or a 3D scene's HDR pass.
//!
//! GPU particles use this hook. A renderer extension implements
//! [`GpuDraw`] and queues it with [`Graphics::draw_gpu`] (2D, ordered by
//! layer and sort key like a sprite) or `Scene::draw_gpu` (3D, after the
//! blended meshes). When the frame ends the renderer calls
//! [`prepare`](GpuDraw::prepare) on the command encoder before any pass is
//! open, then [`draw_2d`](GpuDraw::draw_2d) or
//! [`draw_3d`](GpuDraw::draw_3d) inside the pass. Queueing the same draw
//! twice in a frame (to a render texture and to the screen) calls `prepare`
//! twice and `draw_*` once for each queueing, so an implementation advances
//! its simulation only on the first `prepare` of a frame.
//!
//! The renderer forgets its bound pipeline and bind groups after a draw,
//! so a draw may set whatever it needs, including a different bind group
//! at any index.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

use crate::Graphics;
use crate::batch::{CUSTOM_SHADER, DrawState};
use crate::blend::BlendMode;

/// What [`GpuDraw::prepare`] can use to record compute work.
#[derive(Debug)]
pub struct PrepareContext<'a> {
    /// The device.
    pub device: &'a wgpu::Device,
    /// The queue, for small buffer writes.
    pub queue: &'a wgpu::Queue,
    /// The frame's command encoder. No render pass is open on it.
    pub encoder: &'a mut wgpu::CommandEncoder,
}

/// The state bound when a custom draw records into the 2D pass.
///
/// The view block at `view_group` is the `YowieView` struct of the 2D
/// shader prelude: a `mat4x4<f32>` `view_proj` mapping drawing
/// coordinates to clip space, then `view_size: vec2<f32>`,
/// `pixel_scale: f32` and `time: f32`. Bind it with the dynamic offset
/// `view_offset`. The viewport and scissor rectangle are already set for
/// the draw's camera and clip.
#[derive(Debug)]
pub struct Draw2D<'a> {
    /// The colour format of the target.
    pub format: wgpu::TextureFormat,
    /// The layout of the view block: one uniform buffer binding at
    /// binding 0 with a dynamic offset, visible to vertex and fragment
    /// stages.
    pub view_layout: &'a wgpu::BindGroupLayout,
    /// The bind group holding the view block.
    pub view_group: &'a wgpu::BindGroup,
    /// The dynamic offset of this draw's view block in `view_group`.
    pub view_offset: u32,
    /// Output pixels per view unit.
    pub pixel_scale: f32,
}

/// The state bound when a custom draw records into a 3D scene's HDR pass.
///
/// Group 0 is bound to the scene's frame block (`Frame` in the renderer's
/// WGSL: `view`, `proj`, `view_proj` and `inv_view_proj` matrices, then
/// `camera` position and the rest). A pipeline that keeps group 0 in its
/// layout, built with `frame_layout`, draws with it bound. The pass has a
/// depth attachment that opaque meshes wrote with reversed depth (the
/// far plane is 0): test with `GreaterEqual` and do not write.
#[derive(Debug)]
pub struct Draw3D<'a> {
    /// The format of the HDR colour target.
    pub color_format: wgpu::TextureFormat,
    /// The format of the depth attachment.
    pub depth_format: wgpu::TextureFormat,
    /// The layout of group 0, the scene's frame bind group.
    pub frame_layout: &'a wgpu::BindGroupLayout,
}

/// A renderer extension that records its own GPU work in the frame. See
/// the [module documentation](self).
pub trait GpuDraw: Send + Sync {
    /// Records compute work before the frame's render passes. The default
    /// does nothing.
    fn prepare(&self, _ctx: &mut PrepareContext<'_>) {}

    /// Records the draw into the 2D pass. The default draws nothing.
    fn draw_2d(&self, _pass: &mut wgpu::RenderPass<'_>, _ctx: &Draw2D<'_>) {}

    /// Records the draw into a 3D scene's HDR pass. The default draws
    /// nothing.
    fn draw_3d(&self, _pass: &mut wgpu::RenderPass<'_>, _ctx: &Draw3D<'_>) {}
}

/// A queued custom draw in the frame's tables.
#[derive(Clone)]
pub(crate) struct CustomEntry(pub(crate) Arc<dyn GpuDraw>);

impl fmt::Debug for CustomEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CustomEntry")
    }
}

impl Graphics {
    /// What the adapter can do beyond the baseline: compute shaders,
    /// indirect draws and vertex-stage storage buffers, which renderer
    /// extensions check before choosing a GPU path.
    pub fn downlevel_capabilities(&self) -> wgpu::DownlevelCapabilities {
        self.shared.gpu.adapter.get_downlevel_capabilities()
    }

    /// The renderer extension resource of type `T` for this renderer,
    /// built with `make` the first time and shared after. An extension
    /// keeps its pipelines and layouts here so every effect it makes
    /// reuses them, and they live exactly as long as the renderer.
    pub fn extension<T: Any + Send + Sync>(&self, make: impl FnOnce(&wgpu::Device) -> T) -> Arc<T> {
        let mut map = self.extensions.lock();
        let id = TypeId::of::<T>();
        if let Some(found) = map
            .get(&id)
            .and_then(|e| Arc::clone(e).downcast::<T>().ok())
        {
            return found;
        }
        let made = Arc::new(make(self.device()));
        map.insert(id, made.clone());
        made
    }

    /// Queues `draw` into the 2D pass at the current layer and sort key,
    /// with the current camera and clip. The transform stack and blend
    /// mode do not apply: the extension chooses its own blending.
    pub fn draw_gpu(&mut self, draw: Arc<dyn GpuDraw>) {
        let view = self.view_slot();
        let sc = self.tables.views[view as usize].scissor;
        if sc[2] == 0 || sc[3] == 0 {
            return;
        }
        let slot = self.tables.push_custom(draw);
        let state = DrawState {
            texture: slot,
            shader: CUSTOM_SHADER,
            blend: BlendMode::Alpha,
            view,
        };
        let (layer, key) = (self.cursor.layer, self.cursor.sort_key);
        let i = self.current_index();
        self.lists[i].push_marker(state, layer, key);
    }
}
