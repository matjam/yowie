//! Recording one scene's GPU passes: the shadow atlas, then the HDR scene
//! (opaque and cut-out meshes, the sky, blended meshes, debug lines).

use super::pipelines::{FixedPipeline, MeshKey};
use super::prepare::{Batch, Prepared};
use super::render::SceneRenderer;
use super::shadow::atlas_rect;
use super::slot::{SceneSlot, SlotId};
use super::targets::SceneFormat;
use crate::gpu_timer::{Span, Timed};
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
    /// Records the batches of one pass into attachments of `format`.
    /// Pipelines, bind groups and vertex buffers change only where a batch
    /// differs from the one before.
    pub(crate) fn draw_batches(
        &mut self,
        shared: &Shared,
        pass: &mut wgpu::RenderPass<'_>,
        ri: usize,
        format: SceneFormat,
        batches: &[Batch],
        state: &mut PassState,
    ) -> u32 {
        let rec = &self.records[ri];
        let mut calls = 0;
        for b in batches {
            if state.pipeline != Some(b.key) {
                let p = self
                    .pipelines
                    .mesh(shared.device(), &self.layouts, b.key, format);
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

    fn fixed(
        &mut self,
        shared: &Shared,
        format: SceneFormat,
        which: FixedPipeline,
    ) -> wgpu::RenderPipeline {
        self.pipelines
            .fixed(shared.device(), &self.layouts, which, format)
    }
}

/// Begins an HDR scene pass of a slot into the attachments its post
/// settings asked for (`targets.rs`). The `first` pass clears colour to
/// transparent and depth to the far plane (0, reversed); a pass that
/// continues one loads them. `keep` keeps multisampled colour for a pass
/// that follows.
fn begin_hdr_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    slot: &SceneSlot,
    first: bool,
    keep: bool,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> Option<wgpu::RenderPass<'e>> {
    let t = slot.targets.as_ref()?;
    Some(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("yowie scene"),
        color_attachments: &t.color_attachments(first, keep),
        depth_stencil_attachment: Some(t.depth_attachment(first)),
        timestamp_writes: timestamps,
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
    let span = shared.span(Timed::Shadows);
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
        timestamp_writes: span.as_ref().map(Span::whole),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_bind_group(0, group, &[]);
    pass.set_vertex_buffer(1, instances.slice(..));
    let mut state = PassState::default();
    let mut calls = 0;
    // Shadow pipelines have no colour targets; any format serves.
    let format = SceneFormat::default();
    for (view, batches) in &p.shadows {
        let (x, y, s) = atlas_rect(*view);
        pass.set_viewport(x as f32, y as f32, s as f32, s as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, s, s);
        calls += r.draw_batches(shared, &mut pass, ri, format, batches, &mut state);
    }
    calls
}

/// Records record `ri`'s scene into `slot` and returns its draw call
/// count. Static groups culled on the GPU split the HDR pass in two: the
/// draws visible last frame go first, then the rest are tested against
/// the depth those left (see `statics`). A timed frame on a GPU that
/// cannot write timestamps inside a pass also starts a new pass for the
/// blended draws, to time them apart.
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
    let format = r.slot(slot).format();
    let mut calls = 0;
    if !p.shadows.is_empty() {
        calls += encode_shadows(r, shared, encoder, ri, p, &shadow_group, &instances);
    }
    let statics = r.statics_early(shared, encoder, ri, slot, &frame_group, &p.frame);
    let lines = r.slot(slot).lines.buffer.clone();
    let rec = &r.records[ri];
    let captured = matches!(slot, SlotId::Capture(_));
    let (n_lines, n_top) = if captured {
        (0, 0)
    } else {
        (rec.lines.len() as u32, rec.lines_on_top.len() as u32)
    };
    let sky = p.frame.env[3] > 0.5 && (rec.sky.is_some() || rec.environment.is_some());
    let sky_pipe = sky.then(|| r.fixed(shared, format, FixedPipeline::Sky));
    let lines_pipe = (n_lines > 0).then(|| r.fixed(shared, format, FixedPipeline::Lines));
    let top_pipe = (n_top > 0).then(|| r.fixed(shared, format, FixedPipeline::LinesOnTop));
    // Probe captures leave custom draws out: their compute work (a
    // particle step) must run once a frame, not once per cube face.
    let customs = if captured {
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
    // Timed frames measure the opaque and blended halves apart: with
    // timestamps inside passes in one pass, otherwise at pass edges.
    // Probe captures are not timed.
    let (opaque_span, blend_span) = if captured {
        (None, None)
    } else {
        (shared.span(Timed::Opaque), shared.span(Timed::Blended))
    };
    let inside = shared.timer.lock().inside_passes;
    let split = blend_span.is_some() && !inside;
    let edge = |begin: bool, end: bool| {
        opaque_span
            .as_ref()
            .filter(|_| !inside)
            .and_then(|s| s.part(begin, end))
    };
    let keep = split || statics.is_some();
    let first_writes = edge(true, split && statics.is_none());
    let Some(mut pass) = begin_hdr_pass(encoder, r.slot(slot), true, keep, first_writes) else {
        return calls;
    };
    if let Some(s) = opaque_span.as_ref().filter(|_| inside) {
        pass.write_timestamp(&s.set, s.begin);
    }
    pass.set_bind_group(0, &frame_group, &[]);
    pass.set_vertex_buffer(1, instances.slice(..));
    let mut state = PassState::default();
    calls += r.draw_batches(shared, &mut pass, ri, format, &p.opaque, &mut state);
    if let Some(st) = &statics {
        pass.execute_bundles(st.early.iter());
        drop(pass);
        r.statics_late(encoder, slot, st);
        let writes = edge(false, split);
        let Some(next) = begin_hdr_pass(encoder, r.slot(slot), false, split, writes) else {
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
    if inside && let (Some(o), Some(b)) = (&opaque_span, &blend_span) {
        pass.write_timestamp(&o.set, o.end);
        pass.write_timestamp(&b.set, b.begin);
    }
    if split {
        drop(pass);
        let writes = blend_span.as_ref().map(Span::whole);
        let Some(next) = begin_hdr_pass(encoder, r.slot(slot), false, false, writes) else {
            return calls;
        };
        pass = next;
        pass.set_bind_group(0, &frame_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
        state = PassState::default();
    }
    calls += r.draw_batches(shared, &mut pass, ri, format, &p.blend, &mut state);
    // GPU particles and other extensions draw over the blended meshes with
    // the frame bind group bound; they set their own pipelines.
    let extra_targets = format.color_targets(None, false).split_off(1);
    let ctx = crate::Draw3D {
        color_format: super::layouts::HDR_FORMAT,
        depth_format: super::layouts::DEPTH_FORMAT,
        frame_layout: &r.layouts.frame,
        sample_count: format.samples,
        extra_targets: &extra_targets,
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
    if let Some(b) = blend_span.as_ref().filter(|_| inside) {
        pass.write_timestamp(&b.set, b.end);
    }
    calls
}
