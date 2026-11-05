//! The light probe grid: baked diffuse irradiance at points on a 3D grid.

use std::sync::{Arc, OnceLock};

use yowie_core::{UVec3, Vec3};

use super::sh::{self, CELL_FLOATS};
use crate::error::{GfxError, Result};
use crate::half::f32_to_f16;
use crate::shared::Shared;
use crate::texture::next_id;

/// The most cells a grid may have.
pub const MAX_LIGHT_PROBES: u32 = 65_536;

/// Diffuse light baked at the points of a 3D grid, so surfaces between
/// them pick up the colour of the walls and lights around them.
///
/// Bake it with
/// [`Graphics::bake_light_probes`](crate::Graphics::bake_light_probes)
/// (one small cube capture per cell), or load saved data with
/// [`LightProbeGrid::from_bytes`]; light a scene with it through
/// [`Scene::set_light_probes`](crate::scene::Scene::set_light_probes).
/// Inside the grid, and over half a cell past its outer cells, it replaces
/// the environment's or ambient colour's diffuse light; specular light
/// still comes from reflection probes or the environment. Place cells in
/// open space: a cell inside a wall captures darkness and dims its
/// neighbours, and light filters between neighbouring cells whatever
/// stands between them. Cloning shares the baked data.
#[derive(Clone, Debug)]
pub struct LightProbeGrid {
    /// The position of cell (0, 0, 0), in world units.
    pub origin: Vec3,
    /// The distance between neighbouring cells along each axis.
    pub spacing: Vec3,
    /// The number of cells along each axis, each at least 1.
    pub counts: UVec3,
    /// Multiplies the grid's light.
    pub intensity: f32,
    /// Each cell capture's face size in texels; 16 by default.
    pub resolution: u32,
    /// The capture cameras' near distance in world units.
    pub near: f32,
    /// The capture cameras' far distance in world units.
    pub far: f32,
    pub(crate) baked: Option<Arc<GridBaked>>,
}

/// The coefficients of a baked grid and their texture.
#[derive(Debug)]
pub(crate) struct GridBaked {
    pub(crate) id: u64,
    /// The cell counts the data has.
    pub(crate) counts: UVec3,
    /// Per cell, x fastest, then y, then z.
    pub(crate) cells: Vec<[f32; CELL_FLOATS]>,
    gpu: OnceLock<wgpu::TextureView>,
}

/// The grid a scene is lit by.
pub(crate) type GridUse = LightProbeGrid;

impl LightProbeGrid {
    /// A grid of `counts` cells `spacing` apart starting at `origin`, with
    /// 16 texel captures.
    pub fn new(origin: Vec3, spacing: Vec3, counts: UVec3) -> Self {
        Self {
            origin,
            spacing,
            counts,
            intensity: 1.0,
            resolution: 16,
            near: 0.05,
            far: 1000.0,
            baked: None,
        }
    }

    /// The number of cells.
    pub fn cell_count(&self) -> usize {
        self.counts.x as usize * self.counts.y as usize * self.counts.z as usize
    }

    /// The world position of cell (`x`, `y`, `z`).
    pub fn cell_position(&self, x: u32, y: u32, z: u32) -> Vec3 {
        self.origin + self.spacing * Vec3::new(x as f32, y as f32, z as f32)
    }

    /// Every cell's position, x fastest, then y, then z: the order baked
    /// data is stored in. To see the cells, call
    /// [`Scene::draw_light_probes`](crate::scene::Scene::draw_light_probes).
    pub fn cell_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        let c = self.counts;
        (0..c.z).flat_map(move |z| {
            (0..c.y).flat_map(move |y| (0..c.x).map(move |x| self.cell_position(x, y, z)))
        })
    }

    /// Whether the grid holds baked data for its current cell counts.
    pub fn is_baked(&self) -> bool {
        self.data().is_some()
    }

    /// Cell (`x`, `y`, `z`)'s nine spherical harmonic coefficients (L2,
    /// linear RGB, already convolved with the cosine lobe), or `None` when
    /// unbaked or out of range.
    pub fn coefficients(&self, x: u32, y: u32, z: u32) -> Option<[Vec3; 9]> {
        let d = self.data()?;
        if x >= d.counts.x || y >= d.counts.y || z >= d.counts.z {
            return None;
        }
        let cell = &d.cells[self.index(x, y, z)];
        Some(std::array::from_fn(|i| {
            Vec3::new(cell[i * 3], cell[i * 3 + 1], cell[i * 3 + 2])
        }))
    }

    /// The baked irradiance over pi (what a white matte surface reflects,
    /// linear RGB times `intensity`) at `position` for a surface facing
    /// unit `normal`, filtered between the eight nearest cells as the
    /// renderer does. `None` when unbaked; positions outside the grid use
    /// the nearest cells. Use it to light sprites or particles to match.
    pub fn irradiance(&self, position: Vec3, normal: Vec3) -> Option<Vec3> {
        let d = self.data()?;
        let last = (d.counts - UVec3::ONE).as_vec3();
        let g = ((position - self.origin) / self.spacing.max(Vec3::splat(1e-4)))
            .clamp(Vec3::ZERO, last);
        let i0 = g.floor().as_uvec3();
        let f = g - i0.as_vec3();
        let i1 = (i0 + UVec3::ONE).min(d.counts - UVec3::ONE);
        let mut cell = [0.0f32; CELL_FLOATS];
        for corner in 0..8u32 {
            let pick = |bit: u32, a: u32, b: u32| if corner & bit != 0 { b } else { a };
            let x = pick(1, i0.x, i1.x);
            let y = pick(2, i0.y, i1.y);
            let z = pick(4, i0.z, i1.z);
            let w = |bit: u32, t: f32| if corner & bit != 0 { t } else { 1.0 - t };
            let weight = w(1, f.x) * w(2, f.y) * w(4, f.z);
            for (o, v) in cell.iter_mut().zip(&d.cells[self.index(x, y, z)]) {
                *o += v * weight;
            }
        }
        Some(sh::eval(&cell, normal) * self.intensity)
    }

    /// Checks that the grid can be baked on a device whose 3D textures
    /// reach `max_3d` texels.
    pub(crate) fn validate(&self, max_3d: u32) -> Result<()> {
        let c = self.counts;
        if c.min_element() == 0 {
            return Err(GfxError::Probes(format!(
                "cell counts {c} must be at least 1"
            )));
        }
        if (self.cell_count() as u64) > u64::from(MAX_LIGHT_PROBES) {
            return Err(GfxError::Probes(format!(
                "{} cells is more than {MAX_LIGHT_PROBES}",
                self.cell_count()
            )));
        }
        if c.x > max_3d || c.y > max_3d || c.z.saturating_mul(7) > max_3d {
            return Err(GfxError::Probes(format!(
                "cell counts {c} do not fit this device's 3D textures ({max_3d} texels, z times 7)"
            )));
        }
        if self.spacing.min_element() <= 0.0 || !self.spacing.is_finite() {
            return Err(GfxError::Probes(format!(
                "spacing {} must be above zero",
                self.spacing
            )));
        }
        Ok(())
    }

    /// Keeps freshly baked or loaded coefficients, one cell per entry in
    /// storage order.
    pub(crate) fn set_cells(&mut self, cells: Vec<[f32; CELL_FLOATS]>) {
        self.baked = Some(Arc::new(GridBaked {
            id: next_id(),
            counts: self.counts,
            cells,
            gpu: OnceLock::new(),
        }));
    }

    /// The baked data, when it matches the current counts.
    pub(crate) fn data(&self) -> Option<&Arc<GridBaked>> {
        self.baked.as_ref().filter(|d| d.counts == self.counts)
    }

    fn index(&self, x: u32, y: u32, z: u32) -> usize {
        let c = self.counts;
        ((z as usize * c.y as usize) + y as usize) * c.x as usize + x as usize
    }
}

impl GridBaked {
    /// The grid texture, uploaded the first time a scene uses it: a 3D
    /// `Rgba16Float` texture of `x` by `y` by `7 z` texels, coefficient
    /// float `k` of a cell in slab `k / 4`, channel `k % 4`.
    pub(crate) fn view(&self, shared: &Shared) -> &wgpu::TextureView {
        self.gpu.get_or_init(|| {
            let c = self.counts;
            let texture = shared.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("yowie light probes"),
                size: wgpu::Extent3d {
                    width: c.x,
                    height: c.y,
                    depth_or_array_layers: c.z * 7,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let cells = (c.x * c.y * c.z) as usize;
            let slab = cells * 4;
            let mut texels = vec![0u16; slab * 7];
            for (i, cell) in self.cells.iter().enumerate().take(cells) {
                for (k, v) in cell.iter().enumerate() {
                    texels[(k / 4) * slab + i * 4 + k % 4] = f32_to_f16(*v);
                }
            }
            shared.queue().write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&texels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(c.x * 8),
                    rows_per_image: Some(c.y),
                },
                texture.size(),
            );
            texture.create_view(&Default::default())
        })
    }

    /// The view if it was uploaded.
    pub(crate) fn uploaded(&self) -> Option<&wgpu::TextureView> {
        self.gpu.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_stored_x_fastest() {
        let mut g = LightProbeGrid::new(Vec3::ZERO, Vec3::new(2.0, 1.0, 1.0), UVec3::new(3, 2, 2));
        let p: Vec<Vec3> = g.cell_positions().collect();
        assert_eq!(p.len(), 12);
        assert_eq!(p[1], Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(p[3], Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(p[6], Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(g.index(1, 1, 1), 10);
        // A cell glowing red at x = 2 and black elsewhere.
        let cells = (0..12)
            .map(|i| {
                let mut c = [0.0; CELL_FLOATS];
                if i % 3 == 2 {
                    c[0] = 1.0 / 0.282_095;
                }
                c
            })
            .collect();
        g.set_cells(cells);
        assert!(g.is_baked());
        let at = |x: f32| g.irradiance(Vec3::new(x, 0.5, 0.5), Vec3::Y).map(|e| e.x);
        assert!((at(4.0).unwrap_or(0.0) - 1.0).abs() < 1e-4);
        assert!((at(3.0).unwrap_or(0.0) - 0.5).abs() < 1e-4);
        assert!(at(0.0).unwrap_or(1.0).abs() < 1e-4);
        g.counts.x = 4;
        assert!(!g.is_baked(), "data for other counts is ignored");
    }

    #[test]
    fn validation_rejects_bad_grids() {
        let ok = LightProbeGrid::new(Vec3::ZERO, Vec3::ONE, UVec3::new(4, 2, 4));
        assert!(ok.validate(2048).is_ok());
        let cases = [
            LightProbeGrid::new(Vec3::ZERO, Vec3::ONE, UVec3::new(0, 2, 4)),
            LightProbeGrid::new(Vec3::ZERO, Vec3::ONE, UVec3::new(1, 1, 300)),
            LightProbeGrid::new(Vec3::ZERO, Vec3::ONE, UVec3::new(512, 512, 1)),
            LightProbeGrid::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0), UVec3::ONE),
        ];
        for g in cases {
            assert!(g.validate(2048).is_err(), "{:?}", g.counts);
        }
    }
}
