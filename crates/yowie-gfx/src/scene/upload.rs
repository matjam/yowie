//! Writing a prepared scene into its slot's buffers and building the
//! slot's frame bind groups.

use super::prepare::Prepared;
use super::render::SceneRenderer;
use super::slot::GroupKey;
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
    /// weights, instances and lines of slot `ri`.
    pub(super) fn upload(&mut self, shared: &Shared, ri: usize, p: &Prepared) {
        let (device, queue) = (shared.device(), shared.queue());
        let rec = &self.records[ri];
        let slot = &mut self.slots[ri];
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
        if grew {
            self.generation += 1;
        }
    }

    /// Builds slot `ri`'s frame and shadow-pass bind groups when a buffer,
    /// the environment or the shadow atlas changed.
    pub(super) fn bind_groups(&mut self, shared: &Shared, ri: usize, env: u64, shadows: bool) {
        let atlas = if shadows {
            self.atlas.as_ref().unwrap_or(&self.empty_atlas)
        } else {
            &self.empty_atlas
        };
        let key = GroupKey {
            atlas: u64::from(shadows),
            env,
            buffers: self.generation,
        };
        if self.slots[ri].groups.as_ref().is_some_and(|g| g.0 == key) {
            return;
        }
        let maps = self.env_maps(ri, env);
        let slot = &self.slots[ri];
        let device = shared.device();
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
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
        self.slots[ri].groups = Some((key, frame, shadow));
    }
}
