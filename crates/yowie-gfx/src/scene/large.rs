//! The draw classes of large outdoor scenes: terrain, water, billboards
//! and impostors, and mesh draws dithered out for cross-fades. Each has
//! its own pipelines and draws inside the scene passes:
//!
//! - `render.rs` calls [`SceneRenderer::prepare_large`] after a scene
//!   slot is prepared, which culls each class against the camera and the
//!   shadow views and uploads its instances into the slot's buffers.
//!   Probe captures skip the classes.
//! - `passes.rs` calls [`SceneRenderer::draw_large_opaque`] at the start of
//!   the HDR pass, [`SceneRenderer::draw_large_blend`] after the blended
//!   meshes, and [`SceneRenderer::draw_large_shadows`] after the mesh
//!   casters of the shadow pass.
//!
//! The classes share one shader module, built from the scene's shared
//! declarations and lighting (`common`, `atmosphere`, `vertex`,
//! `surface`) followed by their own files. Their resources sit at
//! bindings 32 and up of group 1, clear of the material bindings.

use yowie_core::Mat4;

use super::billboard::{self, BillboardRecord};
use super::bounds::Frustum;
use super::fade::{self, FadeDraw};
use super::label::LabelDraw;
use super::large_pipelines::LargePipelines;
use super::prepare::Prepared;
use super::render::SceneRenderer;
use super::slot::{GrowBuffer, SlotId};
use super::terrain::{self, TerrainDraw};
use super::water::{self, WaterDraw};
use crate::graphics::FrameStats;
use crate::shared::Shared;

/// What one scene recorded for the large-scene classes.
#[derive(Debug, Default)]
pub(crate) struct LargeRecord {
    /// Game time in seconds when the scene was recorded.
    pub(crate) time: f32,
    pub(crate) terrains: Vec<TerrainDraw>,
    pub(crate) waters: Vec<WaterDraw>,
    pub(crate) billboards: BillboardRecord,
    pub(crate) fades: Vec<FadeDraw>,
    pub(crate) labels: Vec<LabelDraw>,
    /// Draws picked from a level of detail, by level (the last counts
    /// levels 3 and up).
    pub(crate) lod_levels: [u32; 4],
    pub(crate) impostors: u32,
}

impl LargeRecord {
    /// Empties the record, keeping its allocations.
    pub(crate) fn clear(&mut self) {
        self.time = 0.0;
        self.terrains.clear();
        self.waters.clear();
        self.billboards.clear();
        self.fades.clear();
        self.labels.clear();
        self.lod_levels = [0; 4];
        self.impostors = 0;
    }

    fn has_gpu_work(&self) -> bool {
        !self.terrains.is_empty()
            || !self.waters.is_empty()
            || !self.billboards.is_empty()
            || !self.fades.is_empty()
    }
}

/// The per-slot buffers and batches of the large-scene classes.
#[derive(Debug)]
pub(crate) struct LargeSlot {
    pub(crate) terrain: terrain::render::TerrainFrame,
    pub(crate) billboards: billboard::BillboardFrame,
    pub(crate) fades: fade::FadeFrame,
}

impl LargeSlot {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            terrain: terrain::render::TerrainFrame::new(device),
            billboards: billboard::BillboardFrame::new(device),
            fades: fade::FadeFrame::new(device),
        }
    }
}

/// The pipelines, samplers and slots of the large-scene classes, created
/// with the first scene that uses one.
#[derive(Debug)]
pub(crate) struct LargeRenderer {
    pub(crate) pipelines: LargePipelines,
    /// Repeating, filtered, mipmapped: terrain layers.
    pub(crate) repeat_sampler: wgpu::Sampler,
    /// Clamped and filtered: terrain normals and splat, atlases.
    pub(crate) clamp_sampler: wgpu::Sampler,
    /// One white sRGB texel, for a billboard style without a texture.
    pub(crate) white: wgpu::TextureView,
    /// One flat normal texel, for a style without a normal map.
    pub(crate) flat_normal: wgpu::TextureView,
    pub(crate) slots: Vec<LargeSlot>,
}

/// A 1x1 texture of `texel`.
fn texel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    texel: [u8; 4],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("yowie large texel"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        size,
    );
    tex.create_view(&Default::default())
}

impl LargeRenderer {
    pub(crate) fn new(shared: &Shared, layouts: &super::layouts::SceneLayouts) -> Self {
        let (device, queue) = (shared.device(), shared.queue());
        let sampler = |label, mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: mode,
                address_mode_v: mode,
                address_mode_w: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                anisotropy_clamp: 8,
                ..Default::default()
            })
        };
        Self {
            pipelines: LargePipelines::new(device, layouts, super::probes::cube_arrays(shared)),
            repeat_sampler: sampler("yowie large repeat", wgpu::AddressMode::Repeat),
            clamp_sampler: sampler("yowie large clamp", wgpu::AddressMode::ClampToEdge),
            white: texel(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb, [255; 4]),
            flat_normal: texel(
                device,
                queue,
                wgpu::TextureFormat::Rgba8Unorm,
                [128, 128, 255, 255],
            ),
            slots: Vec::new(),
        }
    }
}

/// A growable vertex buffer for instance records.
pub(crate) fn instance_buffer(device: &wgpu::Device, label: &'static str) -> GrowBuffer {
    GrowBuffer::new(device, label, wgpu::BufferUsages::VERTEX)
}

/// The frustum of each shadow view the scene renders, by view index.
fn shadow_frustums(p: &Prepared) -> Vec<(usize, Frustum)> {
    p.shadows
        .iter()
        .map(|(v, _)| {
            let m = Mat4::from_cols_array_2d(&p.frame.shadow_views[*v].view_proj);
            (*v, Frustum::from_view_proj(&m))
        })
        .collect()
}

impl SceneRenderer {
    /// The large-scene renderer, created on first use.
    pub(crate) fn large_renderer(&mut self, shared: &Shared) -> &mut LargeRenderer {
        let layouts = &self.layouts;
        self.large
            .get_or_insert_with(|| Box::new(LargeRenderer::new(shared, layouts)))
    }

    /// Culls and uploads slot `ri`'s large-scene draws, and counts them.
    pub(crate) fn prepare_large(
        &mut self,
        shared: &Shared,
        ri: usize,
        p: &Prepared,
        stats: &mut FrameStats,
    ) {
        let rec = &self.records[ri].large;
        for (s, n) in stats.lod_levels.iter_mut().zip(rec.lod_levels) {
            *s += n;
        }
        stats.impostors += rec.impostors;
        if !rec.has_gpu_work() {
            return;
        }
        self.large_renderer(shared);
        let Some(large) = self.large.as_deref_mut() else {
            return;
        };
        let rec = &self.records[ri];
        while large.slots.len() <= ri {
            large.slots.push(LargeSlot::new(shared.device()));
        }
        let view = Frustum::from_view_proj(&Mat4::from_cols_array_2d(&p.frame.view_proj));
        let shadows = shadow_frustums(p);
        let LargeRenderer {
            pipelines,
            repeat_sampler,
            clamp_sampler,
            white,
            flat_normal,
            slots,
        } = large;
        let slot = &mut slots[ri];
        let ctx = PrepareContext {
            shared,
            pipelines,
            repeat: repeat_sampler,
            clamp: clamp_sampler,
            white,
            flat_normal,
            view: &view,
            shadows: &shadows,
            eye: yowie_core::Vec3::from_slice(&p.frame.camera[..3]),
        };
        terrain::render::prepare(&ctx, &rec.large, &mut slot.terrain, stats);
        water::prepare(&ctx, &rec.large);
        billboard::prepare(&ctx, &rec.large, &mut slot.billboards, stats);
        fade::prepare(&ctx, rec, &mut slot.fades);
    }

    /// Records the opaque large-scene draws of a scene slot: terrain,
    /// cut-out billboards and dithered meshes. Leaves the pipeline, group
    /// 1 and vertex buffers changed.
    pub(crate) fn draw_large_opaque(&self, pass: &mut wgpu::RenderPass<'_>, id: SlotId) -> u32 {
        let Some((ri, large, slot)) = self.large_slot(id) else {
            return 0;
        };
        let rec = &self.records[ri];
        let mut calls = terrain::render::encode(&large.pipelines, &slot.terrain, pass);
        calls += billboard::encode(&large.pipelines, &slot.billboards, pass, false);
        calls += fade::encode(self, &large.pipelines, rec, &slot.fades, pass);
        calls
    }

    /// Records the blended large-scene draws of a scene slot: water and
    /// blended billboards.
    pub(crate) fn draw_large_blend(&self, pass: &mut wgpu::RenderPass<'_>, id: SlotId) -> u32 {
        let Some((ri, large, slot)) = self.large_slot(id) else {
            return 0;
        };
        let rec = &self.records[ri].large;
        let mut calls = water::encode(&large.pipelines, rec, pass);
        calls += billboard::encode(&large.pipelines, &slot.billboards, pass, true);
        calls
    }

    /// Records the large-scene shadow casters of a scene slot into every
    /// shadow view, setting each view's viewport.
    pub(crate) fn draw_large_shadows(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        id: SlotId,
        p: &Prepared,
    ) -> u32 {
        let Some((_, large, slot)) = self.large_slot(id) else {
            return 0;
        };
        let mut calls = 0;
        for (view, _) in &p.shadows {
            let (x, y, s) = super::shadow::atlas_rect(*view);
            pass.set_viewport(x as f32, y as f32, s as f32, s as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, s, s);
            calls += terrain::render::encode_shadow(&large.pipelines, &slot.terrain, pass, *view);
            calls += billboard::encode_shadow(&large.pipelines, &slot.billboards, pass, *view);
        }
        calls
    }

    /// The record index, renderer and buffers a scene slot draws its
    /// large-scene classes from; `None` for probe captures, which leave
    /// them out, and for scenes without any.
    fn large_slot(&self, id: SlotId) -> Option<(usize, &LargeRenderer, &LargeSlot)> {
        let SlotId::Scene(ri) = id else {
            return None;
        };
        let large = self.large.as_deref()?;
        if !self.records.get(ri)?.large.has_gpu_work() {
            return None;
        }
        Some((ri, large, large.slots.get(ri)?))
    }
}

/// What the classes' preparation reads.
pub(crate) struct PrepareContext<'a> {
    pub(crate) shared: &'a Shared,
    pub(crate) pipelines: &'a LargePipelines,
    pub(crate) repeat: &'a wgpu::Sampler,
    pub(crate) clamp: &'a wgpu::Sampler,
    pub(crate) white: &'a wgpu::TextureView,
    pub(crate) flat_normal: &'a wgpu::TextureView,
    pub(crate) view: &'a Frustum,
    pub(crate) shadows: &'a [(usize, Frustum)],
    pub(crate) eye: yowie_core::Vec3,
}
