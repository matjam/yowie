//! The buffers of one GPU particle system and the recording of its
//! simulation steps.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use yowie_gfx::BlendMode;

use super::layout::{DrawBlock, GpuParticle, STEP_STRIDE, StepParams};
use super::programs::Programs;

/// Most simulation steps recorded per frame; more are merged.
pub(crate) const MAX_STEPS: usize = 64;
/// Most particles one system holds: the dispatch size limit times the
/// workgroup size.
pub(crate) const MAX_CAPACITY: u32 = 65535 * 64;
const WORKGROUP: u32 = 64;

/// Where the asynchronous live count read stands.
#[derive(Debug, Default)]
struct ReadShared {
    /// 0 idle, 1 copy recorded, 2 mapping.
    state: AtomicU8,
    /// The prepare count the pending copy belongs to.
    epoch: AtomicU64,
    /// The last finished read: prepare count and live count.
    value: Mutex<Option<(u64, u32)>>,
}

/// One system's GPU state. Owned by the shared inner object behind a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) programs: Arc<Programs>,
    pub(crate) capacity: u32,
    pub(crate) particles: wgpu::Buffer,
    slots: wgpu::Buffer,
    pub(crate) header: wgpu::Buffer,
    births: wgpu::Buffer,
    births_len: usize,
    params: wgpu::Buffer,
    draw: wgpu::Buffer,
    pub(crate) block: DrawBlock,
    block_dirty: bool,
    sim_group: wgpu::BindGroup,
    pub(crate) data_group: wgpu::BindGroup,
    white: wgpu::TextureView,
    view: Option<wgpu::TextureView>,
    sampler: wgpu::Sampler,
    staging: wgpu::Buffer,
    read: Arc<ReadShared>,
    /// Steps and births waiting for the next frame.
    pub(crate) steps: Vec<StepParams>,
    pub(crate) births_cpu: Vec<GpuParticle>,
    clear_pending: bool,
    pub(crate) blend: BlendMode,
    prepares: u64,
    last_birth: u64,
    queued_births: bool,
    pub(crate) alive_seen: u32,
    read_epoch: u64,
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(16),
        usage,
        mapped_at_creation: false,
    })
}

impl Core {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        programs: Arc<Programs>,
        capacity: u32,
    ) -> Self {
        let capacity = capacity.clamp(1, MAX_CAPACITY);
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let particles = buffer(
            device,
            "yowie particles",
            u64::from(capacity) * 48,
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let slots = buffer(
            device,
            "yowie particle slots",
            u64::from(capacity) * 8,
            storage,
        );
        let header = buffer(
            device,
            "yowie particle header",
            32,
            storage | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC,
        );
        let births = buffer(device, "yowie particle births", 48 * 256, storage);
        let params = buffer(
            device,
            "yowie particle steps",
            STEP_STRIDE * MAX_STEPS as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let draw = buffer(
            device,
            "yowie particle tables",
            std::mem::size_of::<DrawBlock>() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let staging = buffer(
            device,
            "yowie particle count",
            16,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie particle white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            tex.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let white = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let block = DrawBlock::new(capacity);
        let sim_group = Self::make_sim_group(
            device, &programs, &params, &particles, &births, &slots, &header, &draw,
        );
        let data_group = Self::make_data_group(
            device, &programs, &draw, &particles, &slots, &header, &white, &sampler,
        );
        let mut core = Self {
            device: device.clone(),
            queue: queue.clone(),
            programs,
            capacity,
            particles,
            slots,
            header,
            births,
            births_len: 256,
            params,
            draw,
            block,
            block_dirty: true,
            sim_group,
            data_group,
            white,
            view: None,
            sampler,
            staging,
            read: Arc::new(ReadShared::default()),
            steps: Vec::new(),
            births_cpu: Vec::new(),
            clear_pending: false,
            blend: BlendMode::Alpha,
            prepares: 0,
            last_birth: 0,
            queued_births: false,
            alive_seen: 0,
            read_epoch: 0,
        };
        core.reset_pool();
        core
    }

    #[allow(clippy::too_many_arguments)]
    fn make_sim_group(
        device: &wgpu::Device,
        programs: &Programs,
        params: &wgpu::Buffer,
        particles: &wgpu::Buffer,
        births: &wgpu::Buffer,
        slots: &wgpu::Buffer,
        header: &wgpu::Buffer,
        draw: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let size = wgpu::BufferSize::new(std::mem::size_of::<StepParams>() as u64);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie particle sim"),
            layout: &programs.sim_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: params,
                        offset: 0,
                        size,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: particles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: births.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: slots.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: header.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: draw.as_entire_binding(),
                },
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn make_data_group(
        device: &wgpu::Device,
        programs: &Programs,
        draw: &wgpu::Buffer,
        particles: &wgpu::Buffer,
        slots: &wgpu::Buffer,
        header: &wgpu::Buffer,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie particle draw"),
            layout: &programs.data_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: draw.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: particles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: slots.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: header.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// Writes the free-slot stack and the indirect arguments for an empty
    /// pool.
    fn reset_pool(&mut self) {
        let cap = self.capacity as usize;
        let mut slots = vec![0u32; cap * 2];
        for (i, s) in slots.iter_mut().take(cap).enumerate() {
            *s = i as u32;
        }
        self.queue
            .write_buffer(&self.slots, 0, bytemuck::cast_slice(&slots));
        let header = [6u32, 0, 0, 0, self.capacity, 0, 0, 0];
        self.queue
            .write_buffer(&self.header, 0, bytemuck::cast_slice(&header));
        self.alive_seen = 0;
        self.clear_pending = true;
    }

    /// Kills every particle and drops births not yet recorded.
    pub(crate) fn clear(&mut self) {
        self.births_cpu.clear();
        for s in &mut self.steps {
            s.birth_count = 0;
        }
        self.reset_pool();
    }

    /// Replaces the pool with one of `capacity` particles; live particles
    /// are lost.
    pub(crate) fn resize(&mut self, capacity: u32) {
        let (device, queue, programs) = (
            self.device.clone(),
            self.queue.clone(),
            self.programs.clone(),
        );
        let mut fresh = Core::new(&device, &queue, programs, capacity);
        fresh.block = DrawBlock {
            capacity: fresh.capacity,
            ..self.block
        };
        fresh.block_dirty = true;
        fresh.blend = self.blend;
        fresh.steps = std::mem::take(&mut self.steps);
        fresh.births_cpu = std::mem::take(&mut self.births_cpu);
        for s in &mut fresh.steps {
            s.capacity = fresh.capacity;
        }
        fresh.queued_births = self.queued_births;
        if let Some(v) = self.view.take() {
            fresh.set_view(Some(v), None);
        }
        *self = fresh;
    }

    /// Binds a texture view (or the white texel for `None`), sampled
    /// smoothly when `smooth`, with nearest filtering otherwise.
    pub(crate) fn set_view(&mut self, view: Option<wgpu::TextureView>, smooth: Option<bool>) {
        if let Some(smooth) = smooth {
            let f = if smooth {
                wgpu::FilterMode::Linear
            } else {
                wgpu::FilterMode::Nearest
            };
            self.sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: f,
                min_filter: f,
                ..Default::default()
            });
        }
        self.view = view;
        let bound = self.view.as_ref().unwrap_or(&self.white);
        self.data_group = Self::make_data_group(
            &self.device,
            &self.programs,
            &self.draw,
            &self.particles,
            &self.slots,
            &self.header,
            bound,
            &self.sampler,
        );
    }

    /// Marks the draw block as changed so the next frame uploads it.
    pub(crate) fn touch_block(&mut self) {
        self.block_dirty = true;
    }

    /// Queues one step; `births` are appended to the pending births.
    pub(crate) fn queue_step(&mut self, mut step: StepParams, births: &[GpuParticle]) {
        step.birth_first = self.births_cpu.len() as u32;
        let n = births.len().min(self.capacity as usize);
        step.birth_count = n as u32;
        self.births_cpu.extend_from_slice(&births[..n]);
        if n > 0 {
            self.queued_births = true;
        }
        self.steps.push(step);
    }

    /// Whether nothing is alive, as far as the last finished read says,
    /// and nothing waits to be born.
    pub(crate) fn settled(&self) -> bool {
        !self.queued_births && self.read_epoch >= self.last_birth && self.alive_seen == 0
    }

    /// Merges steps beyond [`MAX_STEPS`] into longer ones.
    fn merged_steps(&self) -> Vec<StepParams> {
        let group = self.steps.len().div_ceil(MAX_STEPS).max(1);
        self.steps
            .chunks(group)
            .map(|chunk| {
                let mut s = chunk[0];
                s.dt = chunk.iter().map(|c| c.dt).sum();
                let total: u32 = chunk.iter().map(|c| c.birth_count).sum();
                s.birth_count = total.min(self.capacity);
                s
            })
            .collect()
    }

    /// Records the queued steps as compute work on `encoder`.
    pub(crate) fn record(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.block_dirty {
            self.queue
                .write_buffer(&self.draw, 0, bytemuck::bytes_of(&self.block));
            self.block_dirty = false;
        }
        if self.steps.is_empty() {
            return;
        }
        self.prepares += 1;
        if self.clear_pending {
            encoder.clear_buffer(&self.particles, 0, None);
            self.clear_pending = false;
        }
        let steps = self.merged_steps();
        self.upload_births();
        let mut bytes = vec![0u8; steps.len() * STEP_STRIDE as usize];
        for (i, s) in steps.iter().enumerate() {
            let at = i * STEP_STRIDE as usize;
            let src = bytemuck::bytes_of(s);
            bytes[at..at + src.len()].copy_from_slice(src);
        }
        self.queue.write_buffer(&self.params, 0, &bytes);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("yowie particles"),
                timestamp_writes: None,
            });
            let p = &self.programs;
            for (i, s) in steps.iter().enumerate() {
                pass.set_bind_group(0, &self.sim_group, &[(i as u64 * STEP_STRIDE) as u32]);
                if s.birth_count > 0 {
                    pass.set_pipeline(&p.emit);
                    pass.dispatch_workgroups(s.birth_count.div_ceil(WORKGROUP), 1, 1);
                }
                pass.set_pipeline(&p.update);
                pass.dispatch_workgroups(self.capacity.div_ceil(WORKGROUP), 1, 1);
                pass.set_pipeline(&p.finalize);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }
        self.steps.clear();
        self.births_cpu.clear();
        if self.queued_births {
            self.last_birth = self.prepares;
            self.queued_births = false;
        }
        self.advance_read(encoder);
    }

    fn upload_births(&mut self) {
        let n = self.births_cpu.len().max(1);
        if n > self.births_len {
            let len = n.next_power_of_two();
            self.births = buffer(
                &self.device,
                "yowie particle births",
                len as u64 * 48,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            self.births_len = len;
            self.sim_group = Self::make_sim_group(
                &self.device,
                &self.programs,
                &self.params,
                &self.particles,
                &self.births,
                &self.slots,
                &self.header,
                &self.draw,
            );
        }
        if !self.births_cpu.is_empty() {
            self.queue
                .write_buffer(&self.births, 0, bytemuck::cast_slice(&self.births_cpu));
        }
    }

    /// Steps the asynchronous read of the live count: record a copy, then
    /// on a later frame map it. The count lags the simulation by a few
    /// frames.
    fn advance_read(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let _ = self.device.poll(wgpu::PollType::Poll);
        if let Some((epoch, count)) = self
            .read
            .value
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            self.alive_seen = count;
            self.read_epoch = epoch;
        }
        match self.read.state.load(Ordering::Acquire) {
            0 => {
                encoder.copy_buffer_to_buffer(&self.header, 4, &self.staging, 0, 4);
                self.read.epoch.store(self.prepares, Ordering::Release);
                self.read.state.store(1, Ordering::Release);
            }
            1 => {
                self.read.state.store(2, Ordering::Release);
                let shared = self.read.clone();
                let staging = self.staging.clone();
                self.staging
                    .slice(..4)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let mapped = result.is_ok();
                        let count = mapped.then_some(()).and_then(|()| {
                            let view = staging.slice(..4).get_mapped_range().ok()?;
                            let bytes = view.first_chunk::<4>().copied();
                            drop(view);
                            bytes.map(u32::from_le_bytes)
                        });
                        if mapped {
                            staging.unmap();
                        }
                        if let Some(count) = count {
                            let epoch = shared.epoch.load(Ordering::Acquire);
                            *shared.value.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some((epoch, count));
                        }
                        shared.state.store(0, Ordering::Release);
                    });
            }
            _ => {}
        }
    }

    /// Reads `size` bytes of `src` at `offset`, waiting for the GPU. For
    /// tests and tools; it stalls the frame.
    pub(crate) fn read_blocking(&self, src: &wgpu::Buffer, offset: u64, size: u64) -> Vec<u8> {
        let staging = buffer(
            &self.device,
            "yowie particle readback",
            size,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(src, offset, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging
            .slice(..size)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        match rx.recv() {
            Ok(Ok(())) => {
                let data = staging
                    .slice(..size)
                    .get_mapped_range()
                    .map(|v| v.to_vec())
                    .unwrap_or_default();
                staging.unmap();
                data
            }
            _ => Vec::new(),
        }
    }
}
