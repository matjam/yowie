//! Writing a prepared scene into its slot's buffers and building the
//! slot's frame bind groups.

use super::layouts::{
    DECAL_COLOR_BINDING, DECAL_NORMAL_BINDING, DECALS_BINDING, SCENE_COPY_BINDING,
};
use super::prepare::Prepared;
use super::render::SceneRenderer;
use super::sky::Tonemap;
use super::slot::{GroupKey, SlotId};
use crate::shared::Shared;

/// Storage bindings may not be empty; pads an empty slice to one word.
fn padded(bytes: &[u8]) -> &[u8] {
    const ZERO: [u8; 16] = [0; 16];
    if bytes.is_empty() { &ZERO } else { bytes }
}

fn buf(b: &wgpu::Buffer) -> wgpu::BindingResource<'_> {
    b.as_entire_binding()
}

impl SceneRenderer {
    /// Writes the frame block, lights, clusters, materials, joints, morph
    /// weights, instances, lines and resolve settings of record `ri` into
    /// `slot`.
    pub(super) fn upload(&mut self, shared: &Shared, ri: usize, slot: SlotId, p: &Prepared) {
        let (device, queue) = (shared.device(), shared.queue());
        let rec = &self.records[ri];
        let slot = match slot {
            SlotId::Scene(i) => &mut self.slots[i],
            SlotId::Capture(i) => &mut self.captures[i],
        };
        queue.write_buffer(&slot.frame, 0, bytemuck::bytes_of(&p.frame));
        let joints: Vec<[[f32; 4]; 4]> = rec.joints.iter().map(|m| m.to_cols_array_2d()).collect();
        let mut grew = false;
        grew |= slot
            .lights
            .write(device, queue, padded(bytemuck::cast_slice(&p.lights)));
        grew |= slot
            .clusters
            .write(device, queue, padded(bytemuck::cast_slice(&p.clusters)));
        grew |= slot.indices.write(
            device,
            queue,
            padded(bytemuck::cast_slice(&p.light_indices)),
        );
        grew |= slot.materials.write(
            device,
            queue,
            padded(bytemuck::cast_slice(&rec.material_gpu)),
        );
        grew |= slot
            .joints
            .write(device, queue, padded(bytemuck::cast_slice(&joints)));
        grew |= slot.morph_weights.write(
            device,
            queue,
            padded(bytemuck::cast_slice(&rec.morph_weights)),
        );
        slot.instances
            .write(device, queue, bytemuck::cast_slice(&p.instances));
        let mut lines = rec.lines.clone();
        lines.extend_from_slice(&rec.lines_on_top);
        slot.lines
            .write(device, queue, bytemuck::cast_slice(&lines));
        let b = rec.background_color;
        let mode = match rec.tonemap {
            Tonemap::None => 0.0,
            Tonemap::Aces => 1.0,
            Tonemap::Agx => 2.0,
        };
        let resolve: [f32; 8] = [b[0], b[1], b[2], b[3], rec.exposure, mode, 0.0, 0.0];
        queue.write_buffer(&slot.resolve, 0, bytemuck::cast_slice(&resolve));
        if grew {
            self.generation += 1;
        }
    }

    /// Builds `slot`'s frame and shadow-pass bind groups for record `ri`
    /// when a buffer, the environment, the light probe grid or the shadow
    /// atlas changed.
    pub(super) fn bind_groups(
        &mut self,
        shared: &Shared,
        ri: usize,
        slot_id: SlotId,
        env: u64,
        grid: u64,
        shadows: bool,
    ) {
        let atlas = if shadows {
            self.atlas.as_ref().unwrap_or(&self.empty_atlas)
        } else {
            &self.empty_atlas
        };
        let key = GroupKey {
            atlas: u64::from(shadows),
            env,
            buffers: self.generation,
            grid,
            features: self.features.group_key(&self.slot(slot_id).features),
        };
        if self
            .slot(slot_id)
            .groups
            .as_ref()
            .is_some_and(|g| g.0 == key)
        {
            return;
        }
        let maps = self.env_maps(ri, env);
        let grid_view = self.grid_view(ri, grid);
        let slot = self.slot(slot_id);
        let device = shared.device();
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        let (decal_color, decal_normal) = self.features.atlas_views();
        let view = wgpu::BindingResource::TextureView;
        let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie scene frame"),
            layout: &self.layouts.frame,
            entries: &[
                entry(0, buf(&slot.frame)),
                entry(1, buf(&slot.lights.buffer)),
                entry(2, buf(&slot.clusters.buffer)),
                entry(3, buf(&slot.indices.buffer)),
                entry(4, buf(&slot.materials.buffer)),
                entry(5, buf(&slot.joints.buffer)),
                entry(6, buf(&slot.morph_weights.buffer)),
                entry(7, wgpu::BindingResource::TextureView(&atlas.sample_view)),
                entry(8, wgpu::BindingResource::Sampler(&self.shadow_sampler)),
                entry(9, wgpu::BindingResource::TextureView(&maps.specular)),
                entry(10, wgpu::BindingResource::TextureView(&maps.irradiance)),
                entry(11, wgpu::BindingResource::Sampler(&self.linear_sampler)),
                entry(20, buf(&slot.probes)),
                entry(21, view(&self.probes.array_view)),
                entry(22, view(grid_view)),
                entry(
                    SCENE_COPY_BINDING,
                    view(self.features.copy_view(&slot.features)),
                ),
                entry(DECALS_BINDING, buf(&slot.features.decals.buffer)),
                entry(DECAL_COLOR_BINDING, view(decal_color)),
                entry(DECAL_NORMAL_BINDING, view(decal_normal)),
            ],
        });
        let shadow = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie shadow frame"),
            layout: &self.layouts.shadow_frame,
            entries: &[
                entry(0, buf(&slot.frame)),
                entry(4, buf(&slot.materials.buffer)),
                entry(5, buf(&slot.joints.buffer)),
                entry(6, buf(&slot.morph_weights.buffer)),
            ],
        });
        match slot_id {
            SlotId::Scene(i) => self.slots[i].groups = Some((key, frame, shadow)),
            SlotId::Capture(i) => self.captures[i].groups = Some((key, frame, shadow)),
        }
    }
}
