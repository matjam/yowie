//! Bounding boxes, frustums and rays for culling and picking.

use yowie_core::{Mat4, Vec3, Vec4};

/// An axis-aligned box from `min` to `max`. An empty box has `min`
/// greater than `max` on some axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    /// The corner with the smallest coordinates.
    pub min: Vec3,
    /// The corner with the largest coordinates.
    pub max: Vec3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    /// A box holding nothing; growing it by a point gives that point.
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    /// The box from `min` to `max`.
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// The smallest box holding every point, or [`Aabb::EMPTY`].
    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        points.into_iter().fold(Self::EMPTY, |b, p| b.grown(p))
    }

    /// Whether the box holds no point.
    pub fn is_empty(&self) -> bool {
        self.min.cmpgt(self.max).any()
    }

    /// The box grown to hold `p`.
    pub fn grown(self, p: Vec3) -> Self {
        Self {
            min: self.min.min(p),
            max: self.max.max(p),
        }
    }

    /// The smallest box holding both boxes.
    pub fn union(self, other: Aabb) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The centre point.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half the size on each axis.
    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// The radius of the sphere around the centre that holds the box.
    pub fn radius(&self) -> f32 {
        self.half_extents().length()
    }

    /// The box holding this box after `m` moves it.
    pub fn transformed(&self, m: &Mat4) -> Aabb {
        if self.is_empty() {
            return *self;
        }
        let c = m.transform_point3(self.center());
        let h = self.half_extents();
        let e = Vec3::new(
            m.x_axis.x.abs() * h.x + m.y_axis.x.abs() * h.y + m.z_axis.x.abs() * h.z,
            m.x_axis.y.abs() * h.x + m.y_axis.y.abs() * h.y + m.z_axis.y.abs() * h.z,
            m.x_axis.z.abs() * h.x + m.y_axis.z.abs() * h.y + m.z_axis.z.abs() * h.z,
        );
        Aabb::new(c - e, c + e)
    }

    /// The distance along `ray` to where it enters the box, 0 when it
    /// starts inside, or `None` when it misses.
    pub fn intersect_ray(&self, ray: &Ray) -> Option<f32> {
        let inv = ray.direction.recip();
        let t1 = (self.min - ray.origin) * inv;
        let t2 = (self.max - ray.origin) * inv;
        let near = t1.min(t2).max_element();
        let far = t1.max(t2).min_element();
        (far >= near.max(0.0)).then_some(near.max(0.0))
    }
}

/// A half-line from `origin` along `direction`, which is unit length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    /// Where the ray starts.
    pub origin: Vec3,
    /// The unit direction it travels.
    pub direction: Vec3,
}

impl Ray {
    /// A ray from `origin` towards `direction`, normalised here.
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self {
            origin,
            direction: direction.normalize_or(Vec3::NEG_Z),
        }
    }

    /// The point `t` units along the ray.
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }

    /// The ray after `m` moves it; the direction is renormalised, so
    /// distances along the result are in the moved space.
    pub fn transformed(&self, m: &Mat4) -> Ray {
        Ray::new(
            m.transform_point3(self.origin),
            m.transform_vector3(self.direction),
        )
    }

    /// The distance along the ray to triangle `a b c` (either winding),
    /// or `None` when it misses.
    pub fn intersect_triangle(&self, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
        let e1 = b - a;
        let e2 = c - a;
        let p = self.direction.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = self.direction.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = e2.dot(q) * inv;
        (t >= 0.0).then_some(t)
    }

    /// The distance along the ray to the plane through `point` facing
    /// `normal`, or `None` when the ray is parallel to it or points away.
    pub fn intersect_plane(&self, point: Vec3, normal: Vec3) -> Option<f32> {
        let d = self.direction.dot(normal);
        if d.abs() < 1e-9 {
            return None;
        }
        let t = (point - self.origin).dot(normal) / d;
        (t >= 0.0).then_some(t)
    }
}

/// The six planes bounding what a camera sees. Each plane is
/// `(normal, distance)` with the normal pointing inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// Left, right, bottom, top, and the two depth planes.
    pub planes: [Vec4; 6],
}

impl Frustum {
    /// The frustum of a view-projection matrix whose clip depth runs from 0
    /// to 1 in either direction (Yowie's cameras use reversed depth).
    pub fn from_view_proj(m: &Mat4) -> Self {
        let r = m.transpose();
        let (r0, r1, r2, r3) = (r.x_axis, r.y_axis, r.z_axis, r.w_axis);
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| {
            let len = p.truncate().length();
            if len > 0.0 { p / len } else { p }
        });
        Self { planes }
    }

    /// Whether a sphere touches the inside of the frustum.
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes
            .iter()
            .all(|p| p.truncate().dot(center) + p.w >= -radius)
    }

    /// Whether a box touches the inside of the frustum. Boxes outside near
    /// a corner can pass; the test never rejects a visible box.
    pub fn intersects_aabb(&self, b: &Aabb) -> bool {
        self.planes.iter().all(|p| {
            let n = p.truncate();
            let far = Vec3::select(n.cmpge(Vec3::ZERO), b.max, b.min);
            n.dot(far) + p.w >= 0.0
        })
    }

    /// Whether a point is inside the frustum.
    pub fn contains_point(&self, p: Vec3) -> bool {
        self.intersects_sphere(p, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_transforms_and_rays() {
        let b = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        let m = Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0))
            * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4);
        let t = b.transformed(&m);
        assert!((t.center() - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-5);
        assert!((t.half_extents().x - 2f32.sqrt()).abs() < 1e-5);
        let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z);
        assert_eq!(b.intersect_ray(&ray), Some(9.0));
        let miss = Ray::new(Vec3::new(3.0, 0.0, 10.0), Vec3::NEG_Z);
        assert_eq!(b.intersect_ray(&miss), None);
        let tri = ray.intersect_triangle(
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert_eq!(tri, Some(10.0));
    }

    #[test]
    fn frustum_culls_spheres() {
        use yowie_core::glam::camera::rh::{proj::directx, view::look_at_mat4};
        let proj = directx::perspective(1.0, 1.0, 100.0, 0.1);
        let view = look_at_mat4(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let f = Frustum::from_view_proj(&(proj * view));
        let cases = [
            (Vec3::new(0.0, 0.0, -5.0), 0.5, true),
            (Vec3::new(0.0, 0.0, 5.0), 0.5, false),
            (Vec3::new(50.0, 0.0, -5.0), 0.5, false),
            (Vec3::new(0.0, 0.0, -200.0), 1.0, false),
            (Vec3::new(3.0, 0.0, -5.0), 1.0, true),
        ];
        for (c, r, want) in cases {
            assert_eq!(f.intersects_sphere(c, r), want, "{c:?}");
            let b = Aabb::new(c - Vec3::splat(r), c + Vec3::splat(r));
            assert_eq!(f.intersects_aabb(&b), want, "{c:?}");
        }
    }
}
