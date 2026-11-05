//! What the renderer keeps for probes between frames: the cube array the
//! scenes' probes are copied into, the stand-in grid texture, the capture
//! depth target, and writing each view's probe block.

use std::sync::Arc;

use yowie_core::UVec2;

use super::reflection::ProbeMaps;
use super::{Capture, MAX_REFLECTION_PROBES, ProbeBlock, ProbeGpu};
use crate::scene::layouts::HDR_FORMAT;
use crate::scene::render::SceneRenderer;
use crate::scene::slot::SlotId;
use crate::scene::targets::depth_views;
use crate::shared::Shared;

/// Face size of a prefiltered probe cube.
pub(crate) const PROBE_SIZE: u32 = 128;
/// Mips of a prefiltered probe cube, roughness 0 to 1.
pub(crate) const PROBE_MIPS: u32 = 6;

/// A probe capture copied into a cube array slot.
#[derive(Clone, Copy, Debug)]
struct Resident {
    generation: u64,
    /// The view stamp that last used it.
    used: u64,
}

/// The probe resources of a scene renderer. On adapters without cube
/// arrays the array is one cube and holds one probe.
#[derive(Debug)]
pub(crate) struct ProbeState {
    array: wgpu::Texture,
    /// The array (or single cube) view at group 0, binding 21.
    pub(crate) array_view: wgpu::TextureView,
    slots: Vec<Option<Resident>>,
    /// Counts views, so a view never evicts a probe it uses.
    stamp: u64,
    /// A black one-cell grid for scenes without one.
    pub(crate) no_grid: wgpu::TextureView,
    /// The capture depth target's size and its sampled and attached views.
    depth: Option<(u32, wgpu::TextureView, wgpu::TextureView)>,
}

impl ProbeState {
    pub(crate) fn new(shared: &Shared, cube_array: bool) -> Self {
        let device = shared.device();
        let slots = if cube_array { MAX_REFLECTION_PROBES } else { 1 };
        let array = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie reflection probes"),
            size: wgpu::Extent3d {
                width: PROBE_SIZE,
                height: PROBE_SIZE,
                depth_or_array_layers: 6 * slots as u32,
            },
            mip_level_count: PROBE_MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let array_view = array.create_view(&wgpu::TextureViewDescriptor {
            label: Some("yowie reflection probes"),
            dimension: Some(if cube_array {
                wgpu::TextureViewDimension::CubeArray
            } else {
                wgpu::TextureViewDimension::Cube
            }),
            ..Default::default()
        });
        let no_grid = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("yowie no light probes"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 7,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            array,
            array_view,
            slots: vec![None; slots],
            stamp: 0,
            no_grid,
            depth: None,
        }
    }

    /// The most probes a scene shows on this device.
    pub(crate) fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// The capture depth target `size` texels square in `format` (the
    /// scene's depth and stencil format): its depth-only view for sampling
    /// and its view of both aspects for attaching.
    pub(crate) fn capture_depth(
        &mut self,
        device: &wgpu::Device,
        size: u32,
        format: wgpu::TextureFormat,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        if let Some((s, d, ds)) = &self.depth
            && *s == size
        {
            return (d.clone(), ds.clone());
        }
        let (d, ds) = depth_views(device, "yowie capture depth", UVec2::splat(size), format, 1);
        self.depth = Some((size, d.clone(), ds.clone()));
        (d, ds)
    }

    /// The array slot holding `maps`, copying it in (into a free slot, or
    /// the one used longest ago by another view) when it is not there.
    fn resident(&mut self, encoder: &mut wgpu::CommandEncoder, maps: &ProbeMaps) -> u32 {
        let stamp = self.stamp;
        if let Some(i) = self
            .slots
            .iter()
            .position(|s| s.is_some_and(|r| r.generation == maps.generation))
        {
            if let Some(r) = &mut self.slots[i] {
                r.used = stamp;
            }
            return i as u32;
        }
        let pick = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_none_or(|r| r.used != stamp))
            .min_by_key(|(_, s)| s.map_or(0, |r| r.used + 1))
            .map_or(0, |(i, _)| i);
        for mip in 0..PROBE_MIPS {
            let size = PROBE_SIZE >> mip;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &maps.texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.array,
                    mip_level: mip,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: 6 * pick as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 6,
                },
            );
        }
        self.slots[pick] = Some(Resident {
            generation: maps.generation,
            used: stamp,
        });
        pick as u32
    }
}

impl SceneRenderer {
    /// Writes `slot`'s probe block for record `ri`: the record's baked
    /// probes (none in captures), copied into the cube array as needed,
    /// smallest box first; and its light probe grid (none when capturing
    /// the grid). Returns the grid's id for the bind group key, 0 for none.
    pub(crate) fn write_probes(
        &mut self,
        shared: &Shared,
        encoder: &mut wgpu::CommandEncoder,
        ri: usize,
        slot: SlotId,
        capture: Option<Capture>,
    ) -> u64 {
        let rec = &self.records[ri];
        let mut block = ProbeBlock {
            info: [0.0, PROBE_MIPS as f32, 0.0, 0.0],
            ..ProbeBlock::default()
        };
        let mut shown: Vec<(ProbeGpu, Arc<ProbeMaps>, f32)> = Vec::new();
        if capture.is_none() {
            for u in &rec.probes {
                let p = &u.probe;
                if let Some(maps) = p.maps()
                    && !p.bounds.is_empty()
                {
                    let gpu = ProbeGpu {
                        center: p.position.extend(p.intensity).to_array(),
                        box_min: p.bounds.min.extend(p.blend_distance.max(0.0)).to_array(),
                        box_max: p.bounds.max.extend(0.0).to_array(),
                        params: [0.0, f32::from(u8::from(p.box_projection)), 0.0, 0.0],
                    };
                    shown.push((gpu, maps, p.volume()));
                }
            }
        }
        shown.truncate(self.probes.capacity());
        shown.sort_by(|a, b| a.2.total_cmp(&b.2));
        self.probes.stamp += 1;
        for (i, (mut gpu, maps, _)) in shown.into_iter().enumerate() {
            gpu.params[0] = self.probes.resident(encoder, &maps) as f32;
            block.list[i] = gpu;
            block.info[0] = (i + 1) as f32;
        }
        let rec = &self.records[ri];
        let mut grid_id = 0;
        if capture != Some(Capture::Grid)
            && let Some(g) = &rec.light_grid
            && let Some(d) = g.data()
        {
            d.view(shared);
            grid_id = d.id;
            block.grid_origin = g.origin.extend(g.intensity.max(1e-6)).to_array();
            block.grid_spacing = g.spacing.extend(0.0).to_array();
            block.grid_counts = g.counts.as_vec3().extend(0.0).to_array();
        }
        shared
            .queue()
            .write_buffer(&self.slot(slot).probes, 0, bytemuck::bytes_of(&block));
        grid_id
    }

    /// The grid texture slot bindings name for record `ri` under bind
    /// group key `grid` (0 for the stand-in).
    pub(crate) fn grid_view(&self, ri: usize, grid: u64) -> &wgpu::TextureView {
        self.records[ri]
            .light_grid
            .as_ref()
            .and_then(|g| g.data())
            .filter(|d| d.id == grid && grid != 0)
            .and_then(|d| d.uploaded())
            .unwrap_or(&self.probes.no_grid)
    }
}
