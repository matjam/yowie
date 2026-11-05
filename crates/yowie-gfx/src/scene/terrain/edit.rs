//! Editing a terrain: brushes that dig, raise, flatten and smooth, edits
//! of any rectangle of samples, and replacing the splat map or a layer.
//! Each edit rebuilds only the chunks, normals and splat texels it
//! touched.

use image::RgbaImage;
use yowie_core::Vec3;

use super::splat::{self, SplatRules};
use super::{Terrain, TerrainLayer};
use crate::Graphics;
use crate::error::{GfxError, Result};

/// What a [`Brush`] does to the ground under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushMode {
    /// Lifts the ground by up to `strength` world units at the centre.
    Raise,
    /// Lowers the ground by up to `strength` world units at the centre.
    Lower,
    /// Moves the ground towards a world height by `strength` (0 to 1) of
    /// the way at the centre.
    Flatten(f32),
    /// Moves each sample towards the mean of its neighbours by `strength`
    /// (0 to 1) of the way at the centre.
    Smooth,
}

/// A round brush that edits the ground around a point, strongest at the
/// centre and fading smoothly to nothing at `radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// The world point under the brush's centre; `y` is ignored.
    pub center: Vec3,
    /// The radius in world units.
    pub radius: f32,
    /// How much the brush does at its centre; see [`BrushMode`].
    pub strength: f32,
    /// What the brush does.
    pub mode: BrushMode,
}

impl Brush {
    /// A brush that digs a crater `depth` units deep.
    pub fn dig(center: Vec3, radius: f32, depth: f32) -> Self {
        Self {
            center,
            radius,
            strength: depth,
            mode: BrushMode::Lower,
        }
    }

    /// A brush that raises a mound `height` units high.
    pub fn raise(center: Vec3, radius: f32, height: f32) -> Self {
        Self {
            center,
            radius,
            strength: height,
            mode: BrushMode::Raise,
        }
    }
}

impl Terrain {
    /// Applies `brush`, then rebuilds what it touched. Returns whether any
    /// sample changed.
    pub fn apply_brush(&self, gfx: &Graphics, brush: &Brush) -> bool {
        let i = &self.inner;
        let r = brush.radius.max(i.cell * 0.5);
        let (gx, gz) = i.to_grid(brush.center.x, brush.center.z);
        let reach = r / i.cell;
        let x0 = (gx - reach).floor().max(0.0) as u32;
        let z0 = (gz - reach).floor().max(0.0) as u32;
        let x1 = ((gx + reach).ceil().max(0.0) as u32).min(i.cols - 1);
        let z1 = ((gz + reach).ceil().max(0.0) as u32).min(i.rows - 1);
        if x0 > x1 || z0 > z1 || gx + reach < 0.0 || gz + reach < 0.0 {
            return false;
        }
        let snapshot = (brush.mode == BrushMode::Smooth).then(|| i.state.read().map.clone());
        let base_y = i.origin.y;
        let mut changed = false;
        self.edit(gfx, [x0, z0, x1, z1], |x, z, h| {
            let d = (Vec3::new(x as f32 - gx, 0.0, z as f32 - gz).length() * i.cell) / r;
            if d >= 1.0 {
                return;
            }
            let fall = (1.0 - d * d) * (1.0 - d * d);
            let before = *h;
            match brush.mode {
                BrushMode::Raise => *h += brush.strength * fall,
                BrushMode::Lower => *h -= brush.strength * fall,
                BrushMode::Flatten(target) => {
                    let t = (brush.strength * fall).clamp(0.0, 1.0);
                    *h += (target - base_y - *h) * t;
                }
                BrushMode::Smooth => {
                    if let Some(m) = &snapshot {
                        let (x, z) = (i64::from(x), i64::from(z));
                        let mean =
                            (m.get(x - 1, z) + m.get(x + 1, z) + m.get(x, z - 1) + m.get(x, z + 1))
                                * 0.25;
                        *h += (mean - *h) * (brush.strength * fall).clamp(0.0, 1.0);
                    }
                }
            }
            changed |= *h != before;
        });
        changed
    }

    /// Calls `f` with the column, row and a mutable height of every
    /// sample in the inclusive rectangle `[x0, z0, x1, z1]` (clamped to
    /// the grid), then rebuilds the chunks, normals and splat texels it
    /// covers.
    pub fn edit(&self, gfx: &Graphics, rect: [u32; 4], mut f: impl FnMut(u32, u32, &mut f32)) {
        let i = &self.inner;
        let x1 = rect[2].min(i.cols - 1);
        let z1 = rect[3].min(i.rows - 1);
        let (x0, z0) = (rect[0].min(x1), rect[1].min(z1));
        let mut state = i.state.write();
        let cols = i.cols;
        {
            let heights = state.map.heights_mut();
            for z in z0..=z1 {
                for x in x0..=x1 {
                    f(x, z, &mut heights[(z * cols + x) as usize]);
                }
            }
        }
        // Normals read one sample either side, so the rebuilt area grows
        // by one.
        let grown = [
            x0.saturating_sub(1),
            z0.saturating_sub(1),
            (x1 + 1).min(cols - 1),
            (z1 + 1).min(i.rows - 1),
        ];
        let shared = &gfx.shared;
        i.gpu.write_heights(shared, &state.map, z0, z1);
        i.gpu.write_normals(shared, &state.map, i.cell, grown);
        if let Some(rules) = state.rules {
            let state = &mut *state;
            splat::paint(
                &state.map,
                &rules,
                i.origin.y,
                i.cell,
                grown,
                &mut state.splat,
            );
            i.gpu.write_splat(shared, cols, &state.splat, grown);
        }
        let n = i.chunk;
        let (cx0, cz0) = (x0.saturating_sub(1) / n, z0.saturating_sub(1) / n);
        let cx1 = ((x1 + 1) / n).min(i.chunks_x - 1);
        let cz1 = ((z1 + 1) / n).min(i.chunks_z - 1);
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                let c = cz * i.chunks_x + cx;
                let info = i.chunk_info(&state.map, c);
                state.chunks[c as usize] = info;
            }
        }
    }

    /// Replaces the splat map with an image whose red, green, blue and
    /// alpha weight the four layers; it is resampled to one texel per
    /// sample. Painting rules stop, so edits keep the painted weights.
    pub fn set_splat(&self, gfx: &Graphics, image: &RgbaImage) -> Result<()> {
        let i = &self.inner;
        let (w, h) = image.dimensions();
        if w == 0 || h == 0 {
            return Err(GfxError::Terrain("the splat image is empty".into()));
        }
        let mut state = i.state.write();
        state.rules = None;
        for z in 0..i.rows {
            for x in 0..i.cols {
                let px = (x as u64 * u64::from(w - 1) / u64::from(i.cols - 1)) as u32;
                let pz = (z as u64 * u64::from(h - 1) / u64::from(i.rows - 1)) as u32;
                let at = ((z * i.cols + x) * 4) as usize;
                state.splat[at..at + 4].copy_from_slice(&image.get_pixel(px, pz).0);
            }
        }
        i.gpu.write_splat(
            &gfx.shared,
            i.cols,
            &state.splat,
            [0, 0, i.cols - 1, i.rows - 1],
        );
        Ok(())
    }

    /// Repaints the whole splat map from `rules`, which edits then follow.
    pub fn set_splat_rules(&self, gfx: &Graphics, rules: SplatRules) {
        let i = &self.inner;
        let mut state = i.state.write();
        let state = &mut *state;
        state.rules = Some(rules);
        let all = [0, 0, i.cols - 1, i.rows - 1];
        splat::paint(
            &state.map,
            &rules,
            i.origin.y,
            i.cell,
            all,
            &mut state.splat,
        );
        i.gpu.write_splat(&gfx.shared, i.cols, &state.splat, all);
    }

    /// Replaces ground layer `index` (0 to 3).
    pub fn set_layer(&self, gfx: &Graphics, index: usize, layer: TerrainLayer) {
        let i = &self.inner;
        let mut state = i.state.write();
        if let Some(l) = state.layers.get_mut(index) {
            *l = layer;
            state.generation += 1;
            i.gpu.write_params(&gfx.shared, i, &state);
        }
    }

    /// The four ground layers.
    pub fn layers(&self) -> [TerrainLayer; 4] {
        self.inner.state.read().layers.clone()
    }
}
