//! Polar shadow maps for 2D lights, built on the CPU.
//!
//! Each shadow-casting light owns one row of a strip texture: for each of
//! [`ANGLES`] directions around the light, the distance to the nearest
//! occluder edge. The lit shader reads the row along the direction from
//! the light to a pixel and darkens the pixel when it lies beyond the
//! stored distance. The cost is the number of occluder edges within a
//! light's radius times the number of shadowed lights.

use std::f32::consts::{PI, TAU};

use yowie_core::Vec2;

/// Directions stored per light. A light of radius 300 spread over 512
/// directions leaves the shadow edge under four units wide.
pub(crate) const ANGLES: usize = 512;

/// Added to every stored distance, in view units, so an occluder placed
/// along a wall does not shadow the wall itself.
const BIAS: f32 = 0.5;

/// The stored distance where nothing blocks the light.
pub(crate) const CLEAR: f32 = 1.0e9;

/// A closed polygon (or, with two points, one wall) that blocks light.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Occluders {
    pub(crate) points: Vec<Vec2>,
    /// The point count of each polygon, in order.
    pub(crate) runs: Vec<u32>,
}

impl Occluders {
    pub(crate) fn new() -> Self {
        Self {
            points: Vec::new(),
            runs: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.points.clear();
        self.runs.clear();
    }

    pub(crate) fn add(&mut self, polygon: &[Vec2]) {
        if polygon.len() < 2 {
            return;
        }
        self.points.extend_from_slice(polygon);
        self.runs.push(polygon.len() as u32);
    }
}

/// Fills `row` (length [`ANGLES`]) for a light at `light` with `radius`.
pub(crate) fn fill_row(row: &mut [f32], light: Vec2, radius: f32, occluders: &Occluders) {
    row.fill(CLEAR);
    let mut at = 0;
    for &n in &occluders.runs {
        let n = n as usize;
        let Some(poly) = occluders.points.get(at..at + n) else {
            break;
        };
        at += n;
        // Two points are one wall, not a there-and-back.
        let edges = if n == 2 { 1 } else { n };
        for i in 0..edges {
            segment(row, light, radius, poly[i], poly[(i + 1) % n]);
        }
    }
}

/// The unit direction at the centre of texel `i`, the angle the shader
/// looks the row up at.
fn direction(i: usize) -> Vec2 {
    let angle = TAU * ((i as f32 + 0.5) / ANGLES as f32 - 0.5);
    Vec2::new(angle.cos(), angle.sin())
}

/// Lowers the texels one occluder edge covers. Only those texels are
/// visited, so the cost follows the edge's angular width.
fn segment(row: &mut [f32], light: Vec2, radius: f32, a: Vec2, b: Vec2) {
    if distance_to_segment(light, a, b) > radius {
        return;
    }
    let (ra, rb) = (a - light, b - light);
    if ra == Vec2::ZERO || rb == Vec2::ZERO {
        return;
    }
    let mut from = ra.y.atan2(ra.x);
    let mut span = rb.y.atan2(rb.x) - from;
    // The edge covers the shorter way round, which is under half a turn
    // for any edge that does not pass through the light.
    while span > PI {
        span -= TAU;
    }
    while span < -PI {
        span += TAU;
    }
    if span < 0.0 {
        from += span;
        span = -span;
    }
    let n = ANGLES as f32;
    let first = (from / TAU + 0.5) * n - 0.5;
    let last = first + span / TAU * n;
    let (k0, k1) = (first.ceil() as i64, last.floor() as i64);
    for k in k0..=k1 {
        let i = k.rem_euclid(ANGLES as i64) as usize;
        if let Some(t) = ray_segment(light, direction(i), a, b) {
            let d = (t + BIAS).min(CLEAR);
            if d < row[i] {
                row[i] = d;
            }
        }
    }
}

/// How far along the unit direction `dir` from `origin` the segment `a`-`b`
/// lies, or `None` when the ray misses it.
fn ray_segment(origin: Vec2, dir: Vec2, a: Vec2, b: Vec2) -> Option<f32> {
    let e = b - a;
    let den = dir.x * e.y - dir.y * e.x;
    if den == 0.0 {
        return None;
    }
    let oa = a - origin;
    let t = (oa.x * e.y - oa.y * e.x) / den;
    let u = (oa.x * dir.y - oa.y * dir.x) / den;
    (t >= 0.0 && (0.0..=1.0).contains(&u)).then_some(t)
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let e = b - a;
    let len2 = e.length_squared();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(e) / len2).clamp(0.0, 1.0);
    p.distance(a + e * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(row: &[f32], angle: f32) -> f32 {
        let u = angle / TAU + 0.5;
        row[((u * ANGLES as f32) as usize).min(ANGLES - 1)]
    }

    #[test]
    fn a_wall_blocks_the_directions_it_covers() {
        let mut occ = Occluders::new();
        // A vertical wall 100 units to the right of the light.
        occ.add(&[Vec2::new(100.0, -50.0), Vec2::new(100.0, 50.0)]);
        let mut row = vec![0.0; ANGLES];
        fill_row(&mut row, Vec2::ZERO, 300.0, &occ);
        let ahead = at(&row, 0.0);
        assert!((ahead - 100.5).abs() < 0.5, "wall distance {ahead}");
        assert_eq!(at(&row, PI), CLEAR, "behind the light is clear");
        assert_eq!(at(&row, 1.5), CLEAR, "off to the side is clear");
    }

    #[test]
    fn edges_beyond_the_radius_are_ignored() {
        let mut occ = Occluders::new();
        occ.add(&[Vec2::new(500.0, -50.0), Vec2::new(500.0, 50.0)]);
        let mut row = vec![0.0; ANGLES];
        fill_row(&mut row, Vec2::ZERO, 300.0, &occ);
        assert!(row.iter().all(|&d| d == CLEAR));
    }

    #[test]
    fn a_box_stores_its_near_face() {
        let mut occ = Occluders::new();
        occ.add(&[
            Vec2::new(60.0, -20.0),
            Vec2::new(100.0, -20.0),
            Vec2::new(100.0, 20.0),
            Vec2::new(60.0, 20.0),
        ]);
        let mut row = vec![0.0; ANGLES];
        fill_row(&mut row, Vec2::ZERO, 300.0, &occ);
        assert!((at(&row, 0.0) - 60.5).abs() < 0.5);
    }
}
