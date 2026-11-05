//! Recording one scene's GPU passes: the shadow atlas, then the HDR scene
//! (opaque, cut-out and fur meshes, static group bundles, the sky,
//! outlines, refracting meshes over a copy of the scene, x-ray tints,
//! order-independent transparency, sorted blended meshes, debug lines).

use super::pipelines::{FixedPipeline, MeshKey};
use super::prepare::{Batch, Prepared};
use super::render::SceneRenderer;
use super::shadow::atlas_rect;
use super::slot::SlotId;
use super::stencil::StencilKey;
use super::textures::TextureKey;
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
    /// Records the batches of one pass of record `ri` into `slot`.
    /// Pipelines, bind groups and vertex buffers change only where a batch
    /// differs from the one before.
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
        let features = match slot {
            SlotId::Scene(i) => &self.slots[i].features,
            SlotId::Capture(i) => &self.captures[i].features,
        };
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
                    .mesh(shared.device(), &self.layouts, b.key, custom);
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

    fn fixed(&mut self, shared: &Shared, which: FixedPipeline) -> wgpu::RenderPipeline {
        self.pipelines.fixed(shared.device(), &self.layouts, which)
    }
}

/// Begins an HDR scene pass. With `clear`, colour clears to transparent,
/// depth to the far plane (0, reversed) and stencil to 0; otherwise all
/// three load what earlier passes of the same frame left.
fn begin_hdr_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    color: &wgpu::TextureView,
    depth_stencil: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'e> {
    use wgpu::LoadOp;
    let (c, d, s) = if clear {
        (
            LoadOp::Clear(wgpu::Color::TRANSPARENT),
            LoadOp::Clear(0.0),
            LoadOp::Clear(0),
        )
    } else {
        (LoadOp::Load, LoadOp::Load, LoadOp::Load)
    };
    let store = wgpu::StoreOp::Store;
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("yowie scene"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: c, store },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth_stencil,
            depth_ops: Some(wgpu::Operations { load: d, store }),
            stencil_ops: Some(wgpu::Operations { load: s, store }),
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
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
        calls += r.draw_batches(shared, &mut pass, at, batches, &mut state);
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
    let at = (ri, slot);
    if !p.shadows.is_empty() {
        calls += encode_shadows(r, shared, encoder, at, p, &shadow_group, &instances);
    }
    let Some(t) = r.slot(slot).targets.as_ref() else {
        return calls;
    };
    let (color, depth_stencil) = (t.color.clone(), t.depth_stencil.clone());
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
    let bind = |pass: &mut wgpu::RenderPass<'_>| {
        pass.set_bind_group(0, &frame_group, &[]);
        pass.set_vertex_buffer(1, instances.slice(..));
    };
    let mut pass = begin_hdr_pass(encoder, &color, &depth_stencil, true);
    bind(&mut pass);
    let mut state = PassState::default();
    calls += r.draw_batches(shared, &mut pass, at, &p.opaque, &mut state);
    if let Some(st) = &statics {
        // The bundles test depth and leave the stencil as the opaque draws
        // set it, so outlines and stencil tests after them still work.
        pass.execute_bundles(st.early.iter());
        drop(pass);
        r.statics_late(encoder, slot, st);
        pass = begin_hdr_pass(encoder, &color, &depth_stencil, false);
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
    if !p.transmit.is_empty() {
        if let (Some(copy), Some(pipes)) = (
            r.slot(slot).features.copy.as_ref(),
            r.features.copy_pipelines(),
        ) {
            // Refracting meshes sample the opaque scene as it stands now.
            drop(pass);
            copy.record(pipes, encoder);
            pass = begin_hdr_pass(encoder, &color, &depth_stencil, false);
            bind(&mut pass);
            state = PassState::default();
        }
        // Probe captures have no copy: the stand-in has no coverage, so
        // glass there blends over what lies behind it without refraction.
        calls += r.draw_batches(shared, &mut pass, at, &p.transmit, &mut state);
    }
    calls += r.draw_batches(shared, &mut pass, at, &p.xray, &mut state);
    if !p.oit.is_empty()
        && let (Some(oit), Some(composite)) = (
            r.slot(slot).features.oit.as_ref(),
            r.features.composite().cloned(),
        )
    {
        drop(pass);
        let groups = oit.groups.clone();
        {
            let mut accum = oit.begin(encoder, &depth_stencil);
            bind(&mut accum);
            let mut s = PassState::default();
            calls += r.draw_batches(shared, &mut accum, at, &p.oit, &mut s);
        }
        pass = begin_hdr_pass(encoder, &color, &depth_stencil, false);
        pass.set_pipeline(&composite);
        pass.set_bind_group(0, &groups[0], &[]);
        pass.set_bind_group(1, &groups[1], &[]);
        pass.draw(0..3, 0..1);
        calls += 1;
        bind(&mut pass);
        state = PassState::default();
    }
    calls += r.draw_batches(shared, &mut pass, at, &p.blend, &mut state);
    // GPU particles and other extensions draw over the blended meshes with
    // the frame bind group bound; they set their own pipelines.
    let ctx = crate::Draw3D {
        color_format: super::layouts::HDR_FORMAT,
        depth_format: r.layouts.depth_format,
        frame_layout: &r.layouts.frame,
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
    calls
}
