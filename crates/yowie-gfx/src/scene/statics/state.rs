//! What the renderer keeps for static groups between frames: whether the
//! device can cull on the GPU, the compute pipelines, each group's GPU
//! data, and each slot's depth pyramid.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::gpu::GroupGpu;
use crate::scene::record::FastMap;
use crate::scene::slot::SlotId;
use crate::shared::Shared;

/// Frames a group's GPU data may go unused before it is dropped.
const KEEP_FRAMES: u64 = 120;

/// The bundles one view replays, and the latest counts it read back.
#[derive(Debug, Default)]
pub(crate) struct StaticFrame {
    /// Bundles of the items visible last frame.
    pub(crate) early: Vec<wgpu::RenderBundle>,
    /// Bundles of the items that became visible.
    pub(crate) late: Vec<wgpu::RenderBundle>,
    /// The groups culled, by id.
    pub(crate) groups: Vec<u64>,
    /// Indirect draws in the bundles.
    pub(crate) draws: u32,
    /// Items outside the view and items hidden, as last read back.
    pub(crate) culled: u32,
    pub(crate) occluded: u32,
}

/// A staging buffer the culling counters are copied into and read back
/// from a frame or two later.
#[derive(Debug)]
pub(crate) struct Readback {
    pub(crate) buffer: wgpu::Buffer,
    /// `FREE`, `COPIED`, `MAPPING` or `MAPPED`.
    pub(crate) state: Arc<AtomicU8>,
}

pub(crate) const FREE: u8 = 0;
pub(crate) const COPIED: u8 = 1;
pub(crate) const MAPPING: u8 = 2;
pub(crate) const MAPPED: u8 = 3;

impl Readback {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie cull counters"),
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
        }
    }

    /// Starts mapping a buffer whose copy was submitted.
    fn map_if_copied(&self) {
        if self.state.load(Ordering::Acquire) != COPIED {
            return;
        }
        self.state.store(MAPPING, Ordering::Release);
        let state = self.state.clone();
        self.buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            state.store(if r.is_ok() { MAPPED } else { FREE }, Ordering::Release);
        });
    }

    /// The counters of a mapped buffer, freeing it.
    pub(crate) fn take(&self) -> Option<[u32; 4]> {
        if self.state.load(Ordering::Acquire) != MAPPED {
            return None;
        }
        let counts = self.buffer.get_mapped_range(..).ok().map(|m| {
            let w: &[u32] = bytemuck::cast_slice(&m);
            [w[0], w[1], w[2], w[3]]
        });
        self.buffer.unmap();
        self.state.store(FREE, Ordering::Release);
        counts
    }
}

/// The compute pipelines of the GPU path.
#[derive(Debug)]
pub(crate) struct CullPipelines {
    pub(crate) cull_layout: wgpu::BindGroupLayout,
    pub(crate) first_layout: wgpu::BindGroupLayout,
    pub(crate) down_layout: wgpu::BindGroupLayout,
    pub(crate) early: wgpu::ComputePipeline,
    pub(crate) late: wgpu::ComputePipeline,
    pub(crate) first: wgpu::ComputePipeline,
    pub(crate) down: wgpu::ComputePipeline,
}

/// The depth pyramid of one slot and the bind groups that build it.
#[derive(Debug)]
pub(crate) struct Pyramid {
    /// The depth view it reduces and that view's size.
    pub(crate) depth: wgpu::TextureView,
    pub(crate) size: (u32, u32),
    /// Every mip, for culling.
    pub(crate) view: wgpu::TextureView,
    pub(crate) mips: u32,
    /// Bind groups for level 0 from depth, then each level from the one
    /// below, with each level's size.
    pub(crate) steps: Vec<(wgpu::BindGroup, (u32, u32))>,
}

/// Static group state of a scene renderer.
#[derive(Debug)]
pub(crate) struct StaticState {
    gpu: bool,
    pub(crate) pipelines: Option<CullPipelines>,
    pub(crate) groups: FastMap<u64, GroupGpu>,
    pub(crate) pyramids: FastMap<SlotId, Pyramid>,
    pub(crate) frame: u64,
    /// The last frame the device was polled for counter maps.
    pub(crate) polled: u64,
}

impl StaticState {
    pub(crate) fn new(shared: &Shared) -> Self {
        let caps = shared.gpu.adapter.get_downlevel_capabilities();
        let limits = shared.device().limits();
        let gpu = caps.flags.contains(
            wgpu::DownlevelFlags::COMPUTE_SHADERS | wgpu::DownlevelFlags::INDIRECT_EXECUTION,
        ) && limits.max_storage_buffers_per_shader_stage >= 4
            && limits.max_storage_textures_per_shader_stage >= 1;
        if !gpu {
            log::info!(
                "yowie-gfx: no compute shaders or indirect draws; static groups draw without GPU culling"
            );
        }
        Self {
            gpu,
            pipelines: None,
            groups: FastMap::default(),
            pyramids: FastMap::default(),
            frame: 0,
            polled: u64::MAX,
        }
    }

    /// Whether static groups cull on the GPU and draw from bundles.
    pub(crate) fn gpu_culling(&self) -> bool {
        self.gpu
    }

    /// Turns GPU culling off, as on a device without it.
    #[cfg(test)]
    pub(crate) fn disable_gpu_culling(&mut self) {
        self.gpu = false;
    }

    /// Starts mapping last frame's counter copies (their submission is
    /// done) and drops groups unused for a while.
    pub(crate) fn begin_frame(&mut self, frame: u64) {
        self.frame = frame;
        for g in self.groups.values() {
            for v in g.views.values() {
                for r in &v.readbacks {
                    r.map_if_copied();
                }
            }
        }
        self.groups
            .retain(|_, g| frame.saturating_sub(g.used) <= KEEP_FRAMES);
    }
}
