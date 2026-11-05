//! Recording a terrain into a scene: each chunk's level is chosen from
//! its distance to the camera, with hysteresis, when the draw is
//! recorded.

use super::Terrain;
use crate::scene::lod::settle;
use crate::scene::submit::Scene;

/// How [`Scene::draw_terrain_with`] draws a terrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainDrawOptions {
    /// Whether the terrain casts shadows (it always receives them).
    pub cast_shadows: bool,
    /// Tints each chunk by its level (red finest, then yellow, green,
    /// blue), to see where the levels change.
    pub show_levels: bool,
}

impl Default for TerrainDrawOptions {
    fn default() -> Self {
        Self {
            cast_shadows: true,
            show_levels: false,
        }
    }
}

/// One recorded terrain draw: the terrain and the level of each chunk.
#[derive(Clone, Debug)]
pub(crate) struct TerrainDraw {
    pub(crate) terrain: Terrain,
    pub(crate) levels: Vec<u8>,
    pub(crate) options: TerrainDrawOptions,
}

impl Scene<'_> {
    /// Draws a terrain: every chunk in view at the level its distance from
    /// the camera asks for, in one draw call per level. The frustum culls
    /// chunks out of view; [`FrameStats`](crate::FrameStats) counts the
    /// chunks drawn and culled.
    pub fn draw_terrain(&mut self, terrain: &Terrain) {
        self.draw_terrain_with(terrain, TerrainDrawOptions::default());
    }

    /// Draws a terrain with options.
    pub fn draw_terrain_with(&mut self, terrain: &Terrain, options: TerrainDrawOptions) {
        let t = &terrain.inner;
        let eye = self.rec.camera.position;
        let state = t.state.read();
        let mut chosen = t.chosen.lock();
        for (i, c) in state.chunks.iter().enumerate() {
            let near = eye.clamp(c.bounds.min, c.bounds.max);
            let d = (near - eye).length();
            let prev = u32::from(chosen[i]);
            let level = settle(|d| t.level_for(d), prev, d, t.hysteresis);
            chosen[i] = level as u8;
        }
        self.rec.large.terrains.push(TerrainDraw {
            terrain: terrain.clone(),
            levels: chosen.clone(),
            options,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis_holds_a_level_near_its_boundary() {
        // Levels change every 10 units: 0 below 10, 1 below 20, 2 beyond.
        let level_for = |d: f32| (d / 10.0).floor().min(2.0) as u32;
        let cases = [
            ("stays fine just past the boundary", 0, 10.5, 0.1, 0),
            ("goes coarser well past it", 0, 11.5, 0.1, 1),
            ("stays coarse just inside", 1, 9.5, 0.1, 1),
            ("goes finer well inside", 1, 8.5, 0.1, 0),
            ("jumps several levels", 0, 30.0, 0.1, 2),
            ("no hysteresis follows the distance", 0, 10.0, 0.0, 1),
        ];
        for (name, prev, d, h, want) in cases {
            assert_eq!(settle(level_for, prev, d, h), want, "{name}");
        }
    }
}
