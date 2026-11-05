//! Rendering a recorded frame: uploads, then each output's passes in order.
//!
//! [`Graphics::end_frame`] renders the render textures drawn to this frame
//! (in the order their `draw_to` scopes finished, so a nested target is
//! ready before the one that samples it), then the screen. Each output runs
//! its passes in a fixed order; see the crate documentation for where 3D
//! scene passes, post-processing, UI and the console fit.

use std::collections::HashMap;
use std::num::NonZeroU64;

use crate::Graphics;
use crate::batch::{DEFAULT_SHADER, DrawCall, DrawList, FrameTables, ListTarget};
use crate::error::Result;
use crate::graphics::FrameStats;
use crate::pipelines::{PipelineCache, PipelineKey, Vertex2D, sprite_pipeline};
use crate::readback;
use crate::shared::Shared;

/// The frame's vertex, index and uniform buffers, grown as needed.
#[derive(Debug)]
pub(crate) struct FrameBuffers {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    uniform: wgpu::Buffer,
    view_group: wgpu::BindGroup,
    user_groups: HashMap<u64, wgpu::BindGroup>,
    align: u64,
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn uniform_group(
    shared: &Shared,
    layout: &wgpu::BindGroupLayout,
    buf: &wgpu::Buffer,
    size: u64,
) -> wgpu::BindGroup {
    shared
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: buf,
                    offset: 0,
                    size: NonZeroU64::new(size),
                }),
            }],
        })
}

const VIEW_SIZE: u64 = std::mem::size_of::<crate::pipelines::ViewUniform>() as u64;

impl FrameBuffers {
    pub(crate) fn new(shared: &Shared) -> Self {
        let d = shared.device();
        let align = u64::from(d.limits().min_uniform_buffer_offset_alignment);
        let uniform = buffer(d, "yowie uniforms", 64 * 1024, wgpu::BufferUsages::UNIFORM);
        Self {
            vertex: buffer(d, "yowie vertices", 256 * 1024, wgpu::BufferUsages::VERTEX),
            index: buffer(d, "yowie indices", 128 * 1024, wgpu::BufferUsages::INDEX),
            view_group: uniform_group(shared, &shared.layouts.view, &uniform, VIEW_SIZE),
            uniform,
            user_groups: HashMap::new(),
            align,
        }
    }

    /// Writes `data` to `buf`, replacing it with a larger one first when it
    /// does not fit. Returns whether the buffer was replaced.
    fn upload(shared: &Shared, buf: &mut wgpu::Buffer, data: &[u8], label: &str) -> bool {
        let mut grew = false;
        if data.len() as u64 > buf.size() {
            let size = (data.len() as u64).next_power_of_two();
            *buf = buffer(
                shared.device(),
                label,
                size,
                buf.usage() & !wgpu::BufferUsages::COPY_DST,
            );
            grew = true;
        }
        if !data.is_empty() {
            shared.queue().write_buffer(buf, 0, data);
        }
        grew
    }

    fn user_group(&mut self, shared: &Shared, size: u64) -> &wgpu::BindGroup {
        let (uniform, layout) = (&self.uniform, &shared.layouts.user);
        self.user_groups
            .entry(size)
            .or_insert_with(|| uniform_group(shared, layout, uniform, size))
    }
}

/// Where each view and shader uniform block sits in the uniform buffer.
struct UniformLayout {
    bytes: Vec<u8>,
    views: Vec<u32>,
    shaders: Vec<u32>,
}

fn layout_uniforms(tables: &FrameTables, align: u64) -> UniformLayout {
    let mut bytes = Vec::new();
    let mut place = |data: &[u8]| {
        let offset = (bytes.len() as u64).div_ceil(align) * align;
        bytes.resize(offset as usize, 0);
        bytes.extend_from_slice(data);
        offset as u32
    };
    let views = tables
        .views
        .iter()
        .map(|v| place(bytemuck::bytes_of(&v.uniform)))
        .collect();
    let shaders = tables
        .shaders
        .iter()
        .map(|s| {
            if s.uniforms.is_empty() {
                0
            } else {
                place(&s.uniforms)
            }
        })
        .collect();
    UniformLayout {
        bytes,
        views,
        shaders,
    }
}

impl Graphics {
    /// Renders the recorded frame, presents it (or keeps it for capture in
    /// headless mode) and starts recording the next one. A window that
    /// cannot take a frame (minimised) skips rendering and reports zero
    /// draw calls.
    pub fn end_frame(&mut self) -> Result<FrameStats> {
        self.text.end_frame();
        let Some(frame) = self.output.acquire(&self.shared.gpu) else {
            self.stats = FrameStats::default();
            self.begin_frame();
            return Ok(self.stats);
        };
        let mut order = std::mem::take(&mut self.encode_order);
        order.push(0);

        let mut vertices: Vec<Vertex2D> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut prepared: Vec<(usize, Vec<DrawCall>)> = Vec::with_capacity(order.len());
        for &li in &order {
            let base = vertices.len() as u32;
            vertices.extend_from_slice(&self.lists[li].vertices);
            let calls = self.lists[li].flatten(&mut indices, base);
            prepared.push((li, calls));
        }
        let uniforms = layout_uniforms(&self.tables, self.buffers.align);
        let shared = &self.shared;
        let b = &mut self.buffers;
        FrameBuffers::upload(
            shared,
            &mut b.vertex,
            bytemuck::cast_slice(&vertices),
            "yowie vertices",
        );
        FrameBuffers::upload(
            shared,
            &mut b.index,
            bytemuck::cast_slice(&indices),
            "yowie indices",
        );
        if FrameBuffers::upload(shared, &mut b.uniform, &uniforms.bytes, "yowie uniforms") {
            b.view_group = uniform_group(shared, &shared.layouts.view, &b.uniform, VIEW_SIZE);
            b.user_groups.clear();
        }

        let mut encoder = shared
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yowie frame"),
            });
        let mut stats = FrameStats {
            vertices: vertices.len() as u32,
            indices: indices.len() as u32,
            outputs: order.len() as u32,
            ..FrameStats::default()
        };
        for (li, calls) in &prepared {
            let list = &self.lists[*li];
            let target_gpu;
            let (view, format) = match &list.target {
                ListTarget::Screen => (&frame.view, self.output.format()),
                ListTarget::Texture(t) => {
                    target_gpu = t.gpu();
                    (&target_gpu.view, target_gpu.texture.format())
                }
            };
            // Pass 1, 3D scene: renderer extensions record their scene
            // passes for the screen here, into their own HDR targets.
            // Pass 2, post-processing: the chain reads the scene targets and
            // writes `view`; the 2D overlay below then loads instead of
            // clearing. Neither exists yet, so the 2D pass clears.
            // Pass 3, the 2D overlay, which also carries UI and console
            // drawing on their layers.
            let mut pass2d = Pass2D {
                shared,
                tables: &self.tables,
                pipelines: &mut self.pipelines,
                buffers: &mut self.buffers,
                default_module: &self.default_module,
                uniforms: &uniforms,
            };
            stats.draw_calls += pass2d.encode(&mut encoder, view, format, list, calls);
        }
        let pending = if std::mem::take(&mut self.capture_requested) {
            if self.output.can_capture() {
                Some(readback::record(
                    shared.device(),
                    &mut encoder,
                    &frame.texture,
                ))
            } else {
                Some(Err(crate::GfxError::Readback(
                    "this window's surface does not allow copying frames".into(),
                )))
            }
        } else {
            None
        };
        shared.queue().submit([encoder.finish()]);
        if let Some(st) = frame.surface_texture {
            shared.queue().present(st);
        }
        if let Some(p) = pending {
            self.capture = Some(p.and_then(|p| p.finish(self.shared.device(), false)));
        }
        self.stats = stats;
        self.frame_number += 1;
        self.begin_frame();
        Ok(stats)
    }
}

/// Everything the 2D pass reads while it records.
struct Pass2D<'a> {
    shared: &'a Shared,
    tables: &'a FrameTables,
    pipelines: &'a mut PipelineCache,
    buffers: &'a mut FrameBuffers,
    default_module: &'a wgpu::ShaderModule,
    uniforms: &'a UniformLayout,
}

impl Pass2D<'_> {
    /// Records one output's 2D pass and returns its draw call count.
    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        list: &DrawList,
        calls: &[DrawCall],
    ) -> u32 {
        let load = match list.clear {
            Some(c) => {
                let [r, g, b, a] = c.to_linear_premultiplied();
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(r),
                    g: f64::from(g),
                    b: f64::from(b),
                    a: f64::from(a),
                })
            }
            None => wgpu::LoadOp::Load,
        };
        // Bind groups for uniform sizes are made before the pass borrows
        // the buffers.
        for call in calls {
            if call.state.shader != DEFAULT_SHADER {
                let size = self.tables.shaders[call.state.shader as usize]
                    .uniforms
                    .len() as u64;
                if size > 0 {
                    self.buffers.user_group(self.shared, size);
                }
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yowie 2d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if calls.is_empty() {
            return 0;
        }
        pass.set_vertex_buffer(0, self.buffers.vertex.slice(..));
        pass.set_index_buffer(self.buffers.index.slice(..), wgpu::IndexFormat::Uint32);
        let mut cur_key = None;
        let mut cur_texture = None;
        let mut cur_view = None;
        let mut cur_user = None;
        for call in calls {
            let st = call.state;
            let (shader, generation, module, user_size) = if st.shader == DEFAULT_SHADER {
                (0, 0, self.default_module, 0)
            } else {
                let s = &self.tables.shaders[st.shader as usize];
                (s.id, s.generation, &s.module, s.uniforms.len() as u64)
            };
            let key = PipelineKey {
                shader,
                generation,
                blend: st.blend,
                format,
            };
            if cur_key != Some(key) {
                let (device, layouts) = (self.shared.device(), &self.shared.layouts);
                let pipeline = self.pipelines.get(key, || {
                    sprite_pipeline(device, layouts, module, user_size > 0, st.blend, format)
                });
                pass.set_pipeline(&pipeline);
                cur_key = Some(key);
            }
            if cur_texture != Some(st.texture) {
                let tex = &self.tables.textures[st.texture as usize];
                pass.set_bind_group(0, &tex.gpu().bind_group, &[]);
                cur_texture = Some(st.texture);
            }
            if cur_view != Some(st.view) {
                let v = &self.tables.views[st.view as usize];
                pass.set_bind_group(
                    1,
                    &self.buffers.view_group,
                    &[self.uniforms.views[st.view as usize]],
                );
                pass.set_viewport(
                    v.viewport.x,
                    v.viewport.y,
                    v.viewport.w,
                    v.viewport.h,
                    0.0,
                    1.0,
                );
                let [x, y, w, h] = v.scissor;
                pass.set_scissor_rect(x, y, w, h);
                cur_view = Some(st.view);
            }
            if user_size > 0 && cur_user != Some(st.shader) {
                if let Some(group) = self.buffers.user_groups.get(&user_size) {
                    pass.set_bind_group(2, group, &[self.uniforms.shaders[st.shader as usize]]);
                }
                cur_user = Some(st.shader);
            }
            pass.draw_indexed(call.first..call.first + call.count, 0, 0..1);
        }
        calls.len() as u32
    }
}
