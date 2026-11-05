//! Levels of detail: several versions of a mesh or model, finest first,
//! chosen by how much of the view an object covers, with hysteresis so an
//! object resting near a boundary does not flicker between levels.

use yowie_core::Vec3;

use super::camera::Projection;
use super::material::Material;
use super::mesh::Mesh;
use super::model::ModelInstance;
use super::submit::{DrawId, Placement, Scene};

/// The level an object was last drawn at, which hysteresis compares
/// against. Keep one per object that draws through a [`Lod`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LodState {
    level: Option<u8>,
}

impl LodState {
    /// The level last chosen, or `None` before the first draw.
    pub fn level(&self) -> Option<usize> {
        self.level.map(usize::from)
    }
}

/// One level: what it draws (`None` for nothing) and the screen coverage
/// below which it takes over from the level before.
#[derive(Clone, Debug, PartialEq)]
struct Level<T> {
    item: Option<T>,
    below: f32,
}

/// Versions of something from finest to coarsest, each used while the
/// object covers less of the view than the threshold it was added with.
/// Coverage is the object's bounding sphere's projected diameter as a
/// fraction of the view's height: 1 fills the view top to bottom.
///
/// ```no_run
/// # use yowie_gfx::scene::*;
/// # fn f(fine: Mesh, coarse: Mesh, tiny: Mesh) {
/// let rock = Lod::new(fine)
///     .then(coarse, 0.15)
///     .then(tiny, 0.04)
///     .cull_below(0.01);
/// # }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Lod<T> {
    levels: Vec<Level<T>>,
    /// How far past a threshold (as a fraction of it) the coverage must
    /// move before the level changes.
    pub hysteresis: f32,
}

impl<T> Lod<T> {
    /// A level of detail whose finest level is `item`.
    pub fn new(item: T) -> Self {
        Self {
            levels: vec![Level {
                item: Some(item),
                below: f32::INFINITY,
            }],
            hysteresis: 0.15,
        }
    }

    /// Adds a coarser level used below `coverage`. Thresholds must fall
    /// from one level to the next.
    pub fn then(mut self, item: T, coverage: f32) -> Self {
        self.levels.push(Level {
            item: Some(item),
            below: coverage,
        });
        self
    }

    /// Draws nothing below `coverage`.
    pub fn cull_below(mut self, coverage: f32) -> Self {
        self.levels.push(Level {
            item: None,
            below: coverage,
        });
        self
    }

    /// The number of levels, the culled one included.
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Whether there are no levels; never true.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// The item of level `i`, `None` past the end or for a culled level.
    pub fn get(&self, i: usize) -> Option<&T> {
        self.levels.get(i).and_then(|l| l.item.as_ref())
    }

    /// The level for `coverage`, ignoring hysteresis.
    pub fn level_for(&self, coverage: f32) -> usize {
        self.levels
            .iter()
            .rposition(|l| coverage < l.below)
            .unwrap_or(0)
    }

    /// The level for `coverage` after `state`'s last level, with
    /// hysteresis, and its item (`None` when the level draws nothing).
    /// Updates `state`.
    pub fn select(&self, coverage: f32, state: &mut LodState) -> (usize, Option<&T>) {
        let level = match state.level {
            None => self.level_for(coverage),
            Some(prev) => settle(
                |d| self.level_for(1.0 / d.max(1e-12)) as u32,
                u32::from(prev),
                1.0 / coverage.max(1e-12),
                self.hysteresis,
            ) as usize,
        };
        state.level = Some(level.min(255) as u8);
        (level, self.get(level))
    }
}

/// The level after `prev` for something whose distance (or any measure
/// that grows as detail should fall) asks for `level_for(distance)`: a
/// change needs the measure past the boundary by the hysteresis fraction.
pub(crate) fn settle(
    level_for: impl Fn(f32) -> u32,
    prev: u32,
    distance: f32,
    hysteresis: f32,
) -> u32 {
    let target = level_for(distance);
    if target > prev {
        level_for(distance / (1.0 + hysteresis)).max(prev)
    } else if target < prev {
        level_for(distance * (1.0 + hysteresis)).min(prev)
    } else {
        prev
    }
}

impl Scene<'_> {
    /// How much of the view a sphere covers: its projected diameter as a
    /// fraction of the view's height (a sphere around the camera covers
    /// all of it).
    pub fn screen_coverage(&self, center: Vec3, radius: f32) -> f32 {
        let cam = &self.rec.camera;
        match cam.projection {
            Projection::Perspective { fov_y } => {
                let d = (center - cam.position).length();
                if d <= radius {
                    return f32::INFINITY;
                }
                radius / (d * (fov_y.max(1e-3) * 0.5).tan())
            }
            Projection::Orthographic { half_height } => radius / half_height.max(1e-6),
        }
    }

    fn count_lod(&mut self, level: usize) {
        self.rec.large.lod_levels[level.min(3)] += 1;
    }

    /// Draws the level of `lod` that the mesh's coverage asks for, with
    /// `material`; nothing when that level is culled. Coverage is taken
    /// from the finest mesh's bounds.
    pub fn draw_lod(
        &mut self,
        lod: &Lod<Mesh>,
        material: &Material,
        placement: impl Placement,
        state: &mut LodState,
    ) -> Option<DrawId> {
        let m = placement.model_matrix();
        let b = lod.get(0)?.bounds().transformed(&m);
        let coverage = self.screen_coverage(b.center(), b.radius());
        let (level, mesh) = lod.select(coverage, state);
        let mesh = mesh?.clone();
        self.count_lod(level);
        Some(self.draw(&mesh, material, m))
    }

    /// Draws the level of `lod` that the model's coverage asks for;
    /// nothing when that level is culled. Coverage is taken from the
    /// finest model's bounds.
    pub fn draw_model_lod(
        &mut self,
        lod: &Lod<ModelInstance>,
        placement: impl Placement,
        state: &mut LodState,
    ) -> Option<DrawId> {
        let m = placement.model_matrix();
        let b = lod.get(0)?.model().bounds().transformed(&m);
        let coverage = self.screen_coverage(b.center(), b.radius());
        let (level, instance) = lod.select(coverage, state);
        let instance = instance?;
        self.count_lod(level);
        Some(self.draw_model(instance, m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_coverage_with_hysteresis() {
        let lod = Lod::new("fine").then("coarse", 0.2).cull_below(0.05);
        assert_eq!(lod.level_for(0.5), 0);
        assert_eq!(lod.level_for(0.1), 1);
        assert_eq!(lod.level_for(0.01), 2);
        let mut state = LodState::default();
        assert_eq!(lod.select(0.5, &mut state), (0, Some(&"fine")));
        // Just under the threshold holds the fine level; well under
        // switches.
        assert_eq!(lod.select(0.19, &mut state).0, 0);
        assert_eq!(lod.select(0.15, &mut state).0, 1);
        // Back slightly over holds the coarse level.
        assert_eq!(lod.select(0.21, &mut state).0, 1);
        assert_eq!(lod.select(0.3, &mut state).0, 0);
        assert_eq!(lod.select(0.001, &mut state), (2, None));
    }
}
