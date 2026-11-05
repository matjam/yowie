//! Axis-aligned rectangles.

use glam::{Vec2, vec2};
use serde::{Deserialize, Serialize};

/// An axis-aligned rectangle given by its top-left corner and size, in
/// whatever units the caller uses (pixels for 2D drawing, texels for sprite
/// sources, world units for culling). `y` grows downwards in screen space.
///
/// A rectangle with negative width or height is empty: it contains no point
/// and intersects nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// The rectangle from `min` to `max`.
    pub fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self::new(min.x, min.y, max.x - min.x, max.y - min.y)
    }

    /// The rectangle of `size` centred on `center`.
    pub fn from_center(center: Vec2, size: Vec2) -> Self {
        Self::from_min_max(center - size * 0.5, center + size * 0.5)
    }

    /// The smallest rectangle containing every point, or `None` for none.
    pub fn bounding<I: IntoIterator<Item = Vec2>>(points: I) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let (min, max) = it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Some(Self::from_min_max(min, max))
    }

    pub fn min(&self) -> Vec2 {
        vec2(self.x, self.y)
    }

    pub fn max(&self) -> Vec2 {
        vec2(self.x + self.w, self.y + self.h)
    }

    pub fn size(&self) -> Vec2 {
        vec2(self.w, self.h)
    }

    pub fn center(&self) -> Vec2 {
        vec2(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Whether `p` lies inside; the left and top edges are inside, the right
    /// and bottom edges outside, so tiled rectangles never share a point.
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }

    /// Whether `other` lies entirely inside.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        !other.is_empty()
            && other.x >= self.x
            && other.y >= self.y
            && other.x + other.w <= self.x + self.w
            && other.y + other.h <= self.y + self.h
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.intersection(other).is_some()
    }

    /// The overlap of two rectangles, or `None` when they do not overlap.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let min = self.min().max(other.min());
        let max = self.max().min(other.max());
        let r = Self::from_min_max(min, max);
        (!r.is_empty()).then_some(r)
    }

    /// The smallest rectangle containing both.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Self::from_min_max(self.min().min(other.min()), self.max().max(other.max()))
    }

    /// Grows each edge outwards by `amount` (shrinks for negative amounts).
    pub fn inflate(&self, amount: f32) -> Rect {
        Self::new(
            self.x - amount,
            self.y - amount,
            self.w + amount * 2.0,
            self.h + amount * 2.0,
        )
    }

    pub fn translate(&self, by: Vec2) -> Rect {
        Self::new(self.x + by.x, self.y + by.y, self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(vec2(0.0, 0.0)));
        assert!(r.contains(vec2(9.99, 9.99)));
        assert!(!r.contains(vec2(10.0, 5.0)));
        assert!(!r.contains(vec2(5.0, 10.0)));
    }

    #[test]
    fn intersection_cases() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let cases = [
            (
                Rect::new(5.0, 5.0, 10.0, 10.0),
                Some(Rect::new(5.0, 5.0, 5.0, 5.0)),
            ),
            (Rect::new(10.0, 0.0, 5.0, 5.0), None),
            (
                Rect::new(2.0, 2.0, 2.0, 2.0),
                Some(Rect::new(2.0, 2.0, 2.0, 2.0)),
            ),
            (Rect::new(-5.0, -5.0, 1.0, 1.0), None),
        ];
        for (b, want) in cases {
            assert_eq!(a.intersection(&b), want, "{b:?}");
        }
    }

    #[test]
    fn union_ignores_empty() {
        let a = Rect::new(1.0, 1.0, 2.0, 2.0);
        assert_eq!(a.union(&Rect::ZERO), a);
        assert_eq!(
            a.union(&Rect::new(5.0, 0.0, 1.0, 1.0)),
            Rect::new(1.0, 0.0, 5.0, 3.0)
        );
    }

    #[test]
    fn bounding_points() {
        let r = Rect::bounding([vec2(3.0, 1.0), vec2(-1.0, 4.0), vec2(2.0, 2.0)]).unwrap();
        assert_eq!(r, Rect::new(-1.0, 1.0, 4.0, 3.0));
        assert_eq!(Rect::bounding(std::iter::empty()), None);
    }
}
