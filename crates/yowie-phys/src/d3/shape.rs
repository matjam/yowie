//! 3D collider shapes.

use rapier3d::geometry::SharedShape;
use rapier3d::parry::utils::Array2;
use yowie_core::{Transform, Vec3};

use super::convert::{transform_pose, vec};
use crate::error::{PhysError, Result};

/// A 3D collision shape in its own local space.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape3D {
    /// A sphere centred on the origin.
    Sphere {
        /// Radius in world units.
        radius: f32,
    },
    /// An axis-aligned box centred on the origin.
    Box {
        /// Half the size along each axis.
        half_extents: Vec3,
    },
    /// A capsule along the Y axis: a segment from `-half_height` to
    /// `+half_height` swept by `radius`.
    Capsule {
        /// Half the length of the inner segment.
        half_height: f32,
        /// Radius of the end caps and the body.
        radius: f32,
    },
    /// A cylinder along the Y axis.
    Cylinder {
        /// Half the cylinder's height.
        half_height: f32,
        /// Radius in world units.
        radius: f32,
    },
    /// The convex hull of a point cloud (at least four non-coplanar points).
    ConvexHull(Vec<Vec3>),
    /// A triangle mesh. Meshes are hollow: they collide on their surface
    /// only, so use them for static geometry.
    TriMesh {
        /// Vertex positions.
        vertices: Vec<Vec3>,
        /// Triangles as indices into `vertices`.
        indices: Vec<[u32; 3]>,
    },
    /// A regular grid of heights, centred on the origin in X and Z.
    Heightfield {
        /// Number of samples along Z. At least 2.
        rows: usize,
        /// Number of samples along X. At least 2.
        cols: usize,
        /// Heights in row-major order: `heights[row * cols + col]`.
        heights: Vec<f32>,
        /// Total extent along X, a multiplier for heights along Y, and
        /// total extent along Z.
        scale: Vec3,
    },
    /// Several shapes with local offsets, acting as one collider.
    Compound(Vec<(Transform, Shape3D)>),
}

impl Shape3D {
    /// A sphere of the given radius.
    pub fn sphere(radius: f32) -> Self {
        Self::Sphere { radius }
    }

    /// A box with the given half extents.
    pub fn cuboid(half_extents: Vec3) -> Self {
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
            Self::Sphere { radius } if positive(*radius) => Ok(SharedShape::ball(*radius)),
            Self::Box { half_extents }
                if positive(half_extents.x)
                    && positive(half_extents.y)
                    && positive(half_extents.z) =>
            {
                Ok(SharedShape::cuboid(
                    half_extents.x,
                    half_extents.y,
                    half_extents.z,
                ))
            }
            Self::Capsule {
                half_height,
                radius,
            } if *half_height >= 0.0 && positive(*radius) => {
                Ok(SharedShape::capsule_y(*half_height, *radius))
            }
            Self::Cylinder {
                half_height,
                radius,
            } if positive(*half_height) && positive(*radius) => {
                Ok(SharedShape::cylinder(*half_height, *radius))
            }
            Self::ConvexHull(points) => {
                let pts: Vec<_> = points.iter().map(|p| vec(*p)).collect();
                SharedShape::convex_hull(&pts).ok_or(PhysError::InvalidShape(
                    "convex hull needs 4 non-coplanar points",
                ))
            }
            Self::TriMesh { vertices, indices } => {
                if indices
                    .iter()
                    .flatten()
                    .any(|&i| i as usize >= vertices.len())
                {
                    return Err(PhysError::InvalidShape("triangle index out of range"));
                }
                let verts: Vec<_> = vertices.iter().map(|p| vec(*p)).collect();
                SharedShape::trimesh(verts, indices.clone())
                    .map_err(|_| PhysError::InvalidShape("triangle mesh is empty or degenerate"))
            }
            Self::Heightfield {
                rows,
                cols,
                heights,
                scale,
            } => {
                if *rows < 2 || *cols < 2 || heights.len() != rows * cols {
                    return Err(PhysError::InvalidShape(
                        "heightfield needs rows * cols heights and at least 2x2 samples",
                    ));
                }
                let mut data = Vec::with_capacity(heights.len());
                for col in 0..*cols {
                    for row in 0..*rows {
                        data.push(heights[row * cols + col]);
                    }
                }
                Ok(SharedShape::heightfield(
                    Array2::new(*rows, *cols, data),
                    vec(*scale),
                ))
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
        let cube = [
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, -1.0, -1.0),
            Vec3::new(-1.0, 1.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
        ];
        let cases: Vec<(Shape3D, bool)> = vec![
            (Shape3D::sphere(0.5), true),
            (Shape3D::sphere(0.0), false),
            (Shape3D::sphere(f32::NAN), false),
            (Shape3D::cuboid(Vec3::splat(1.0)), true),
            (Shape3D::cuboid(Vec3::new(1.0, 0.0, 1.0)), false),
            (Shape3D::capsule(0.5, 0.25), true),
            (Shape3D::capsule(-1.0, 0.25), false),
            (Shape3D::ConvexHull(cube.to_vec()), true),
            (Shape3D::ConvexHull(vec![Vec3::ZERO, Vec3::X]), false),
            (
                Shape3D::TriMesh {
                    vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Z],
                    indices: vec![[0, 1, 2]],
                },
                true,
            ),
            (
                Shape3D::TriMesh {
                    vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Z],
                    indices: vec![[0, 1, 5]],
                },
                false,
            ),
            (
                Shape3D::Heightfield {
                    rows: 2,
                    cols: 3,
                    heights: vec![0.0; 6],
                    scale: Vec3::splat(1.0),
                },
                true,
            ),
            (
                Shape3D::Heightfield {
                    rows: 2,
                    cols: 3,
                    heights: vec![0.0; 5],
                    scale: Vec3::splat(1.0),
                },
                false,
            ),
            (Shape3D::Compound(vec![]), false),
            (
                Shape3D::Compound(vec![(Transform::IDENTITY, Shape3D::sphere(1.0))]),
                true,
            ),
        ];
        for (shape, ok) in cases {
            assert_eq!(shape.to_shared().is_ok(), ok, "{shape:?}");
        }
    }
}
