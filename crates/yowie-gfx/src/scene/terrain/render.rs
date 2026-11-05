//! Drawing recorded terrains: culling chunks against the camera and the
//! shadow views, grouping them by level into instance runs, and recording
//! one draw call per terrain and level.

use super::{Terrain, TerrainInner};
use crate::graphics::FrameStats;
use crate::scene::large::{LargeRecord, PrepareContext, instance_buffer};
use crate::scene::large_pipelines::LargePipelines;
use crate::scene::slot::GrowBuffer;

/// A run of chunk instances of one terrain at one level.
#[derive(Clone, Debug)]
pub(crate) struct TerrainBatch {
    group: wgpu::BindGroup,
    indices: wgpu::Buffer,
    index_count: u32,
    first: u32,
    count: u32,
    /// The shadow view, or `usize::MAX` for the scene pass.
    view: usize,
}

/// One slot's terrain instances and batches.
#[derive(Debug)]
pub(crate) struct TerrainFrame {
    instances: GrowBuffer,
    batches: Vec<TerrainBatch>,
}

impl TerrainFrame {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            instances: instance_buffer(device, "yowie terrain chunks"),
            batches: Vec::new(),
        }
    }
}

/// The bind group of a terrain, rebuilt when a layer image or the state
/// generation changed.
fn bind_group(ctx: &PrepareContext<'_>, t: &TerrainInner) -> wgpu::BindGroup {
    let state = t.state.read();
    let images = t.gpu.layer_images(&state);
    let key = (
        std::array::from_fn(|i| std::sync::Arc::as_ptr(&images[i]) as usize),
        state.generation,
    );
    let mut cached = t.gpu.group.lock();
    if let Some((k, g)) = cached.as_ref()
        && *k == key
    {
        return g.clone();
    }
    let normals = t.gpu.normals.create_view(&Default::default());
    let splat = t.gpu.splat.create_view(&Default::default());
    let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
    let tv = wgpu::BindingResource::TextureView;
    let group = ctx
        .shared
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie terrain"),
            layout: &ctx.pipelines.terrain_layout,
            entries: &[
                entry(32, t.gpu.heights.as_entire_binding()),
                entry(33, t.gpu.params.as_entire_binding()),
                entry(34, tv(&normals)),
                entry(35, tv(&splat)),
                entry(36, tv(&images[0].view)),
                entry(37, tv(&images[1].view)),
                entry(38, tv(&images[2].view)),
                entry(39, tv(&images[3].view)),
                entry(40, wgpu::BindingResource::Sampler(ctx.repeat)),
                entry(41, wgpu::BindingResource::Sampler(ctx.clamp)),
            ],
        });
    *cached = Some((key, group.clone()));
    group
}

/// The bits above the level in a chunk instance's third word: the shadow
/// view (`usize::MAX` for the scene pass) and the flags.
fn tag(view: usize, flags: u32) -> u32 {
    let view_bits = if view == usize::MAX { 0 } else { view as u32 };
    (view_bits << 8) | (flags << 16)
}

/// Appends the chunks of `terrain` that pass `keep`, grouped by level,
/// to `instances`, with a batch per level.
fn emit(
    terrain: &Terrain,
    levels: &[u8],
    group: &wgpu::BindGroup,
    (view, flags): (usize, u32),
    mut keep: impl FnMut(&crate::scene::bounds::Aabb) -> bool,
    instances: &mut Vec<[u32; 4]>,
    batches: &mut Vec<TerrainBatch>,
) -> u32 {
    let t = &terrain.inner;
    let state = t.state.read();
    let mut kept = 0;
    let tag = tag(view, flags);
    for level in 0..t.levels {
        let first = instances.len() as u32;
        for (i, c) in state.chunks.iter().enumerate() {
            if u32::from(levels[i]) != level || !keep(&c.bounds) {
                continue;
            }
            let (x0, z0) = t.chunk_origin(i as u32);
            instances.push([x0, z0, level | tag, c.skirt.to_bits()]);
        }
        let count = instances.len() as u32 - first;
        if count > 0 {
            let (indices, index_count) = &t.gpu.indices[level as usize];
            batches.push(TerrainBatch {
                group: group.clone(),
                indices: indices.clone(),
                index_count: *index_count,
                first,
                count,
                view,
            });
            kept += count;
        }
    }
    kept
}

/// Culls the recorded terrains and uploads their chunk instances.
pub(crate) fn prepare(
    ctx: &PrepareContext<'_>,
    rec: &LargeRecord,
    frame: &mut TerrainFrame,
    stats: &mut FrameStats,
) {
    frame.batches.clear();
    if rec.terrains.is_empty() {
        return;
    }
    let mut instances = Vec::new();
    for d in &rec.terrains {
        let group = bind_group(ctx, &d.terrain.inner);
        let flags = u32::from(d.options.show_levels);
        let total = d.terrain.chunk_count() as u32;
        let drawn = emit(
            &d.terrain,
            &d.levels,
            &group,
            (usize::MAX, flags),
            |b| ctx.view.intersects_aabb(b),
            &mut instances,
            &mut frame.batches,
        );
        stats.terrain_chunks += drawn;
        stats.terrain_chunks_culled += total - drawn;
        if d.options.cast_shadows {
            for (view, frustum) in ctx.shadows {
                emit(
                    &d.terrain,
                    &d.levels,
                    &group,
                    (*view, 0),
                    |b| frustum.intersects_aabb(b),
                    &mut instances,
                    &mut frame.batches,
                );
            }
        }
    }
    frame.instances.write(
        ctx.shared.device(),
        ctx.shared.queue(),
        bytemuck::cast_slice(&instances),
    );
}

fn draw(pass: &mut wgpu::RenderPass<'_>, frame: &TerrainFrame, view: usize) -> u32 {
    let mut calls = 0;
    for b in frame.batches.iter().filter(|b| b.view == view) {
        pass.set_bind_group(1, &b.group, &[]);
        pass.set_index_buffer(b.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..b.index_count, 0, b.first..b.first + b.count);
        calls += 1;
    }
    calls
}

/// Records the terrain batches of the scene pass.
pub(crate) fn encode(
    p: &LargePipelines,
    frame: &TerrainFrame,
    pass: &mut wgpu::RenderPass<'_>,
) -> u32 {
    if !frame.batches.iter().any(|b| b.view == usize::MAX) {
        return 0;
    }
    pass.set_pipeline(&p.terrain);
    pass.set_vertex_buffer(0, frame.instances.buffer.slice(..));
    draw(pass, frame, usize::MAX)
}

/// Records the terrain casters of shadow view `view`.
pub(crate) fn encode_shadow(
    p: &LargePipelines,
    frame: &TerrainFrame,
    pass: &mut wgpu::RenderPass<'_>,
    view: usize,
) -> u32 {
    if !frame.batches.iter().any(|b| b.view == view) {
        return 0;
    }
    pass.set_pipeline(&p.terrain_shadow);
    pass.set_vertex_buffer(0, frame.instances.buffer.slice(..));
    draw(pass, frame, view)
}
