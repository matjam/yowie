//! Height, normal and ray queries against a terrain's current heights.

use yowie_core::Vec3;

use super::Terrain;
use crate::scene::bounds::Ray;
use crate::scene::mesh::RayHit;

impl Terrain {
    /// The ground's world height at world `x`, `z`, on the surface the
    /// finest level draws. Outside the terrain it is the nearest edge's.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (gx, gz) = self.inner.to_grid(x, z);
        self.inner.origin.y + self.inner.state.read().map.sample(gx, gz)
    }

    /// The ground's unit normal at world `x`, `z`, smoothed between
    /// samples. Use it to stand an object on a slope or to refuse to build
    /// on one.
    pub fn normal(&self, x: f32, z: f32) -> Vec3 {
        let (gx, gz) = self.inner.to_grid(x, z);
        self.inner
            .state
            .read()
            .map
            .normal_at(gx, gz, self.inner.cell)
    }

    /// Where `ray` first passes under the ground within `max_distance`
    /// world units: it steps half a cell at a time, then narrows the step
    /// it crossed on. A ray starting under the ground hits at its origin.
    /// `triangle` in the result is the index of the cell that was hit.
    pub fn raycast(&self, ray: &Ray, max_distance: f32) -> Option<RayHit> {
        let inner = &self.inner;
        let bounds = self.bounds();
        let state = inner.state.read();
        let above = |t: f32| {
            let p = ray.at(t);
            let (gx, gz) = inner.to_grid(p.x, p.z);
            p.y - inner.origin.y - state.map.sample(gx, gz)
        };
        let start = bounds.intersect_ray(ray)?;
        let end = max_distance.min(start + (bounds.max - bounds.min).length() + inner.cell);
        let hit_at = if above(start) <= 0.0 {
            start
        } else {
            let step = inner.cell * 0.5;
            let mut prev = start;
            let mut found = None;
            let mut t = start + step;
            while prev < end {
                let t1 = t.min(end);
                if above(t1) <= 0.0 {
                    let (mut lo, mut hi) = (prev, t1);
                    for _ in 0..24 {
                        let mid = (lo + hi) * 0.5;
                        if above(mid) > 0.0 {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    found = Some(hi);
                    break;
                }
                prev = t1;
                t += step;
            }
            found?
        };
        let point = ray.at(hit_at);
        let (gx, gz) = inner.to_grid(point.x, point.z);
        let cell_index = gz.max(0.0) as u32 * (inner.cols - 1) + gx.max(0.0) as u32;
        Some(RayHit {
            distance: hit_at,
            point,
            normal: state.map.normal_at(gx, gz, inner.cell),
            triangle: cell_index,
        })
    }
}
