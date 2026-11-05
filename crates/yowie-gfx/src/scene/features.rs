//! The GPU state of the material features beside the core scene: the
//! transmission copy and the order-independent transparency targets of
//! each slot, the decal records and atlases, and the surface shaders'
//! uniform blocks.

use std::collections::HashMap;

use super::decals::DecalAtlases;
use super::layouts::HDR_FORMAT;
use super::prepare::Prepared;
use super::render::SceneRenderer;
use super::slot::{GrowBuffer, SlotId};
use super::targets::SceneFormat;
use super::transmission::{CopyPipelines, SceneCopy};
use super::transparency::{OitTargets, composite_pipeline};
use crate::shared::Shared;

/// A slot's feature targets and buffers.
#[derive(Debug)]
pub(crate) struct SlotFeatures {
    pub(crate) copy: Option<SceneCopy>,
    pub(crate) oit: Option<OitTargets>,
    pub(crate) decals: GrowBuffer,
    pub(crate) user: GrowBuffer,
    /// The uniform bind group and the buffer generation and binding size
    /// it was made for.
    pub(crate) user_group: Option<((u64, u64), wgpu::BindGroup)>,
    user_generation: u64,
    /// Each material's dynamic offset into `user`, or `u32::MAX`.
    pub(crate) user_offsets: Vec<u32>,
}

impl SlotFeatures {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        use wgpu::BufferUsages as U;
        Self {
            copy: None,
            oit: None,
            decals: GrowBuffer::new(device, "yowie decals", U::STORAGE),
            user: GrowBuffer::new(device, "yowie surface uniforms", U::UNIFORM),
            user_group: None,
            user_generation: 0,
            user_offsets: Vec::new(),
        }
    }
}

/// The renderer's feature pipelines, atlases and stand-in textures.
#[derive(Debug)]
pub(crate) struct Features {
    copy: Option<CopyPipelines>,
    /// The OIT composite pipeline for each scene format it drew into.
    composite: HashMap<SceneFormat, wgpu::RenderPipeline>,
    atlases: Option<DecalAtlases>,
    /// A 1x1 HDR texture bound where a scene has no copy.
    no_copy: wgpu::TextureView,
    /// A 1x1 colour texture bound where there are no decal atlases.
    no_atlas: wgpu::TextureView,
}

fn one_texel(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie feature stand-in"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl Features {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            copy: None,
            composite: HashMap::new(),
            atlases: None,
            no_copy: one_texel(device, HDR_FORMAT),
            no_atlas: one_texel(device, wgpu::TextureFormat::Rgba8Unorm),
        }
    }

    /// The decal atlases' views, or stand-ins before any decal was drawn.
    pub(crate) fn atlas_views(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        match &self.atlases {
            Some(a) => (a.color_view(), a.normal_view()),
            None => (&self.no_atlas, &self.no_atlas),
        }
    }

    /// A slot's copy view, or the stand-in.
    pub(crate) fn copy_view<'a>(&'a self, slot: &'a SlotFeatures) -> &'a wgpu::TextureView {
        slot.copy.as_ref().map_or(&self.no_copy, |c| &c.view)
    }

    /// What the frame bind group names from the features, so it rebuilds
    /// when that changes.
    pub(crate) fn group_key(&self, slot: &SlotFeatures) -> u64 {
        let copy = slot.copy.as_ref().map_or(0, |c| c.id);
        (copy << 1) | u64::from(self.atlases.is_some())
    }

    pub(crate) fn copy_pipelines(&self) -> Option<&CopyPipelines> {
        self.copy.as_ref()
    }

    /// The OIT composite pipeline for a pass of `format`, once made.
    pub(crate) fn composite(&self, format: SceneFormat) -> Option<&wgpu::RenderPipeline> {
        self.composite.get(&format)
    }
}

impl SceneRenderer {
    /// Makes what record `ri` drawn into `slot_id` needs this frame: the
    /// decal records (placing their images in the atlases), the
    /// transmission copy of the slot's colour, the OIT targets at its size
    /// and sample count with the composite for its format, and the
    /// surface shaders' uniform blocks. Probe captures get no transmission
    /// copy (`encode_scene` draws their glass without one) and prepare no
    /// OIT draws.
    pub(super) fn prepare_features(
        &mut self,
        shared: &Shared,
        ri: usize,
        slot_id: SlotId,
        p: &mut Prepared,
    ) {
        let device = shared.device();
        let frame = self.frame_number();
        let rec = &self.records[ri];
        if !rec.decals.is_empty() {
            let atlases = self
                .features
                .atlases
                .get_or_insert_with(|| DecalAtlases::new(shared));
            atlases.records(shared, &rec.decals, frame, &mut p.decals);
        } else {
            p.decals.clear();
        }
        p.frame.counts[2] = p.decals.len() as u32;
        let Some(targets) = self.slot(slot_id).targets.as_ref() else {
            return;
        };
        let (size, format, targets_id) = (targets.size, targets.format, targets.id);
        if !p.transmit.is_empty()
            && let SlotId::Scene(i) = slot_id
        {
            if self.features.copy.is_none() {
                self.features.copy = Some(CopyPipelines::new(device, &self.layouts));
            }
            let slot = &mut self.slots[i];
            if slot
                .features
                .copy
                .as_ref()
                .is_none_or(|c| c.source != targets_id)
            {
                let Some(t) = slot.targets.as_ref() else {
                    return;
                };
                // The copy reads the single-sample colour, which a
                // multisampled pass resolves into when it ends.
                slot.features.copy = Some(SceneCopy::new(
                    device,
                    &self.layouts,
                    &self.linear_sampler,
                    (t.id, &t.color),
                    size,
                ));
            }
        }
        if !p.oit.is_empty() || self.large_oit(slot_id) {
            if !self.features.composite.contains_key(&format) {
                let pipeline = composite_pipeline(device, &self.layouts, format);
                self.features.composite.insert(format, pipeline);
            }
            let stale = self
                .slot(slot_id)
                .features
                .oit
                .as_ref()
                .is_none_or(|o| o.size != size || o.samples != format.samples);
            if stale {
                let oit = OitTargets::new(
                    device,
                    &self.layouts,
                    &self.linear_sampler,
                    size,
                    format.samples,
                );
                self.slot_mut(slot_id).features.oit = Some(oit);
            }
        }
        self.upload_features(shared, ri, slot_id, p);
    }

    /// Writes the decal records and surface shader uniform blocks.
    fn upload_features(&mut self, shared: &Shared, ri: usize, slot_id: SlotId, p: &Prepared) {
        let (device, queue) = (shared.device(), shared.queue());
        let rec = &self.records[ri];
        let slot = match slot_id {
            SlotId::Scene(i) => &mut self.slots[i].features,
            SlotId::Capture(i) => &mut self.captures[i].features,
        };
        let decals: &[u8] = if p.decals.is_empty() {
            &[0; 112]
        } else {
            bytemuck::cast_slice(&p.decals)
        };
        if slot.decals.write(device, queue, decals) {
            self.generation += 1;
        }
        slot.user_offsets.clear();
        if rec.user_blocks.is_empty() {
            return;
        }
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(16);
        let size = rec
            .user_blocks
            .iter()
            .map(|b| (b.len() as u64).div_ceil(16) * 16)
            .max()
            .unwrap_or(16)
            .max(16);
        let mut bytes = Vec::new();
        let mut offsets = Vec::with_capacity(rec.user_blocks.len());
        for b in &rec.user_blocks {
            let at = (bytes.len() as u64).div_ceil(align) * align;
            bytes.resize(at as usize, 0);
            bytes.extend_from_slice(b);
            offsets.push(at as u32);
        }
        // The last block's binding reaches `size` bytes past its offset.
        let end = u64::from(*offsets.last().unwrap_or(&0)) + size;
        bytes.resize(end as usize, 0);
        if slot.user.write(device, queue, &bytes) {
            slot.user_generation += 1;
        }
        slot.user_offsets = rec
            .material_blocks
            .iter()
            .map(|&b| {
                if b == u32::MAX {
                    u32::MAX
                } else {
                    offsets[b as usize]
                }
            })
            .collect();
        let key = (slot.user_generation, size);
        if slot.user_group.as_ref().is_none_or(|(k, _)| *k != key) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie surface uniforms"),
                layout: &self.layouts.user,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &slot.user.buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(size),
                    }),
                }],
            });
            slot.user_group = Some((key, group));
        }
    }
}
