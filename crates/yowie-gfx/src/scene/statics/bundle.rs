//! Recording a static group's render bundles: one per culling phase, each
//! drawing every batch indirectly from the instances the cull wrote.

use super::StaticUse;
use super::gpu::{BundleKey, INSTANCE_BYTES};
use super::layout::Layout;
use crate::scene::layouts::{DEPTH_FORMAT, HDR_FORMAT};
use crate::scene::pipelines::MeshKey;
use crate::scene::render::SceneRenderer;
use crate::scene::slot::SlotId;
use crate::shared::Shared;

/// Bytes of one set of indexed indirect draw arguments.
const ARGS_BYTES: u64 = 20;

impl SceneRenderer {
    /// Makes sure group `u`'s bundles for `slot` match the scene's bind
    /// group, the materials' texture groups and the meshes' buffers,
    /// recording them again when not. Returns the indirect draws the two
    /// bundles make.
    pub(crate) fn ensure_bundles(
        &mut self,
        shared: &Shared,
        u: &StaticUse,
        slot: SlotId,
        frame_group: &wgpu::BindGroup,
    ) -> u32 {
        let device = shared.device();
        let l = u.data.layout();
        let key = BundleKey {
            frame: frame_group.clone(),
            materials: l
                .materials
                .iter()
                .map(|m| self.textures.group(&m.texture_ids()).cloned())
                .collect(),
            meshes: l
                .meshes
                .iter()
                .map(|m| {
                    let g = m.inner.gpu.lock();
                    (g.vertices.clone(), g.indices.clone())
                })
                .collect(),
        };
        let draws = 2 * l.batches.len() as u32;
        let Some(g) = self.statics.groups.get(&u.data.id) else {
            return 0;
        };
        let Some(v) = g.views.get(&slot) else {
            return 0;
        };
        if v.bundles.as_ref().is_some_and(|(k, _, _)| *k == key) {
            return draws;
        }
        let (args, visible) = (v.args.clone(), v.visible.clone());
        let late_args = u64::from(g.n_batches) * 5 * 4;
        let late_region = u64::from(g.n_items) * INSTANCE_BYTES;
        let pipes: Vec<wgpu::RenderPipeline> = l
            .batches
            .iter()
            .map(|b| self.pipelines.mesh(device, &self.layouts, b.key))
            .collect();
        let rec = Recording {
            key: &key,
            pipes: &pipes,
            layout: l,
            args: &args,
            visible: &visible,
            morph_none: &self.morph_none,
        };
        let early = rec.record(device, 0, 0);
        let late = rec.record(device, late_args, late_region);
        if let Some(v) = self
            .statics
            .groups
            .get_mut(&u.data.id)
            .and_then(|g| g.views.get_mut(&slot))
        {
            v.bundles = Some((key, early, late));
        }
        draws
    }
}

/// What both bundles of a group are recorded from.
struct Recording<'a> {
    key: &'a BundleKey,
    pipes: &'a [wgpu::RenderPipeline],
    layout: &'a Layout,
    args: &'a wgpu::Buffer,
    visible: &'a wgpu::Buffer,
    morph_none: &'a wgpu::BindGroup,
}

impl Recording<'_> {
    /// A bundle drawing every batch with the arguments at `args_base` and
    /// the instances at `region` (bytes).
    fn record(&self, device: &wgpu::Device, args_base: u64, region: u64) -> wgpu::RenderBundle {
        let mut enc = device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
            label: Some("yowie static"),
            color_formats: &[Some(HDR_FORMAT)],
            depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                format: DEPTH_FORMAT,
                depth_read_only: false,
                stencil_read_only: true,
            }),
            sample_count: 1,
            multiview: None,
        });
        enc.set_bind_group(0, &self.key.frame, &[]);
        enc.set_bind_group(2, self.morph_none, &[]);
        let mut pipeline: Option<MeshKey> = None;
        let mut material = None;
        let mut mesh = None;
        for (b, batch) in self.layout.batches.iter().enumerate() {
            let Some(group) = self.key.materials[batch.material as usize].as_ref() else {
                continue;
            };
            if pipeline != Some(batch.key) {
                enc.set_pipeline(&self.pipes[b]);
                pipeline = Some(batch.key);
            }
            if material != Some(batch.material) {
                enc.set_bind_group(1, group, &[]);
                material = Some(batch.material);
            }
            if mesh != Some(batch.mesh) {
                let (vertices, indices) = &self.key.meshes[batch.mesh as usize];
                enc.set_vertex_buffer(0, vertices.slice(..));
                enc.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                mesh = Some(batch.mesh);
            }
            let at = region + u64::from(batch.first) * INSTANCE_BYTES;
            enc.set_vertex_buffer(1, self.visible.slice(at..));
            enc.draw_indexed_indirect(self.args, args_base + b as u64 * ARGS_BYTES);
        }
        enc.finish(&wgpu::RenderBundleDescriptor {
            label: Some("yowie static"),
        })
    }
}
