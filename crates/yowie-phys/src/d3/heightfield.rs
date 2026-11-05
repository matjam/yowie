//! Building a heightfield collider from a regular grid of samples, such
//! as a renderer's terrain.

use yowie_core::Vec3;

use super::shape::Shape3D;

impl Shape3D {
    /// A heightfield over a grid of `cols` samples along X and `rows`
    /// along Z, `cell` world units apart, with `heights` in row-major order
    /// (`heights[row * cols + col]`, world units). The shape is centred on
    /// the origin in X and Z, so place its body at the grid's centre.
    ///
    /// Each cell splits into two triangles along the diagonal from its
    /// (+X, -Z) corner to its (-X, +Z) corner, as `yowie_gfx`'s
    /// `Terrain` draws and samples it, so bodies rest on the surface the
    /// player sees. To collide with a terrain:
    ///
    /// ```ignore
    /// let map = terrain.height_map();
    /// let shape = Shape3D::grid_heightfield(map.cols(), map.rows(), terrain.cell(), map.heights().to_vec());
    /// let body = world.add_body(&BodyDesc3D::fixed().at(terrain.center()));
    /// world.add_collider(body, &ColliderDesc3D::new(shape))?;
    /// ```
    pub fn grid_heightfield(cols: u32, rows: u32, cell: f32, heights: Vec<f32>) -> Shape3D {
        let span = |n: u32| n.saturating_sub(1) as f32 * cell;
        Shape3D::Heightfield {
            rows: rows as usize,
            cols: cols as usize,
            heights,
            scale: Vec3::new(span(cols), 1.0, span(rows)),
        }
    }
}

#[cfg(test)]
mod tests {
    use yowie_core::Vec3;

    use super::*;
    use crate::QueryFilter;
    use crate::d3::{BodyDesc3D, ColliderDesc3D, World3D};

    /// The height of a grid at fractional sample coordinates, split along
    /// the anti-diagonal like the renderer's terrain.
    fn expected(heights: &[f32], cols: usize, gx: f32, gz: f32) -> f32 {
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        let (u, v) = (gx - i as f32, gz - j as f32);
        let h = |x: usize, z: usize| heights[z * cols + x];
        let (h00, h10, h01, h11) = (h(i, j), h(i + 1, j), h(i, j + 1), h(i + 1, j + 1));
        if u + v <= 1.0 {
            h00 + (h10 - h00) * u + (h01 - h00) * v
        } else {
            h11 + (h01 - h11) * (1.0 - u) + (h10 - h11) * (1.0 - v)
        }
    }

    #[test]
    fn grid_heightfield_matches_the_terrain_surface() {
        let (cols, rows, cell) = (4u32, 3u32, 2.0);
        let heights = vec![0.0, 1.0, 3.0, 2.0, 0.5, 4.0, 1.0, 0.0, 2.0, 0.0, 1.5, 3.0];
        let center = Vec3::new(10.0, -1.0, 5.0);
        let mut world = World3D::default();
        let body = world.add_body(&BodyDesc3D::fixed().at(center));
        let shape = Shape3D::grid_heightfield(cols, rows, cell, heights.clone());
        world
            .add_collider(body, &ColliderDesc3D::new(shape))
            .expect("collider");
        world.refresh_queries();
        let corner = center - Vec3::new(3.0 * cell, 0.0, 2.0 * cell) * 0.5;
        // Points on both triangles of several cells.
        for (gx, gz) in [(0.2, 0.3), (0.8, 0.7), (1.5, 0.2), (2.6, 1.9), (1.1, 1.8)] {
            let at = corner + Vec3::new(gx * cell, 50.0, gz * cell);
            let hit = world
                .ray_cast(at, Vec3::NEG_Y, 100.0, &QueryFilter::new())
                .expect("hit");
            let want = center.y + expected(&heights, cols as usize, gx, gz);
            assert!(
                (hit.point.y - want).abs() < 1e-3,
                "({gx}, {gz}): {} vs {want}",
                hit.point.y
            );
        }
    }
}
