//! Mesh draws dithered out by a fraction of their pixels, for cross-fades
//! between a model and its impostor: the model hides the pixels whose
//! ordered-dither threshold is below the fraction, and the impostor shows
//! exactly those.

use yowie_core::Mat4;

use super::large::{PrepareContext, instance_buffer};
use super::large_pipelines::LargePipelines;
use super::record::SceneRecord;
use super::render::SceneRenderer;
use super::slot::GrowBuffer;
use super::vertex::{GpuInstance, affine_rows};

/// One dithered draw: a mesh and material slot of the scene record, where,
/// and the fraction of its pixels hidden.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FadeDraw {
    pub(crate) mesh: u32,
    pub(crate) material: u32,
    pub(crate) model: Mat4,
    pub(crate) hidden: f32,
}

#[derive(Clone, Copy, Debug)]
struct FadeBatch {
    mesh: u32,
    material: u32,
    first: u32,
    count: u32,
}

/// One slot's dithered instances and batches.
#[derive(Debug)]
pub(crate) struct FadeFrame {
    instances: GrowBuffer,
    batches: Vec<FadeBatch>,
}

impl FadeFrame {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            instances: instance_buffer(device, "yowie fade instances"),
            batches: Vec::new(),
        }
    }
}

/// Culls the dithered draws, groups them by mesh and material, and
/// uploads their instances.
pub(crate) fn prepare(ctx: &PrepareContext<'_>, rec: &SceneRecord, frame: &mut FadeFrame) {
    frame.batches.clear();
    let draws = &rec.large.fades;
    if draws.is_empty() {
        return;
    }
    let mut order: Vec<&FadeDraw> = draws
        .iter()
        .filter(|d| {
            let b = rec.meshes[d.mesh as usize].bounds().transformed(&d.model);
            ctx.view.intersects_aabb(&b)
        })
        .collect();
    order.sort_by_key(|d| (d.material, d.mesh));
    let mut instances = Vec::with_capacity(order.len());
    for d in order {
        let at = instances.len() as u32;
        instances.push(GpuInstance {
            rows: affine_rows(&d.model),
            params: [d.material, u32::MAX, d.hidden.clamp(0.0, 1.0).to_bits(), 0],
        });
        match frame.batches.last_mut() {
            Some(b) if b.mesh == d.mesh && b.material == d.material => b.count += 1,
            _ => frame.batches.push(FadeBatch {
                mesh: d.mesh,
                material: d.material,
                first: at,
                count: 1,
            }),
        }
    }
    frame.instances.write(
        ctx.shared.device(),
        ctx.shared.queue(),
        bytemuck::cast_slice(&instances),
    );
}

/// Records the dithered draws.
pub(crate) fn encode(
    r: &SceneRenderer,
    p: &LargePipelines,
    rec: &SceneRecord,
    frame: &FadeFrame,
    pass: &mut wgpu::RenderPass<'_>,
) -> u32 {
    if frame.batches.is_empty() {
        return 0;
    }
    pass.set_pipeline(&p.fade);
    pass.set_vertex_buffer(1, frame.instances.buffer.slice(..));
    pass.set_bind_group(2, &r.morph_none, &[]);
    let mut calls = 0;
    for b in &frame.batches {
        let material = &rec.materials[b.material as usize];
        let Some(group) = r.textures.group(&material.texture_ids()) else {
            continue;
        };
        pass.set_bind_group(1, group, &[]);
        let gpu = rec.meshes[b.mesh as usize].inner.gpu.lock();
        pass.set_vertex_buffer(0, gpu.vertices.slice(..));
        pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..gpu.index_count, 0, b.first..b.first + b.count);
        calls += 1;
    }
    calls
}
