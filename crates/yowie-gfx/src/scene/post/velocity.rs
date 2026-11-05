//! The velocity pass: the meshes a scene drew with
//! [`Scene::draw_moved`](crate::scene::Scene::draw_moved)
//! render again with this frame's and last frame's placement, writing
//! their screen motion for temporal anti-aliasing and motion blur.
//! Skinned and morphed draws are left out and move with the camera alone.

use bytemuck::{Pod, Zeroable};

use super::PostRenderer;
use super::chain::{Input, cached, mix, tex};
use super::pipelines::Program;
use super::targets::{HDR, Kind, PostTargets};
use crate::pipelines::trusted_module;
use crate::scene::record::SceneRecord;
use crate::scene::vertex::{GpuVertex, affine_rows};

const SHADER: &str = include_str!("shaders/velocity.wgsl");
const PRELUDE: &str = include_str!("shaders/prelude.wgsl");

/// Bind group cache tag of the velocity pass.
const TAG: u64 = 50;

/// One moved draw: this frame's and last frame's model rows.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MovedInstance {
    rows: [[f32; 4]; 3],
    previous: [[f32; 4]; 3],
}

const MOVED_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    1 => Float32x4,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
];

/// The velocity pipeline, built the first time a scene needs it.
#[derive(Debug)]
pub(crate) struct VelocityPipeline {
    pipeline: wgpu::RenderPipeline,
}

impl VelocityPipeline {
    fn new(device: &wgpu::Device, r: &PostRenderer) -> Self {
        let module = trusted_module(device, "yowie velocity", &[PRELUDE, SHADER].join("\n"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie velocity"),
            bind_group_layouts: &[
                Some(&r.pipes.common),
                Some(r.pipes.group(Program::DepthMip)),
            ],
            immediate_size: 0,
        });
        let position = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        }];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yowie velocity"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_velocity"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GpuVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &position,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MovedInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &MOVED_ATTRIBUTES,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_velocity"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }
}

impl PostRenderer {
    /// Records the velocity pass of `rec`'s moved draws into the
    /// velocity image, testing against the chain's `depth`. Returns
    /// whether anything was drawn (otherwise the image is not touched and
    /// the chain binds a stand-in).
    pub(crate) fn velocity(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        t: &mut PostTargets,
        rec: &SceneRecord,
        depth: Input<'_>,
    ) -> bool {
        let mut instances = Vec::with_capacity(rec.moved.len());
        let mut meshes = Vec::with_capacity(rec.moved.len());
        for (draw, previous) in &rec.moved {
            let Some(d) = rec.draws.get(*draw as usize) else {
                continue;
            };
            if d.joints != u32::MAX || d.morph_count > 0 {
                continue;
            }
            instances.push(MovedInstance {
                rows: affine_rows(&d.model),
                previous: affine_rows(previous),
            });
            meshes.push(d.mesh);
        }
        if instances.is_empty() {
            return false;
        }
        if self.velocity.is_none() {
            self.velocity = Some(VelocityPipeline::new(device, self));
        }
        let bytes: &[u8] = bytemuck::cast_slice(&instances);
        let fits = t
            .velocity_instances
            .as_ref()
            .is_some_and(|b| b.size() >= bytes.len() as u64);
        if !fits {
            t.velocity_instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie velocity instances"),
                size: (bytes.len() as u64).next_power_of_two().max(256),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(buffer) = t.velocity_instances.clone() else {
            return false;
        };
        queue.write_buffer(&buffer, 0, bytes);
        let (target, _) = t.view(device, Kind::Velocity);
        let layout = self.pipes.group(Program::DepthMip).clone();
        let group = cached(t, [TAG, mix(&[depth.id]), 0, 0], |_| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie velocity"),
                layout: &layout,
                entries: &[tex(0, depth.view)],
            })
        });
        let Some(v) = &self.velocity else {
            return false;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yowie velocity"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&v.pipeline);
        pass.set_bind_group(0, &t.common, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.set_vertex_buffer(1, buffer.slice(..));
        let mut bound = None;
        let mut count = 0;
        for (k, mesh) in meshes.iter().enumerate() {
            if bound != Some(*mesh) {
                let gpu = rec.meshes[*mesh as usize].inner.gpu.lock();
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                count = gpu.index_count;
                bound = Some(*mesh);
            }
            pass.draw_indexed(0..count, 0, k as u32..k as u32 + 1);
        }
        true
    }
}
