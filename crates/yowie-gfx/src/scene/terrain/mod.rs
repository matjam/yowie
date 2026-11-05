//! Chunked heightfield terrain: levels of detail per chunk, a splat map
//! blending four ground layers, height, normal and ray queries, and
//! editing with a brush.
//!
//! A [`Terrain`] keeps its heights on the processor for queries and on the
//! GPU in a storage buffer. Its chunks have no vertex buffers: the vertex
//! program builds each vertex from the chunk's corner, its level and the
//! vertex index, reading the height from the buffer, so every chunk at a
//! level shares one index buffer and a whole terrain draws in one call per
//! level. Each chunk carries a skirt around its edge, deep enough to hide
//! the crack where it meets a coarser neighbour. Normals come from a
//! texture at the full resolution, so lighting does not change when a
//! chunk changes level.

mod draw;
mod edit;
mod gpu;
mod heightmap;
mod query;
pub(crate) mod render;
mod splat;

use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use yowie_core::{Color, Vec3};

use super::bounds::Aabb;
use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::texture::Texture;

pub use draw::TerrainDrawOptions;
pub use edit::{Brush, BrushMode};
pub use heightmap::HeightMap;
pub use splat::{LayerRule, SplatRules};

pub(crate) use draw::TerrainDraw;
pub(crate) use gpu::TerrainGpu;

/// One ground layer: a tiling colour texture, its tint, how often it
/// repeats and how rough it is.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainLayer {
    /// The tiling albedo (sRGB). `None` uses the tint alone.
    pub texture: Option<Texture>,
    /// Multiplies the texture, sRGB.
    pub color: Color,
    /// World units per repeat of the texture.
    pub scale: f32,
    /// 0 for a mirror, 1 for fully matte.
    pub roughness: f32,
}

impl TerrainLayer {
    /// An untextured layer of `color`.
    pub fn color(color: Color, roughness: f32) -> Self {
        Self {
            texture: None,
            color,
            scale: 8.0,
            roughness,
        }
    }

    /// A layer showing `texture`, repeating every `scale` world units.
    pub fn textured(texture: &Texture, scale: f32, roughness: f32) -> Self {
        Self {
            texture: Some(texture.clone()),
            color: Color::WHITE,
            scale,
            roughness,
        }
    }
}

/// How a [`Terrain`] is laid out, refined and shaded.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainOptions {
    /// World units between samples.
    pub cell: f32,
    /// Where the middle of the grid sits; its `y` is added to every height.
    pub center: Vec3,
    /// Samples across one chunk: a power of two. `cols - 1` and `rows - 1`
    /// must be whole multiples of it.
    pub chunk_size: u32,
    /// Resolutions per chunk, each halving the samples of the one before;
    /// clamped to what the chunk size can be halved to.
    pub levels: u32,
    /// How far from the camera the finest level reaches, in world units;
    /// each level after covers twice the distance of the one before. Zero
    /// means two chunk widths.
    pub lod_distance: f32,
    /// How far past a level's distance (as a fraction of it) a chunk must
    /// move before it changes level, so a camera resting on a boundary
    /// does not make it flicker.
    pub hysteresis: f32,
    /// The four ground layers the splat map's red, green, blue and alpha
    /// weight: by convention sand, grass, rock and snow.
    pub layers: [TerrainLayer; 4],
    /// Paints the splat map from height and slope, and repaints the
    /// edited part after each edit. `None` starts with the first layer
    /// everywhere, for a game that paints its own map with
    /// [`Terrain::set_splat`].
    pub rules: Option<SplatRules>,
    /// Projects the layers from the sides as well as from above where
    /// the ground is steep, so cliffs do not show stretched texels.
    pub triplanar: bool,
}

impl Default for TerrainOptions {
    fn default() -> Self {
        Self {
            cell: 1.0,
            center: Vec3::ZERO,
            chunk_size: 32,
            levels: 4,
            lod_distance: 0.0,
            hysteresis: 0.1,
            layers: [
                TerrainLayer::color(Color::rgb8(194, 178, 128), 0.95),
                TerrainLayer::color(Color::rgb8(86, 125, 50), 0.9),
                TerrainLayer::color(Color::rgb8(110, 105, 100), 0.85),
                TerrainLayer::color(Color::rgb8(235, 240, 245), 0.6),
            ],
            rules: Some(SplatRules::default()),
            triplanar: true,
        }
    }
}

/// One chunk's world box (skirt included) and skirt depth.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChunkInfo {
    pub(crate) bounds: Aabb,
    pub(crate) skirt: f32,
}

/// What edits change.
#[derive(Debug)]
pub(crate) struct TerrainState {
    pub(crate) map: HeightMap,
    pub(crate) chunks: Vec<ChunkInfo>,
    pub(crate) rules: Option<SplatRules>,
    pub(crate) splat: Vec<u8>,
    pub(crate) layers: [TerrainLayer; 4],
    /// Bumped when a layer changes, so the bind group is rebuilt.
    pub(crate) generation: u64,
}

#[derive(Debug)]
pub(crate) struct TerrainInner {
    pub(crate) cols: u32,
    pub(crate) rows: u32,
    pub(crate) cell: f32,
    /// The world position of sample (0, 0) at height zero.
    pub(crate) origin: Vec3,
    pub(crate) chunk: u32,
    pub(crate) chunks_x: u32,
    pub(crate) chunks_z: u32,
    pub(crate) levels: u32,
    pub(crate) lod_distance: f32,
    pub(crate) hysteresis: f32,
    pub(crate) triplanar: bool,
    pub(crate) state: RwLock<TerrainState>,
    pub(crate) gpu: TerrainGpu,
    /// The level each chunk was last drawn at.
    pub(crate) chosen: Mutex<Vec<u8>>,
}

/// A chunked heightfield terrain; see the module documentation. Cloning
/// shares it.
///
/// ```no_run
/// # use yowie_gfx::{Graphics, scene::*};
/// # fn f(gfx: &mut Graphics, camera: &Camera3D) -> yowie_gfx::Result<()> {
/// let map = HeightMap::from_fn(129, 129, |x, z| ((x as f32) * 0.1).sin() * 2.0 + (z as f32) * 0.02);
/// let terrain = Terrain::new(gfx, map, TerrainOptions::default())?;
/// let ground = terrain.height(10.0, -4.0);
/// gfx.scene(camera, |s| s.draw_terrain(&terrain));
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct Terrain {
    pub(crate) inner: Arc<TerrainInner>,
}

impl Terrain {
    /// Builds a terrain from a height map: checks the chunk layout,
    /// paints the splat map and uploads the heights, normals and splat
    /// map. Fails when `cols - 1` or `rows - 1` is not a whole number of
    /// chunks or the chunk size is not a power of two.
    pub fn new(gfx: &Graphics, map: HeightMap, options: TerrainOptions) -> Result<Terrain> {
        let chunk = options.chunk_size;
        if chunk < 2 || !chunk.is_power_of_two() {
            return Err(GfxError::Terrain(format!(
                "chunk size {chunk} is not a power of two of at least 2"
            )));
        }
        let (cols, rows) = (map.cols(), map.rows());
        if !(cols - 1).is_multiple_of(chunk) || !(rows - 1).is_multiple_of(chunk) {
            return Err(GfxError::Terrain(format!(
                "a {cols} by {rows} grid is not a whole number of {chunk}-sample chunks plus one"
            )));
        }
        let cell = if options.cell > 0.0 {
            options.cell
        } else {
            1.0
        };
        let max_levels = chunk.trailing_zeros() + 1;
        let levels = options.levels.clamp(1, max_levels.min(8));
        let origin = Vec3::new(
            options.center.x - (cols - 1) as f32 * cell * 0.5,
            options.center.y,
            options.center.z - (rows - 1) as f32 * cell * 0.5,
        );
        let lod_distance = if options.lod_distance > 0.0 {
            options.lod_distance
        } else {
            chunk as f32 * cell * 2.0
        };
        let (chunks_x, chunks_z) = ((cols - 1) / chunk, (rows - 1) / chunk);
        let mut splat = vec![0u8; cols as usize * rows as usize * 4];
        match &options.rules {
            Some(r) => splat::paint(
                &map,
                r,
                origin.y,
                cell,
                [0, 0, cols - 1, rows - 1],
                &mut splat,
            ),
            None => splat
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p[0] = 255),
        }
        let gpu = TerrainGpu::new(&gfx.shared, &map, cell, chunk, levels, &splat);
        let inner = TerrainInner {
            cols,
            rows,
            cell,
            origin,
            chunk,
            chunks_x,
            chunks_z,
            levels,
            lod_distance,
            hysteresis: options.hysteresis.max(0.0),
            triplanar: options.triplanar,
            state: RwLock::new(TerrainState {
                map,
                chunks: Vec::new(),
                rules: options.rules,
                splat,
                layers: options.layers,
                generation: 0,
            }),
            gpu,
            chosen: Mutex::new(vec![0; (chunks_x * chunks_z) as usize]),
        };
        {
            let mut state = inner.state.write();
            state.chunks = (0..chunks_x * chunks_z)
                .map(|i| inner.chunk_info(&state.map, i))
                .collect();
            inner.gpu.write_params(&gfx.shared, &inner, &state);
        }
        Ok(Terrain {
            inner: Arc::new(inner),
        })
    }

    /// The world box the terrain fills, skirts included.
    pub fn bounds(&self) -> Aabb {
        self.inner
            .state
            .read()
            .chunks
            .iter()
            .fold(Aabb::EMPTY, |b, c| b.union(c.bounds))
    }

    /// Samples along X.
    pub fn cols(&self) -> u32 {
        self.inner.cols
    }

    /// Samples along Z.
    pub fn rows(&self) -> u32 {
        self.inner.rows
    }

    /// World units between samples.
    pub fn cell(&self) -> f32 {
        self.inner.cell
    }

    /// Where the middle of the grid sits; its `y` is the height that
    /// sample values are measured from.
    pub fn center(&self) -> Vec3 {
        let i = &self.inner;
        i.origin
            + Vec3::new(
                (i.cols - 1) as f32 * i.cell * 0.5,
                0.0,
                (i.rows - 1) as f32 * i.cell * 0.5,
            )
    }

    /// A copy of the height samples, for building a physics heightfield
    /// (`yowie_phys::Shape3D::grid_heightfield`) or saving an edited map.
    pub fn height_map(&self) -> HeightMap {
        self.inner.state.read().map.clone()
    }

    /// The number of chunks.
    pub fn chunk_count(&self) -> usize {
        (self.inner.chunks_x * self.inner.chunks_z) as usize
    }

    /// The world box of chunk `i` (row by row from the -X, -Z corner).
    pub fn chunk_bounds(&self, i: usize) -> Option<Aabb> {
        self.inner.state.read().chunks.get(i).map(|c| c.bounds)
    }

    /// The level chunk `i` was last drawn at: 0 is the finest.
    pub fn chunk_level(&self, i: usize) -> Option<u32> {
        self.inner.chosen.lock().get(i).map(|l| u32::from(*l))
    }

    /// The resolutions each chunk keeps.
    pub fn levels(&self) -> u32 {
        self.inner.levels
    }

    /// The level a chunk `distance` world units from the camera is drawn
    /// at, ignoring hysteresis.
    pub fn level_for_distance(&self, distance: f32) -> u32 {
        self.inner.level_for(distance)
    }
}

impl TerrainInner {
    /// World `x`, `z` in grid units.
    pub(crate) fn to_grid(&self, x: f32, z: f32) -> (f32, f32) {
        (
            (x - self.origin.x) / self.cell,
            (z - self.origin.z) / self.cell,
        )
    }

    /// The level for a distance: the finest out to `lod_distance`, then one
    /// coarser per doubling.
    pub(crate) fn level_for(&self, distance: f32) -> u32 {
        let mut level = 0;
        let mut reach = self.lod_distance;
        while level + 1 < self.levels && distance >= reach {
            level += 1;
            reach *= 2.0;
        }
        level
    }

    /// The first sample of chunk `i`.
    pub(crate) fn chunk_origin(&self, i: u32) -> (u32, u32) {
        (
            (i % self.chunks_x) * self.chunk,
            (i / self.chunks_x) * self.chunk,
        )
    }

    /// The world box and skirt depth of chunk `i`. The skirt is twice the
    /// farthest any coarser level's surface strays from the samples it
    /// skips, which covers the crack on both sides of a join.
    pub(crate) fn chunk_info(&self, map: &HeightMap, i: u32) -> ChunkInfo {
        let (x0, z0) = self.chunk_origin(i);
        let n = self.chunk;
        let mut worst = 0.0f32;
        for level in 1..self.levels {
            let step = 1u32 << level;
            for j in (0..n).step_by(step as usize) {
                for k in (0..n).step_by(step as usize) {
                    let (cx, cz) = (x0 + k, z0 + j);
                    for dj in 0..=step {
                        for dk in 0..=step {
                            let (u, v) = (dk as f32 / step as f32, dj as f32 / step as f32);
                            let flat = coarse_height(map, cx, cz, step, u, v);
                            let h = map.get(i64::from(cx + dk), i64::from(cz + dj));
                            worst = worst.max((h - flat).abs());
                        }
                    }
                }
            }
        }
        let skirt = worst * 2.0 + self.cell * 0.25;
        let (lo, hi) = map.range(x0, z0, x0 + n, z0 + n);
        let min = self.origin + Vec3::new(x0 as f32 * self.cell, lo - skirt, z0 as f32 * self.cell);
        let max =
            self.origin + Vec3::new((x0 + n) as f32 * self.cell, hi, (z0 + n) as f32 * self.cell);
        ChunkInfo {
            bounds: Aabb::new(min, max),
            skirt,
        }
    }
}

/// The height a coarse cell of `step` samples from `(x, z)` draws at
/// fractions `u`, `v` across it, split like the fine cells.
fn coarse_height(map: &HeightMap, x: u32, z: u32, step: u32, u: f32, v: f32) -> f32 {
    let g = |dx: u32, dz: u32| map.get(i64::from(x + dx), i64::from(z + dz));
    let (h00, h10, h01, h11) = (g(0, 0), g(step, 0), g(0, step), g(step, step));
    if u + v <= 1.0 {
        h00 + (h10 - h00) * u + (h01 - h00) * v
    } else {
        h11 + (h01 - h11) * (1.0 - u) + (h10 - h11) * (1.0 - v)
    }
}
