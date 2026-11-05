//! Recording one scene's GPU passes: the shadow atlas, then the HDR scene
//! (opaque and cut-out meshes, the sky, blended meshes, debug lines).

use super::pipelines::{FixedPipeline, MeshKey};
use super::prepare::{Batch, Prepared};
use super::render::SceneRenderer;
use super::shadow::atlas_rect;
use super::slot::{SceneSlot, SlotId};
use crate::graphics::FrameStats;
use crate::shared::Shared;

/// What the current pass has bound.
#[derive(Debug, Default)]
pub(crate) struct PassState {
    pipeline: Option<MeshKey>,
    textures: Option<[u64; 5]>,
    mesh: Option<u32>,
    index_count: u32,
}

impl SceneRenderer {
    /// Records the batches of one pass. Pipelines, bind groups and vertex
    /// buffers change only where a batch differs from the one before.
    pub(crate) fn draw_batches(
        &mut self,
        shared: &Shared,
        pass: &mut wgpu::RenderPass<'_>,
        ri: usize,
        batches: &[Batch],
        state: &mut PassState,
    ) -> u32 {
        let rec = &self.records[ri];
        let mut calls = 0;
        for b in batches {
            if state.pipeline != Some(b.key) {
                let p = self.pipelines.mesh(shared.device(), &self.layouts, b.key);
                pass.set_pipeline(&p);
                state.pipeline = Some(b.key);
            }
            let material = &rec.materials[b.material as usize];
            let tex_key = material.texture_ids();
            if state.textures != Some(tex_key)
                && let Some(g) = self.textures.group(&tex_key)
            {
                pass.set_bind_group(1, g, &[]);
                state.textures = Some(tex_key);
            }
            if state.mesh != Some(b.mesh) {
                let mesh = &rec.meshes[b.mesh as usize];
                let gpu = mesh.inner.gpu.lock();
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                if let Some(skin) = &gpu.skin {
                    pass.set_vertex_buffer(2, skin.slice(..));
                }
                state.index_count = gpu.index_count;
                let morph = mesh
                    .inner
                    .morph
                    .as_ref()
                    .and_then(|m| m.bind_group.get())
                    .unwrap_or(&self.morph_none);
                pass.set_bind_group(2, morph, &[]);
                state.mesh = Some(b.mesh);
            }
            pass.draw_indexed(0..state.index_count, 0, b.first..b.first + b.count);
            calls += 1;
        }
        calls
    }

    fn fixed(&mut self, shared: &Shared, which: FixedPipeline) -> wgpu::RenderPipeline {
        self.pipelines.fixed(shared.device(), &self.layouts, which)
    }
}

/// Begins the HDR scene pass of a slot, clearing colour to transparent and
/// depth to the far plane (0, reversed), or with `clear` false keeping
/// what an earlier pass of the same frame drew.
fn begin_hdr_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    slot: &SceneSlot,
    clear: bool,
) -> Option<wgpu::RenderPass<'e>> {
    let t = slot.targets.as_ref()?;
    let (color_load, depth_load) = if clear {
        (
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            wgpu::LoadOp::Clear(0.0),
        )
    } else {
        (wgpu::LoadOp::Load, wgpu::LoadOp::Load)
    };
    Some(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("yowie scene"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &t.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: color_load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &t.depth,
            depth_ops: Some(wgpu::Operations {
                load: depth_load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }))
}

/// Records the shadow atlas pass: every shadow view's casters into its
/// atlas region.
fn encode_shadows(
    r: &mut SceneRenderer,
    shared: &Shared,
    encoder: &mut wgpu::CommandEncoder,
    ri: usize,
    p: &Prepared,
    group: &wgpu::BindGroup,
    instances: &wgpu::Buffer,
) -> u32 {
    let Some(atlas) = r.atlas_target().cloned() else {
        return 0;
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("yowie shadows"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &atlas,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_bind_group(0, group, &[]);
    pass.set_vertex_buffer(1, instances.slice(..));
    let mut state = PassState::default();
    let mut calls = 0;
    for (view, batches) in &p.shadows {
        let (x, y, s) = atlas_rect(*view);
        pass.set_viewport(x as f32, y as f32, s as f32, s as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, s, s);
        calls += r.draw_batches(shared, &mut pass, ri, batches, &mut state);
    }
    calls
}

/// Records record `ri`'s scene into `slot` and returns its draw call
/// count. Static groups culled on the GPU split the HDR pass in two: the
/// draws visible last frame go first, then the rest are tested against
/// the depth those left (see `statics`).
pub(crate) fn encode_scene(
    r: &mut SceneRenderer,
    shared: &Shared,
    encoder: &mut wgpu::CommandEncoder,
    ri: usize,
    slot: SlotId,
    p: &Prepared,
    stats: &mut FrameStats,
) -> u32 {
    let Some((_, frame_group, shadow_group)) = r.slot(slot).groups.clone() else {
        return 0;
    };
    let instances = r.slot(slot).instances.buffer.clone();
    let mut calls = 0;
    if !p.shadows.is_empty() {
        calls += encode_shadows(r, shared, encoder, ri, p, &shadow_group, &instances);
    }
    let statics = r.statics_early(shared, encoder, ri, slot, &frame_group, &p.frame);
    let lines = r.slot(slot).lines.buffer.clone();
    let rec = &r.records[ri];
    let (n_lines, n_top) = if matches!(slot, SlotId::Capture(_)) {
        (0, 0)
    } else {
        (rec.lines.len() as u32, rec.lines_on_top.len() as u32)
    };
    let sky = p.frame.env[3] > 0.5 && (rec.sky.is_some() || rec.environment.is_some());
    let sky_pipe = sky.then(|| r.fixed(shared, FixedPipeline::Sky));
    let lines_pipe = (n_lines > 0).then(|| r.fixed(shared, FixedPipeline::Lines));
    let top_pipe = (n_top > 0).then(|| r.fixed(shared, FixedPipeline::LinesOnTop));
    // Probe captures leave custom draws out: their compute work (a
    // particle step) must run once a frame, not once per cube face.
    let customs = if matches!(slot, SlotId::Capture(_)) {
        Vec::new()
    } else {
        r.records[ri].customs.clone()
    };
    for c in &customs {
        c.0.prepare(&mut crate::PrepareContext {
            device: shared.device(),
            queue: shared.queue(),
            encoder,
        });
    }
    let Some(mut pass) = begin_hdr_pass(encoder, r.slot(slot), true) else {
        return calls;
    };
    pass.set_bind_group(0, &frame_group, &[]);
    pass.set_vertex_buffer(1, instances.slice(..));
    let mut state = PassState::default();
    calls += r.draw_batches(shared, &mut pass, ri, &p.opaque, &mut state);
    if let Some(st) = &statics {
        pass.execute_bundles(st.early.iter());
        drop(pass);
        r.statics_late(encoder, slot, st);
        let Some(next) = begin_hdr_pass(encoder, r.slot(slot), false) else {
            return calls;
        };
        pass = next;
        pass.execute_bundles(st.late.iter());
        // Bundles leave no state behind, so bind the scene again.
        pass.set_bind_group(0, &frame_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
        state = PassState::default();
        calls += st.draws;
        stats.bundle_draws += st.draws;
        stats.culled_3d += st.culled;
        stats.occluded_3d += st.occluded;
    }
    if let Some(sky) = &sky_pipe {
        pass.set_pipeline(sky);
        pass.draw(0..3, 0..1);
        state.pipeline = None;
        calls += 1;
    }
    calls += r.draw_batches(shared, &mut pass, ri, &p.blend, &mut state);
    // GPU particles and other extensions draw over the blended meshes with
    // the frame bind group bound; they set their own pipelines.
    let ctx = crate::Draw3D {
        color_format: super::layouts::HDR_FORMAT,
        depth_format: super::layouts::DEPTH_FORMAT,
        frame_layout: &r.layouts.frame,
    };
    for c in &customs {
        c.0.draw_3d(&mut pass, &ctx);
        calls += 1;
    }
    if !customs.is_empty() {
        pass.set_bind_group(0, &frame_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
    }
    if let Some(lp) = &lines_pipe {
        pass.set_pipeline(lp);
        pass.set_vertex_buffer(0, lines.slice(..));
        pass.draw(0..n_lines, 0..1);
        calls += 1;
    }
    if let Some(tp) = &top_pipe {
        pass.set_pipeline(tp);
        pass.set_vertex_buffer(0, lines.slice(..));
        pass.draw(n_lines..n_lines + n_top, 0..1);
        calls += 1;
    }
    calls
}
