//! Recording one scene's GPU passes: the shadow atlas, then the HDR scene
//! (opaque, cut-out and fur meshes, static group bundles, the sky,
//! outlines, refracting meshes over a copy of the scene, x-ray tints,
//! order-independent transparency, sorted blended meshes, debug lines).

use super::pipelines::{FixedPipeline, MeshKey};
use super::prepare::{Batch, Prepared};
use super::render::SceneRenderer;
use super::shadow::atlas_rect;
use super::slot::{SceneSlot, SlotId};
use super::stencil::StencilKey;
use super::targets::SceneFormat;
use super::textures::TextureKey;
use crate::gpu_timer::{Span, Timed};
use crate::graphics::FrameStats;
use crate::shared::Shared;

/// What the current pass has bound.
#[derive(Debug, Default)]
pub(crate) struct PassState {
    pipeline: Option<MeshKey>,
    textures: Option<TextureKey>,
    mesh: Option<u32>,
    index_count: u32,
    stencil: Option<u32>,
    user: Option<u32>,
}

impl SceneRenderer {
    /// Records the batches of one pass of record `ri` into `slot`, with
    /// pipelines matching the slot's attachments. Pipelines, bind groups
    /// and vertex buffers change only where a batch differs from the one
    /// before.
    pub(crate) fn draw_batches(
        &mut self,
        shared: &Shared,
        pass: &mut wgpu::RenderPass<'_>,
        (ri, slot): (usize, SlotId),
        batches: &[Batch],
        state: &mut PassState,
    ) -> u32 {
        let rec = &self.records[ri];
        // Matched by field so the pipeline cache below can still change.
        let slot = match slot {
            SlotId::Scene(i) => &self.slots[i],
            SlotId::Capture(i) => &self.captures[i],
        };
        let (features, format) = (&slot.features, slot.format());
        let mut calls = 0;
        for b in batches {
            let material = &rec.materials[b.material as usize];
            if b.key.shader.1 {
                // A shader with uniforms draws only with its block bound.
                let offset = features
                    .user_offsets
                    .get(b.material as usize)
                    .copied()
                    .unwrap_or(u32::MAX);
                let Some((_, group)) = features.user_group.as_ref() else {
                    continue;
                };
                if offset == u32::MAX {
                    continue;
                }
                if state.user != Some(offset) {
                    pass.set_bind_group(3, group, &[offset]);
                    state.user = Some(offset);
                }
            }
            if state.pipeline != Some(b.key) {
                let custom = if b.key.shader.0 != 0 {
                    material.shader.as_ref().map(|s| s.module())
                } else {
                    None
                };
                let p = self
                    .pipelines
                    .mesh(shared.device(), &self.layouts, b.key, format, custom);
                pass.set_pipeline(&p);
                state.pipeline = Some(b.key);
            }
            if b.key.stencil != StencilKey::None {
                let reference = b.key.stencil.reference(&material.stencil);
                if state.stencil != Some(reference) {
                    pass.set_stencil_reference(reference);
                    state.stencil = Some(reference);
                }
            }
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
/// transparent, depth to the far plane (0, reversed) and stencil to 0; a
/// pass that continues one loads them. `keep` keeps multisampled colour
/// for a pass that follows.
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
    at: (usize, SlotId),
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
    for (view, batches) in &p.shadows {
        let (x, y, s) = atlas_rect(*view);
        pass.set_viewport(x as f32, y as f32, s as f32, s as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, s, s);
        calls += r.draw_batches(shared, &mut pass, at, batches, &mut state);
    }
    calls += r.draw_large_shadows(&mut pass, at.1, p);
    calls
}

/// Records record `ri`'s scene into `slot` and returns its draw call
/// count.
///
/// The HDR scene ends its pass and begins another where later draws need
/// work outside it: static groups culled on the GPU test their remaining
/// draws against the depth the first pass left (see `statics`),
/// refracting meshes read a copy of the opaque scene, and the
/// transparency accumulation has its own targets. A timed frame on a GPU
/// that cannot write timestamps inside a pass also begins a pass for the
/// blended half, to time it apart. Every pass but the last keeps its
/// multisampled colour for the next; each pass resolves into the
/// single-sample colour when it ends, which is what the scene copy reads.
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
    let at = (ri, slot);
    if !p.shadows.is_empty() {
        calls += encode_shadows(r, shared, encoder, at, p, &shadow_group, &instances);
    }
    let Some(depth_stencil) = r
        .slot(slot)
        .targets
        .as_ref()
        .map(|t| t.attached_depth().clone())
    else {
        return calls;
    };
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
    let bind = |pass: &mut wgpu::RenderPass<'_>| {
        pass.set_bind_group(0, &frame_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
    };

    // Every break in the HDR pass is known before the first pass begins,
    // so each pass knows whether another follows it.
    let copy_break = !p.transmit.is_empty()
        && r.slot(slot).features.copy.is_some()
        && r.features.copy_pipelines().is_some();
    // Water and blended billboards join the accumulation when the scene
    // uses order-independent transparency, and draw after the sorted
    // blended meshes otherwise.
    let large_oit = r.large_oit(slot);
    let oit_break = (!p.oit.is_empty() || large_oit)
        && r.slot(slot).features.oit.is_some()
        && r.features.composite(format).is_some();
    let large_oit = large_oit && oit_break;

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
    let opaque_edge = |begin: bool, end: bool| {
        opaque_span
            .as_ref()
            .filter(|_| !inside)
            .and_then(|s| s.part(begin, end))
    };
    let blend_edge = |begin: bool, end: bool| {
        blend_span
            .as_ref()
            .filter(|_| split)
            .and_then(|s| s.part(begin, end))
    };
    // A pass for the blended half begins after the opaque one when timing
    // splits them or refracting meshes need the scene copy.
    let blend_break = split || copy_break;
    let after_statics = blend_break || oit_break;

    let keep = statics.is_some() || after_statics;
    let writes = opaque_edge(true, split && statics.is_none());
    let Some(mut pass) = begin_hdr_pass(encoder, r.slot(slot), true, keep, writes) else {
        return calls;
    };
    if let Some(s) = opaque_span.as_ref().filter(|_| inside) {
        pass.write_timestamp(&s.set, s.begin);
    }
    pass.set_bind_group(0, &frame_group, &[]);
    // Terrain, cut-out billboards and dithered fades set their own
    // pipelines, group 1 and vertex buffers.
    calls += r.draw_large_opaque(&mut pass, slot);
    bind(&mut pass);
    let mut state = PassState::default();
    calls += r.draw_batches(shared, &mut pass, at, &p.opaque, &mut state);
    if let Some(st) = &statics {
        // The bundles test depth and leave the stencil as the opaque draws
        // set it, so outlines and stencil tests after them still work.
        pass.execute_bundles(st.early.iter());
        drop(pass);
        r.statics_late(encoder, slot, st);
        let writes = opaque_edge(false, split);
        let Some(next) = begin_hdr_pass(encoder, r.slot(slot), false, after_statics, writes) else {
            return calls;
        };
        pass = next;
        pass.execute_bundles(st.late.iter());
        // Bundles leave no state behind, so bind the scene again.
        bind(&mut pass);
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
    calls += r.draw_batches(shared, &mut pass, at, &p.outline, &mut state);

    // The blended half: refracting meshes, x-ray tints, transparency.
    if inside && let (Some(o), Some(b)) = (&opaque_span, &blend_span) {
        pass.write_timestamp(&o.set, o.end);
        pass.write_timestamp(&b.set, b.begin);
    }
    if blend_break {
        drop(pass);
        if copy_break
            && let (Some(copy), Some(pipes)) = (
                r.slot(slot).features.copy.as_ref(),
                r.features.copy_pipelines(),
            )
        {
            // Refracting meshes sample the opaque scene as it stands now,
            // resolved into the single-sample colour by the pass above.
            copy.record(pipes, encoder);
        }
        let writes = blend_edge(true, !oit_break);
        let Some(next) = begin_hdr_pass(encoder, r.slot(slot), false, oit_break, writes) else {
            return calls;
        };
        pass = next;
        bind(&mut pass);
        state = PassState::default();
    }
    // Probe captures have no copy: the stand-in has no coverage, so glass
    // there blends over what lies behind it without refraction.
    calls += r.draw_batches(shared, &mut pass, at, &p.transmit, &mut state);
    calls += r.draw_batches(shared, &mut pass, at, &p.xray, &mut state);
    if oit_break
        && let (Some(oit), Some(composite)) = (
            r.slot(slot).features.oit.as_ref(),
            r.features.composite(format).cloned(),
        )
    {
        drop(pass);
        let groups = oit.groups.clone();
        {
            // The accumulation tests against the depth and stencil the
            // HDR pass attaches, multisampled when the scene is.
            let mut accum = oit.begin(encoder, &depth_stencil);
            bind(&mut accum);
            let mut s = PassState::default();
            calls += r.draw_batches(shared, &mut accum, at, &p.oit, &mut s);
            if large_oit {
                calls += r.draw_large_blend(&mut accum, slot, true);
            }
        }
        let writes = blend_edge(false, true);
        let Some(next) = begin_hdr_pass(encoder, r.slot(slot), false, false, writes) else {
            return calls;
        };
        pass = next;
        pass.set_pipeline(&composite);
        pass.set_bind_group(0, &groups[0], &[]);
        pass.set_bind_group(1, &groups[1], &[]);
        pass.draw(0..3, 0..1);
        calls += 1;
        bind(&mut pass);
        state = PassState::default();
    }
    calls += r.draw_batches(shared, &mut pass, at, &p.blend, &mut state);
    if !large_oit {
        calls += r.draw_large_blend(&mut pass, slot, false);
    }
    // GPU particles and other extensions draw over the blended meshes with
    // the frame bind group bound; they set their own pipelines.
    let extra_targets = format.color_targets(None, false).split_off(1);
    let ctx = crate::Draw3D {
        color_format: super::layouts::HDR_FORMAT,
        depth_format: r.layouts.depth_format,
        frame_layout: &r.layouts.frame,
        sample_count: format.samples,
        extra_targets: &extra_targets,
    };
    for c in &customs {
        c.0.draw_3d(&mut pass, &ctx);
        calls += 1;
    }
    if !customs.is_empty() {
        bind(&mut pass);
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
