//! The GPU buffers and targets one scene of a frame renders with. Scenes
//! take slots in call order and keep them across frames, so buffers only
//! grow.

use yowie_core::UVec2;

use super::frame_data::FrameUniform;
use super::layouts::{DEPTH_FORMAT, HDR_FORMAT};

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

/// The HDR colour and depth images of one scene.
#[derive(Debug)]
pub(crate) struct Targets {
    pub(crate) size: UVec2,
    pub(crate) color: wgpu::TextureView,
    pub(crate) depth: wgpu::TextureView,
}

impl Targets {
    fn new(device: &wgpu::Device, size: UVec2) -> Self {
        let make = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            size,
            color: make(HDR_FORMAT, "yowie scene colour"),
            depth: make(DEPTH_FORMAT, "yowie scene depth"),
        }
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
    pub(crate) resolve: wgpu::Buffer,
    pub(crate) targets: Option<Targets>,
    /// The frame and shadow bind groups and what they were built from.
    pub(crate) groups: Option<(GroupKey, wgpu::BindGroup, wgpu::BindGroup)>,
    pub(crate) resolve_group: Option<wgpu::BindGroup>,
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
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie resolve"),
                size: 32,
                usage: U::UNIFORM | U::COPY_DST,
                mapped_at_creation: false,
            }),
            targets: None,
            groups: None,
            resolve_group: None,
        }
    }

    /// The targets at `size`, recreated when the size changed. Returns
    /// whether they were recreated.
    pub(crate) fn ensure_targets(&mut self, device: &wgpu::Device, size: UVec2) -> bool {
        let size = size.max(UVec2::ONE);
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return false;
        }
        self.targets = Some(Targets::new(device, size));
        self.resolve_group = None;
        true
    }
}
