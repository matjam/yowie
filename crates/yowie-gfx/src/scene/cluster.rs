//! Clustered light assignment: the view is cut into a grid of screen tiles
//! and exponential depth slices, and each light is listed in the clusters
//! its sphere reaches, so a fragment loops over its own cluster's lights.

use yowie_core::{Mat4, Vec2, Vec3, Vec4};

/// The most point and spot lights a scene keeps; later ones are dropped
/// and counted in [`FrameStats::lights_dropped`](crate::FrameStats).
pub const MAX_LIGHTS: usize = 1024;
/// Tiles across the view.
const TILES_X: u32 = 16;
/// Tiles down the view.
const TILES_Y: u32 = 9;
/// Depth slices from the near to the far plane.
const SLICES: u32 = 24;
/// The most lights one cluster lists.
const MAX_PER_CLUSTER: u32 = 255;

/// The cluster grid of one scene view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClusterGrid {
    tiles_x: u32,
    tiles_y: u32,
    tile: f32,
    near: f32,
    far: f32,
    size: Vec2,
}

impl ClusterGrid {
    pub(crate) fn new(size: Vec2, near: f32, far: f32) -> Self {
        let tile = (size.x / TILES_X as f32)
            .max(size.y / TILES_Y as f32)
            .ceil()
            .max(1.0);
        Self {
            tiles_x: ((size.x / tile).ceil() as u32).clamp(1, TILES_X),
            tiles_y: ((size.y / tile).ceil() as u32).clamp(1, TILES_Y),
            tile,
            near: near.max(1e-4),
            far: far.max(near * 1.001),
            size,
        }
    }

    pub(crate) fn len(&self) -> usize {
        (self.tiles_x * self.tiles_y * SLICES) as usize
    }

    /// The grid fields of the frame block: tiles, slices, tile size; and
    /// the depth mapping `slice = log(depth) * scale + bias`.
    pub(crate) fn uniforms(&self) -> ([f32; 4], [f32; 4]) {
        let ln = (self.far / self.near).ln();
        let scale = SLICES as f32 / ln;
        let bias = -(self.near.ln()) * scale;
        (
            [
                self.tiles_x as f32,
                self.tiles_y as f32,
                SLICES as f32,
                self.tile,
            ],
            [self.near, self.far, scale, bias],
        )
    }

    fn slice_depth(&self, k: u32) -> f32 {
        self.near * (self.far / self.near).powf(k as f32 / SLICES as f32)
    }

    fn slice_of(&self, depth: f32) -> u32 {
        let (_, d) = self.uniforms();
        ((depth.max(self.near).ln() * d[2] + d[3]).max(0.0) as u32).min(SLICES - 1)
    }
}

/// Fills `table` (one word per cluster: first index shifted left by 8,
/// count in the low byte) and `indices` from lights given as view-space
/// spheres (`xyz` centre, `w` radius). Returns how many cluster entries
/// were dropped because a cluster was full.
pub(crate) fn assign(
    grid: &ClusterGrid,
    proj: &Mat4,
    spheres: &[Vec4],
    table: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> u32 {
    let n = grid.len();
    table.clear();
    table.resize(n, 0);
    indices.clear();
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    for (li, s) in spheres.iter().enumerate() {
        let c = s.truncate();
        let r = s.w;
        let depth = -c.z;
        let (d0, d1) = ((depth - r).max(grid.near), (depth + r).min(grid.far));
        if d0 > d1 {
            continue;
        }
        let (k0, k1) = (grid.slice_of(d0), grid.slice_of(d1));
        for k in k0..=k1 {
            let z0 = grid.slice_depth(k).max(d0);
            let z1 = grid.slice_depth(k + 1).min(d1);
            if z0 > z1 {
                continue;
            }
            let dz = if depth < z0 {
                z0 - depth
            } else if depth > z1 {
                depth - z1
            } else {
                0.0
            };
            let rr2 = r * r - dz * dz;
            if rr2 <= 0.0 {
                continue;
            }
            let rr = rr2.sqrt();
            let Some((t0, t1)) = tile_range(grid, proj, c, rr, z0, z1) else {
                continue;
            };
            for ty in t0.y as u32..=t1.y as u32 {
                for tx in t0.x as u32..=t1.x as u32 {
                    let cell = (k * grid.tiles_y + ty) * grid.tiles_x + tx;
                    pairs.push((cell, li as u32));
                }
            }
        }
    }
    // Counting sort by cluster, keeping light order within a cluster.
    let mut counts = vec![0u32; n];
    let mut dropped = 0;
    for (cell, _) in &pairs {
        counts[*cell as usize] += 1;
    }
    let mut offset = 0u32;
    let mut starts = vec![0u32; n];
    for (i, count) in counts.iter().enumerate() {
        let kept = (*count).min(MAX_PER_CLUSTER);
        dropped += count - kept;
        starts[i] = offset;
        table[i] = (offset << 8) | kept;
        offset += kept;
    }
    indices.resize(offset.max(1) as usize, 0);
    let mut fill = vec![0u32; n];
    for (cell, light) in pairs {
        let c = cell as usize;
        if fill[c] < MAX_PER_CLUSTER {
            indices[(starts[c] + fill[c]) as usize] = light;
            fill[c] += 1;
        }
    }
    dropped
}

/// The tiles a view-space box covers: `x` and `y` within `rr` of the
/// centre, depth from `z0` to `z1`.
fn tile_range(
    grid: &ClusterGrid,
    proj: &Mat4,
    c: Vec3,
    rr: f32,
    z0: f32,
    z1: f32,
) -> Option<(Vec2, Vec2)> {
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    for z in [z0, z1] {
        for dx in [-rr, rr] {
            for dy in [-rr, rr] {
                let p = *proj * Vec4::new(c.x + dx, c.y + dy, -z, 1.0);
                let ndc = Vec2::new(p.x, p.y) / p.w.max(1e-6);
                let px = Vec2::new(
                    (ndc.x * 0.5 + 0.5) * grid.size.x,
                    (0.5 - ndc.y * 0.5) * grid.size.y,
                );
                lo = lo.min(px);
                hi = hi.max(px);
            }
        }
    }
    if hi.x < 0.0 || hi.y < 0.0 || lo.x >= grid.size.x || lo.y >= grid.size.y {
        return None;
    }
    let max = Vec2::new((grid.tiles_x - 1) as f32, (grid.tiles_y - 1) as f32);
    let t0 = (lo / grid.tile).floor().clamp(Vec2::ZERO, max);
    let t1 = (hi / grid.tile).floor().clamp(Vec2::ZERO, max);
    Some((t0, t1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cluster a light's sphere reaches by brute force lists the
    /// light: the per-slice fit is conservative.
    #[test]
    fn assignment_is_conservative() {
        let size = Vec2::new(640.0, 360.0);
        let (near, far) = (0.1, 100.0);
        let proj = yowie_core::glam::camera::rh::proj::directx::perspective(
            1.0,
            size.x / size.y,
            far,
            near,
        );
        let grid = ClusterGrid::new(size, near, far);
        let spheres = [
            Vec4::new(0.0, 0.0, -10.0, 3.0),
            Vec4::new(4.0, -1.0, -6.0, 1.5),
            Vec4::new(-2.0, 1.0, -30.0, 8.0),
            Vec4::new(0.0, 0.0, 1.0, 2.0),
        ];
        let (mut table, mut indices) = (Vec::new(), Vec::new());
        assign(&grid, &proj, &spheres, &mut table, &mut indices);
        let inv = proj.inverse();
        for k in 0..SLICES {
            for ty in 0..grid.tiles_y {
                for tx in 0..grid.tiles_x {
                    let cell = (k * grid.tiles_y + ty) * grid.tiles_x + tx;
                    let word = table[cell as usize];
                    let listed =
                        &indices[(word >> 8) as usize..((word >> 8) + (word & 0xff)) as usize];
                    // Sample points inside the cluster.
                    for (fx, fy, fz) in [(0.5, 0.5, 0.5), (0.1, 0.1, 0.1), (0.9, 0.9, 0.9)] {
                        let px =
                            Vec2::new((tx as f32 + fx) * grid.tile, (ty as f32 + fy) * grid.tile);
                        if px.x >= size.x || px.y >= size.y {
                            continue;
                        }
                        let depth = grid.slice_depth(k)
                            + (grid.slice_depth(k + 1) - grid.slice_depth(k)) * fz;
                        let ndc = Vec2::new(px.x / size.x * 2.0 - 1.0, 1.0 - px.y / size.y * 2.0);
                        let dir = inv * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
                        let dir = dir.truncate() / dir.w;
                        let p = dir * (depth / -dir.z);
                        for (i, s) in spheres.iter().enumerate() {
                            if (p - s.truncate()).length() < s.w {
                                assert!(
                                    listed.contains(&(i as u32)),
                                    "light {i} missing from cluster {tx},{ty},{k}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
