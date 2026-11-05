//! A grid of heights on the processor: building one from a function or a
//! greyscale image, and the height, normal and ray queries a terrain
//! answers from it.

use image::RgbaImage;
use yowie_core::Vec3;

use crate::error::{GfxError, Result};

/// A grid of heights: `cols` samples along X by `rows` along Z, stored row
/// by row, so `heights()[z * cols + x]` is the sample at column `x` of row
/// `z`. Heights are world units; a [`Terrain`](super::Terrain) places the
/// grid in the world and spaces the samples.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightMap {
    cols: u32,
    rows: u32,
    heights: Vec<f32>,
}

impl HeightMap {
    /// A grid from `cols * rows` heights, row by row. Fails when the count
    /// does not match or either side has fewer than two samples.
    pub fn new(cols: u32, rows: u32, heights: Vec<f32>) -> Result<Self> {
        if cols < 2 || rows < 2 || heights.len() != cols as usize * rows as usize {
            return Err(GfxError::Terrain(format!(
                "a {cols} by {rows} height map needs {} heights and at least 2 by 2, got {}",
                cols as usize * rows as usize,
                heights.len()
            )));
        }
        Ok(Self {
            cols,
            rows,
            heights,
        })
    }

    /// A grid with every sample at `height`. Sides below two are raised to
    /// two.
    pub fn flat(cols: u32, rows: u32, height: f32) -> Self {
        let (cols, rows) = (cols.max(2), rows.max(2));
        Self {
            cols,
            rows,
            heights: vec![height; cols as usize * rows as usize],
        }
    }

    /// A grid whose sample at column `x`, row `z` is `f(x, z)`. Sides below
    /// two are raised to two.
    pub fn from_fn(cols: u32, rows: u32, mut f: impl FnMut(u32, u32) -> f32) -> Self {
        let mut map = Self::flat(cols, rows, 0.0);
        for z in 0..map.rows {
            for x in 0..map.cols {
                map.heights[(z * map.cols + x) as usize] = f(x, z);
            }
        }
        map
    }

    /// A grid from an image's brightness, one sample per pixel: black is
    /// `low` and white is `high`. The image's top row is row 0 (the
    /// terrain's -Z edge).
    pub fn from_image(image: &RgbaImage, low: f32, high: f32) -> Result<Self> {
        let (w, h) = image.dimensions();
        let heights = image
            .pixels()
            .map(|p| {
                let [r, g, b, _] = p.0;
                let l =
                    (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0;
                low + (high - low) * l
            })
            .collect();
        Self::new(w, h, heights)
    }

    /// Samples along X.
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// Samples along Z.
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Every sample, row by row.
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// Every sample, row by row, for editing in place.
    pub fn heights_mut(&mut self) -> &mut [f32] {
        &mut self.heights
    }

    /// The sample at column `x`, row `z`, clamped to the grid.
    pub fn get(&self, x: i64, z: i64) -> f32 {
        let x = x.clamp(0, i64::from(self.cols) - 1) as usize;
        let z = z.clamp(0, i64::from(self.rows) - 1) as usize;
        self.heights[z * self.cols as usize + x]
    }

    /// Sets the sample at column `x`, row `z`; ignored outside the grid.
    pub fn set(&mut self, x: u32, z: u32, height: f32) {
        if x < self.cols && z < self.rows {
            self.heights[(z * self.cols + x) as usize] = height;
        }
    }

    /// The height at a point in grid units (column, row) on the surface
    /// the terrain draws: each cell is split into two triangles along the
    /// diagonal from its (x, z + 1) corner to its (x + 1, z) corner, the
    /// same split rapier's heightfields use. Outside the grid it is the
    /// nearest edge's height.
    pub fn sample(&self, gx: f32, gz: f32) -> f32 {
        let fx = gx.clamp(0.0, (self.cols - 1) as f32);
        let fz = gz.clamp(0.0, (self.rows - 1) as f32);
        let ix = (fx as i64).min(i64::from(self.cols) - 2);
        let iz = (fz as i64).min(i64::from(self.rows) - 2);
        let (tx, tz) = (fx - ix as f32, fz - iz as f32);
        let h00 = self.get(ix, iz);
        let h10 = self.get(ix + 1, iz);
        let h01 = self.get(ix, iz + 1);
        let h11 = self.get(ix + 1, iz + 1);
        if tx + tz <= 1.0 {
            h00 + (h10 - h00) * tx + (h01 - h00) * tz
        } else {
            h11 + (h01 - h11) * (1.0 - tx) + (h10 - h11) * (1.0 - tz)
        }
    }

    /// The unit normal at sample `(x, z)` from the heights either side,
    /// for samples `cell` world units apart.
    pub fn normal(&self, x: i64, z: i64, cell: f32) -> Vec3 {
        let dx = self.get(x - 1, z) - self.get(x + 1, z);
        let dz = self.get(x, z - 1) - self.get(x, z + 1);
        Vec3::new(dx, 2.0 * cell, dz).normalize_or(Vec3::Y)
    }

    /// The normal at a point in grid units, blended from the four samples
    /// around it.
    pub fn normal_at(&self, gx: f32, gz: f32, cell: f32) -> Vec3 {
        let fx = gx.clamp(0.0, (self.cols - 1) as f32);
        let fz = gz.clamp(0.0, (self.rows - 1) as f32);
        let (ix, iz) = (fx.floor() as i64, fz.floor() as i64);
        let (tx, tz) = (fx - ix as f32, fz - iz as f32);
        let n = |x, z| self.normal(x, z, cell);
        let a = n(ix, iz).lerp(n(ix + 1, iz), tx);
        let b = n(ix, iz + 1).lerp(n(ix + 1, iz + 1), tx);
        a.lerp(b, tz).normalize_or(Vec3::Y)
    }

    /// The lowest and highest samples in the inclusive rectangle of
    /// samples from `(x0, z0)` to `(x1, z1)`.
    pub(crate) fn range(&self, x0: u32, z0: u32, x1: u32, z1: u32) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for z in z0..=z1.min(self.rows - 1) {
            let row = &self.heights[(z * self.cols) as usize..][..self.cols as usize];
            for h in &row[x0 as usize..=x1.min(self.cols - 1) as usize] {
                lo = lo.min(*h);
                hi = hi.max(*h);
            }
        }
        (lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_follows_the_triangles() {
        // A single cell with one raised corner at (1, 1).
        let map = HeightMap::new(2, 2, vec![0.0, 0.0, 0.0, 4.0]).expect("map");
        let cases = [
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 4.0),
            // On the split diagonal every point is flat.
            (0.5, 0.5, 0.0),
            (0.25, 0.75, 0.0),
            // Past the diagonal the raised corner pulls the surface up.
            (0.75, 0.75, 2.0),
            (1.0, 0.5, 2.0),
            // Outside the grid the edge holds.
            (5.0, 5.0, 4.0),
        ];
        for (x, z, want) in cases {
            let got = map.sample(x, z);
            assert!((got - want).abs() < 1e-5, "({x}, {z}) = {got}, want {want}");
        }
    }

    #[test]
    fn from_image_maps_brightness() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let map = HeightMap::from_image(&img, -1.0, 3.0).expect("map");
        assert_eq!(map.get(0, 0), -1.0);
        assert!((map.get(1, 1) - 3.0).abs() < 1e-4);
        assert!(HeightMap::new(3, 3, vec![0.0; 8]).is_err());
    }

    #[test]
    fn normal_leans_away_from_a_slope() {
        let map = HeightMap::from_fn(5, 5, |x, _| x as f32);
        let n = map.normal(2, 2, 1.0);
        assert!(n.x < -0.5 && n.y > 0.5 && n.z.abs() < 1e-6, "{n:?}");
    }
}
