//! Auto exposure: a luminance histogram of the picture and the adapted
//! exposure, two compute passes over a state buffer each scene keeps.
//! Devices without compute shaders keep the manual exposure.

use super::PostRenderer;
use super::chain::{Input, cached, mix, tex};
use super::targets::PostTargets;
use crate::pipelines::trusted_module;

const SHADER: &str = include_str!("shaders/exposure.wgsl");
const PRELUDE: &str = include_str!("shaders/prelude.wgsl");

/// Bind group cache tag of the exposure passes.
const TAG: u64 = 51;
/// Bytes of the state: four floats, then 64 bins.
const STATE_SIZE: u64 = 16 + 64 * 4;

/// The two compute pipelines and their group-1 layout.
#[derive(Debug)]
pub(crate) struct ExposurePipelines {
    group: wgpu::BindGroupLayout,
    histogram: wgpu::ComputePipeline,
    adapt: wgpu::ComputePipeline,
}

impl ExposurePipelines {
    fn new(device: &wgpu::Device, common: &wgpu::BindGroupLayout) -> Self {
        let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie exposure"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(STATE_SIZE),
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie exposure"),
            bind_group_layouts: &[Some(common), Some(&group)],
            immediate_size: 0,
        });
        let module = trusted_module(device, "yowie exposure", &[PRELUDE, SHADER].join("\n"));
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("yowie exposure"),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            histogram: pipeline("cs_histogram"),
            adapt: pipeline("cs_adapt"),
            group,
        }
    }
}

/// Whether the device runs the exposure passes: compute shaders with
/// 256-invocation workgroups and storage buffers.
pub(crate) fn supported(device: &wgpu::Device) -> bool {
    let l = device.limits();
    l.max_compute_invocations_per_workgroup >= 256
        && l.max_compute_workgroup_size_x >= 16
        && l.max_compute_workgroup_size_y >= 16
        && l.max_storage_buffers_per_shader_stage > 0
}

impl PostRenderer {
    /// Records the histogram and adaptation of `picture` into `t`'s
    /// exposure state, made the first time. Returns the state buffer and
    /// its id for the composite, or `None` where compute is missing.
    pub(crate) fn auto_exposure(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        picture: Input<'_>,
    ) -> Option<(wgpu::Buffer, u64)> {
        if !self.compute {
            return None;
        }
        if self.exposure.is_none() {
            self.exposure = Some(ExposurePipelines::new(device, &self.pipes.common));
        }
        let p = self.exposure.as_ref()?;
        let state = match &t.exposure {
            Some(s) => s.clone(),
            None => {
                let (buffer, id) = (
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("yowie exposure state"),
                        size: STATE_SIZE,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    crate::texture::next_id(),
                );
                // Exposure 1 until the first adaptation; the bins start
                // empty (new buffers are zeroed).
                queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0]));
                t.exposure = Some((buffer.clone(), id));
                (buffer, id)
            }
        };
        let layout = p.group.clone();
        let group = cached(t, [TAG, mix(&[picture.id, state.1]), 0, 0], |_| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie exposure"),
                layout: &layout,
                entries: &[tex(0, picture.view), super::chain::buf(1, &state.0)],
            })
        });
        let size = t.size;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("yowie exposure"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &t.common, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.set_pipeline(&p.histogram);
        pass.dispatch_workgroups(size.x.div_ceil(16), size.y.div_ceil(16), 1);
        pass.set_pipeline(&p.adapt);
        pass.dispatch_workgroups(1, 1, 1);
        Some(state)
    }
}
