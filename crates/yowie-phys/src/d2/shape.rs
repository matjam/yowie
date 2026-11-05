//! 2D collider shapes.

use rapier2d::geometry::SharedShape;
use yowie_core::{Transform2D, Vec2};

use super::convert::{transform_pose, vec};
use crate::error::{PhysError, Result};

/// A 2D collision shape in its own local space.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape2D {
    /// A circle centred on the origin.
    Circle {
        /// Radius in world units.
        radius: f32,
    },
    /// An axis-aligned box centred on the origin.
    Box {
        /// Half the size along each axis.
        half_extents: Vec2,
    },
    /// A capsule along the Y axis: a segment from `-half_height` to
    /// `+half_height` swept by `radius`.
    Capsule {
        /// Half the length of the inner segment.
        half_height: f32,
        /// Radius of the end caps and the body.
        radius: f32,
    },
    /// The convex hull of a point set (at least three points).
    ConvexPolygon(Vec<Vec2>),
    /// A single line segment. Segments have no inside, so use them for
    /// static geometry.
    Segment {
        /// First end point.
        a: Vec2,
        /// Second end point.
        b: Vec2,
    },
    /// A chain of connected segments through `points`, closed into a loop
    /// when `closed` is set. Use it for static terrain outlines.
    Polyline {
        /// Vertices in order along the chain.
        points: Vec<Vec2>,
        /// Joins the last point back to the first.
        closed: bool,
    },
    /// A height profile sampled at evenly spaced X positions, filled below
    /// the profile. Use it for static terrain.
    Heightfield {
        /// Height of each sample, left to right. At least 2.
        heights: Vec<f32>,
        /// Total extent along X (samples span `-extent / 2` to
        /// `extent / 2` around the collider origin) and a multiplier
        /// for the heights.
        scale: Vec2,
    },
    /// Several shapes with local offsets, acting as one collider.
    Compound(Vec<(Transform2D, Shape2D)>),
}

impl Shape2D {
    /// A circle of the given radius.
    pub fn circle(radius: f32) -> Self {
        Self::Circle { radius }
    }

    /// A box with the given half extents.
    pub fn cuboid(half_extents: Vec2) -> Self {
        Self::Box { half_extents }
    }

    /// A Y-axis capsule.
    pub fn capsule(half_height: f32, radius: f32) -> Self {
        Self::Capsule {
            half_height,
            radius,
        }
    }

    pub(crate) fn to_shared(&self) -> Result<SharedShape> {
        let positive = |v: f32| v.is_finite() && v > 0.0;
        match self {
            Self::Circle { radius } if positive(*radius) => Ok(SharedShape::ball(*radius)),
            Self::Box { half_extents } if positive(half_extents.x) && positive(half_extents.y) => {
                Ok(SharedShape::cuboid(half_extents.x, half_extents.y))
            }
            Self::Capsule {
                half_height,
                radius,
            } if *half_height >= 0.0 && positive(*radius) => {
                Ok(SharedShape::capsule_y(*half_height, *radius))
            }
            Self::ConvexPolygon(points) => {
                let pts: Vec<_> = points.iter().map(|p| vec(*p)).collect();
                SharedShape::convex_hull(&pts).ok_or(PhysError::InvalidShape(
                    "convex polygon needs 3 non-collinear points",
                ))
            }
            Self::Segment { a, b } if a != b => Ok(SharedShape::segment(vec(*a), vec(*b))),
            Self::Polyline { points, closed } => {
                if points.len() < 2 || (*closed && points.len() < 3) {
                    return Err(PhysError::InvalidShape("polyline needs at least 2 points"));
                }
                let verts: Vec<_> = points.iter().map(|p| vec(*p)).collect();
                let n = verts.len() as u32;
                let mut idx: Vec<[u32; 2]> = (0..n - 1).map(|i| [i, i + 1]).collect();
                if *closed {
                    idx.push([n - 1, 0]);
                }
                Ok(SharedShape::polyline(verts, Some(idx)))
            }
            Self::Heightfield { heights, scale } => {
                if heights.len() < 2
                    || !positive(scale.x)
                    || !scale.y.is_finite()
                    || heights.iter().any(|h| !h.is_finite())
                {
                    return Err(PhysError::InvalidShape(
                        "heightfield needs 2 finite heights and a positive width",
                    ));
                }
                Ok(SharedShape::heightfield(heights.clone(), vec(*scale)))
            }
            Self::Compound(parts) => {
                if parts.is_empty() {
                    return Err(PhysError::InvalidShape("compound shape has no parts"));
                }
                let mut shapes = Vec::with_capacity(parts.len());
                for (t, s) in parts {
                    shapes.push((transform_pose(t), s.to_shared()?));
                }
                Ok(SharedShape::compound(shapes))
            }
            _ => Err(PhysError::InvalidShape("dimensions must be positive")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_and_invalid_shapes() {
        let tri = vec![Vec2::ZERO, Vec2::X, Vec2::Y];
        let cases: Vec<(Shape2D, bool)> = vec![
            (Shape2D::circle(0.5), true),
            (Shape2D::circle(0.0), false),
            (Shape2D::cuboid(Vec2::new(1.0, 2.0)), true),
            (Shape2D::cuboid(Vec2::new(1.0, -2.0)), false),
            (Shape2D::capsule(0.5, 0.25), true),
            (Shape2D::capsule(0.5, 0.0), false),
            (Shape2D::ConvexPolygon(tri.clone()), true),
            (Shape2D::ConvexPolygon(vec![Vec2::ZERO, Vec2::X]), false),
            (
                Shape2D::Segment {
                    a: Vec2::ZERO,
                    b: Vec2::X,
                },
                true,
            ),
            (
                Shape2D::Segment {
                    a: Vec2::X,
                    b: Vec2::X,
                },
                false,
            ),
            (
                Shape2D::Polyline {
                    points: tri.clone(),
                    closed: true,
                },
                true,
            ),
            (
                Shape2D::Polyline {
                    points: vec![Vec2::ZERO],
                    closed: false,
                },
                false,
            ),
            (
                Shape2D::Heightfield {
                    heights: vec![0.0, 1.0, 0.5],
                    scale: Vec2::new(10.0, 1.0),
                },
                true,
            ),
            (
                Shape2D::Heightfield {
                    heights: vec![0.0],
                    scale: Vec2::new(10.0, 1.0),
                },
                false,
            ),
            (
                Shape2D::Heightfield {
                    heights: vec![0.0, 1.0],
                    scale: Vec2::new(0.0, 1.0),
                },
                false,
            ),
            (Shape2D::Compound(vec![]), false),
            (
                Shape2D::Compound(vec![(Transform2D::IDENTITY, Shape2D::circle(1.0))]),
                true,
            ),
        ];
        for (shape, ok) in cases {
            assert_eq!(shape.to_shared().is_ok(), ok, "{shape:?}");
        }
    }
}
