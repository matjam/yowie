//! The GPU buffers and targets one scene of a frame renders with. Scenes
//! take slots in call order and keep them across frames, so buffers only
//! grow.

use yowie_core::{Mat4, UVec2, Vec2, Vec3};

use super::frame_data::FrameUniform;
use super::post::{PostCamera, PostTargets};
use super::prepare::View;
use super::targets::{SceneFormat, Targets};

/// A buffer that grows to fit what is written to it.
#[derive(Debug)]
pub(crate) struct GrowBuffer {
    pub(crate) buffer: wgpu::Buffer,
    label: &'static str,
}

impl GrowBuffer {
    pub(crate) fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
    ) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 256,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            label,
        }
    }

    /// Writes `data` from the start, replacing the buffer with a larger one
    /// first when it does not fit. Returns whether it was replaced.
    pub(crate) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[u8],
    ) -> bool {
        let mut grew = false;
        if data.len() as u64 > self.buffer.size() {
            let size = (data.len() as u64).next_power_of_two().max(256);
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size,
                usage: self.buffer.usage(),
                mapped_at_creation: false,
            });
            grew = true;
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buffer, 0, data);
        }
        grew
    }
}

/// Everything one scene slot owns on the GPU.
#[derive(Debug)]
pub(crate) struct SceneSlot {
    pub(crate) frame: wgpu::Buffer,
    pub(crate) lights: GrowBuffer,
    pub(crate) clusters: GrowBuffer,
    pub(crate) indices: GrowBuffer,
    pub(crate) materials: GrowBuffer,
    pub(crate) joints: GrowBuffer,
    pub(crate) morph_weights: GrowBuffer,
    pub(crate) instances: GrowBuffer,
    pub(crate) lines: GrowBuffer,
    pub(crate) targets: Option<Targets>,
    /// The frame and shadow bind groups and what they were built from.
    pub(crate) groups: Option<(GroupKey, wgpu::BindGroup, wgpu::BindGroup)>,
    /// The post chain's images and block, made on the first frame.
    pub(crate) post: Option<PostTargets>,
    /// The camera this frame rendered with, for the post chain.
    pub(crate) camera: PostCamera,
    /// Last frame's steady view-projection, for reprojection; `None`
    /// until a frame has rendered or after a cut.
    pub(crate) previous: Option<Mat4>,
    /// Position in the jitter sequence.
    jitter_index: u32,
}

/// Element `i` (from 1) of the Halton sequence in `base`, in 0..1.
fn halton(mut i: u32, base: u32) -> f32 {
    let mut f = 1.0;
    let mut r = 0.0;
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// What a slot's bind groups name besides its own buffers: the atlas and
/// environment ids, and a counter bumped when a buffer is replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GroupKey {
    pub(crate) atlas: u64,
    pub(crate) env: u64,
    pub(crate) buffers: u64,
}

impl SceneSlot {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        use wgpu::BufferUsages as U;
        Self {
            frame: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie scene frame"),
                size: std::mem::size_of::<FrameUniform>() as u64,
                usage: U::UNIFORM | U::COPY_DST,
                mapped_at_creation: false,
            }),
            lights: GrowBuffer::new(device, "yowie lights", U::STORAGE),
            clusters: GrowBuffer::new(device, "yowie clusters", U::STORAGE),
            indices: GrowBuffer::new(device, "yowie light indices", U::STORAGE),
            materials: GrowBuffer::new(device, "yowie materials", U::STORAGE),
            joints: GrowBuffer::new(device, "yowie joints", U::STORAGE),
            morph_weights: GrowBuffer::new(device, "yowie morph weights", U::STORAGE),
            instances: GrowBuffer::new(device, "yowie instances", U::VERTEX),
            lines: GrowBuffer::new(device, "yowie lines", U::VERTEX),
            targets: None,
            groups: None,
            post: None,
            camera: PostCamera::default(),
            previous: None,
            jitter_index: 0,
        }
    }

    /// Keeps the camera this frame renders with for the post chain and
    /// returns the view to rasterise with: `steady` itself, or with
    /// `jitter` a sub-pixel offset of it that changes every frame (the
    /// Halton 2, 3 sequence over eight frames) for temporal
    /// anti-aliasing. A cut forgets the previous frame's camera.
    pub(crate) fn set_camera(&mut self, steady: &View, cut: bool, jitter: bool) -> View {
        let mut v = steady.clone();
        let mut offset = Vec2::ZERO;
        if jitter {
            self.jitter_index = (self.jitter_index + 1) % 8;
            let i = self.jitter_index + 1;
            offset = Vec2::new(halton(i, 2), halton(i, 3)) - 0.5;
            let size = steady.size.max(Vec2::ONE);
            let shift = Vec3::new(2.0 * offset.x / size.x, -2.0 * offset.y / size.y, 0.0);
            v.proj = Mat4::from_translation(shift) * steady.proj;
            v.view_proj = v.proj * v.view;
        }
        self.camera = PostCamera {
            view: v.view,
            proj: v.proj,
            view_proj: v.view_proj,
            steady_view_proj: steady.view_proj,
            jitter: offset,
        };
        if cut {
            self.previous = None;
        }
        v
    }

    /// The targets at `size` in `format`, recreated when either changed.
    /// Returns whether they were recreated.
    pub(crate) fn ensure_targets(
        &mut self,
        device: &wgpu::Device,
        size: UVec2,
        format: SceneFormat,
    ) -> bool {
        let size = size.max(UVec2::ONE);
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.size == size && t.format == format)
        {
            return false;
        }
        self.targets = Some(Targets::new(device, size, format));
        if let Some(post) = &mut self.post {
            // Bind groups naming the old images go with them.
            post.groups.clear();
        }
        true
    }

    /// The attachment layout of the slot's HDR pass.
    pub(crate) fn format(&self) -> SceneFormat {
        self.targets
            .as_ref()
            .map_or_else(SceneFormat::default, |t| t.format)
    }
}
