//! The GPU side of 2D lights: the lights block, the shadow strip and the
//! lit sprite pipeline.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use yowie_core::Vec2;

use super::shadow::{ANGLES, CLEAR, Occluders, fill_row};
use super::types::{Light2D, MAX_LIGHTS_2D};
use super::{LitRun, LitState};
use crate::pipelines::{VERTEX_ATTRIBUTES, Vertex2D, trusted_module};
use crate::shared::Shared;

/// One light as the shader reads it; matches `Light` in `lit.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct LightGpu {
    pos: [f32; 4],
    color: [f32; 4],
    spot: [f32; 4],
    shadow: [f32; 4],
}

/// The lights block; matches `Lights` in `lit.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct LightsGpu {
    ambient: [f32; 4],
    lights: [LightGpu; MAX_LIGHTS_2D],
}

impl LightGpu {
    fn new(l: &Light2D, row: usize) -> Self {
        let c = l.color.to_linear();
        let k = l.intensity.max(0.0);
        let spot = match l.spot {
            Some(s) => {
                let half = (s.angle * 0.5).clamp(0.001, std::f32::consts::PI);
                let dir = Vec2::from_angle(s.direction);
                [
                    dir.x,
                    dir.y,
                    half.cos(),
                    (half * (1.0 - s.softness.clamp(0.0, 1.0))).cos(),
                ]
            }
            None => [0.0, 0.0, 2.0, 2.0],
        };
        Self {
            pos: [l.position.x, l.position.y, l.height, l.radius.max(1e-3)],
            color: [c[0] * k, c[1] * k, c[2] * k, l.falloff.max(0.01)],
            spot,
            shadow: [
                if l.casts_shadows { 1.0 } else { 0.0 },
                row as f32,
                l.softness.max(0.0),
                0.0,
            ],
        }
    }
}

/// What the shadow strip was last built from.
#[derive(Debug, PartialEq)]
struct ShadowKey {
    lights: Vec<(Vec2, f32)>,
    occluders: Occluders,
}

/// Pipelines, buffers and bind group of the lit sprite pass.
#[derive(Debug)]
pub(crate) struct LitRenderer {
    module: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    uniform: wgpu::Buffer,
    strip: wgpu::Texture,
    group: wgpu::BindGroup,
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    rows: Vec<f32>,
    built: Option<ShadowKey>,
}

fn grow(device: &wgpu::Device, buf: &mut wgpu::Buffer, bytes: u64, usage: wgpu::BufferUsages) {
    if bytes > buf.size() {
        *buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie lit geometry"),
            size: bytes.next_power_of_two(),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }
}

impl LitRenderer {
    pub(crate) fn new(shared: &Shared) -> Self {
        let device = shared.device();
        let module = trusted_module(device, "yowie lit", include_str!("../shaders/lit.wgsl"));
        let layout_group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie lights"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let l = &shared.layouts;
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie lit"),
            bind_group_layouts: &[
                Some(&l.texture),
                Some(&l.view),
                Some(&layout_group),
                Some(&l.texture),
            ],
            immediate_size: 0,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie lights"),
            size: std::mem::size_of::<LightsGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let strip = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie shadow strip"),
            size: wgpu::Extent3d {
                width: ANGLES as u32,
                height: MAX_LIGHTS_2D as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = strip.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie lights"),
            layout: &layout_group,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let geometry = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yowie lit geometry"),
                size: 4096,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            module,
            layout,
            pipelines: HashMap::new(),
            uniform,
            strip,
            group,
            vertex: geometry(wgpu::BufferUsages::VERTEX),
            index: geometry(wgpu::BufferUsages::INDEX),
            rows: vec![CLEAR; ANGLES * MAX_LIGHTS_2D],
            built: None,
        }
    }

    /// Writes the frame's lights, shadow maps and geometry to the GPU.
    pub(crate) fn upload(&mut self, shared: &Shared, state: &LitState) {
        let (device, queue) = (shared.device(), shared.queue());
        let mut block = LightsGpu::default();
        let a = state.ambient.to_linear();
        block.ambient = [a[0], a[1], a[2], state.lights.len() as f32];
        for (i, l) in state.lights.iter().enumerate() {
            block.lights[i] = LightGpu::new(l, i);
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&block));

        let key = ShadowKey {
            lights: state
                .lights
                .iter()
                .map(|l| {
                    let r = if l.casts_shadows { l.radius } else { -1.0 };
                    (l.position, r)
                })
                .collect(),
            occluders: state.occluders.clone(),
        };
        if self.built.as_ref() != Some(&key) {
            for (i, l) in state.lights.iter().enumerate() {
                if l.casts_shadows {
                    let row = &mut self.rows[i * ANGLES..(i + 1) * ANGLES];
                    fill_row(row, l.position, l.radius, &state.occluders);
                }
            }
            queue.write_texture(
                self.strip.as_image_copy(),
                bytemuck::cast_slice(&self.rows),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some((ANGLES * 4) as u32),
                    rows_per_image: Some(MAX_LIGHTS_2D as u32),
                },
                wgpu::Extent3d {
                    width: ANGLES as u32,
                    height: MAX_LIGHTS_2D as u32,
                    depth_or_array_layers: 1,
                },
            );
            self.built = Some(key);
        }

        let vbytes: &[u8] = bytemuck::cast_slice(&state.vertices);
        let ibytes: &[u8] = bytemuck::cast_slice(&state.indices);
        grow(
            device,
            &mut self.vertex,
            vbytes.len() as u64,
            wgpu::BufferUsages::VERTEX,
        );
        grow(
            device,
            &mut self.index,
            ibytes.len() as u64,
            wgpu::BufferUsages::INDEX,
        );
        if !vbytes.is_empty() {
            queue.write_buffer(&self.vertex, 0, vbytes);
            queue.write_buffer(&self.index, 0, ibytes);
        }
    }

    /// Records one run of lit sprites into a pass whose view block is
    /// bound by the caller's `view_group` at `view_offset`.
    pub(crate) fn draw(
        &mut self,
        shared: &Shared,
        pass: &mut wgpu::RenderPass<'_>,
        run: &LitRun,
        view: (&wgpu::BindGroup, u32),
        format: wgpu::TextureFormat,
    ) {
        let pipeline = self.pipelines.entry(format).or_insert_with(|| {
            let module = &self.module;
            shared
                .device()
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("yowie lit"),
                    layout: Some(&self.layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex2D>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &VERTEX_ATTRIBUTES,
                        })],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(crate::BlendMode::Alpha.state()),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &run.albedo.gpu().bind_group, &[]);
        pass.set_bind_group(1, view.0, &[view.1]);
        pass.set_bind_group(2, &self.group, &[]);
        pass.set_bind_group(3, &run.normal.gpu().bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex.slice(..));
        pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(run.first..run.first + run.count, 0, 0..1);
    }
}
