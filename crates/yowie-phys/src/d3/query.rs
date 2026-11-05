//! Scene queries: rays, overlaps, shape casts and signed distance.

use rapier3d::geometry::Ray;
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::pipeline::{QueryFilter as RFilter, QueryFilterFlags};
use yowie_core::{Quat, Transform, Vec3};

use super::convert::*;
use super::shape::Shape3D;
use super::world::World3D;
use crate::error::{PhysError, Result};
use crate::event::{RayHit, ShapeHit, SurfaceDistance};
use crate::ids::{ColliderId, QueryFilter};

fn filter(f: &QueryFilter) -> RFilter<'static> {
    let mut flags = QueryFilterFlags::empty();
    if f.exclude_sensors {
        flags |= QueryFilterFlags::EXCLUDE_SENSORS;
    }
    if f.exclude_static {
        flags |= QueryFilterFlags::EXCLUDE_FIXED;
    }
    RFilter {
        flags,
        groups: f.layers.map(groups),
        exclude_collider: f.exclude_collider.map(collider_handle),
        exclude_rigid_body: f.exclude_body.map(body_handle),
        predicate: None,
    }
}

impl World3D {
    /// Casts a ray and returns the nearest hit within `max_distance`.
    ///
    /// `direction` need not be normalised. A ray that starts inside a solid
    /// collider reports that collider at distance 0. Returns `None` when
    /// `direction` is zero.
    pub fn ray_cast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        f: &QueryFilter,
    ) -> Option<RayHit<Vec3>> {
        let dir = direction.try_normalize()?;
        let ray = Ray::new(vec(origin), vec(dir));
        let (h, hit) = self
            .pw
            .cast_ray_and_get_normal(&ray, max_distance, true, filter(f))?;
        let t = hit.time_of_impact;
        Some(RayHit {
            collider: collider_id(h),
            body: self.body_of(collider_id(h)),
            point: origin + dir * t,
            normal: unvec(hit.normal),
            distance: t,
        })
    }

    /// Returns every collider a ray passes through within `max_distance`,
    /// nearest first.
    pub fn ray_cast_all(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        f: &QueryFilter,
    ) -> Vec<RayHit<Vec3>> {
        let Some(dir) = direction.try_normalize() else {
            return Vec::new();
        };
        let ray = Ray::new(vec(origin), vec(dir));
        let mut hits: Vec<_> = self
            .pw
            .intersect_ray(ray, max_distance, true, filter(f))
            .map(|(h, _, hit)| RayHit {
                collider: collider_id(h),
                body: self.body_of(collider_id(h)),
                point: origin + dir * hit.time_of_impact,
                normal: unvec(hit.normal),
                distance: hit.time_of_impact,
            })
            .collect();
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        hits
    }

    /// Returns the colliders that contain `point`.
    pub fn overlap_point(&self, point: Vec3, f: &QueryFilter) -> Vec<ColliderId> {
        self.pw
            .intersect_point(vec(point), filter(f))
            .map(|(h, _)| collider_id(h))
            .collect()
    }

    /// Returns the colliders that overlap `shape` placed at `at`.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidShape`] when the shape cannot be built.
    pub fn overlap_shape(
        &self,
        shape: &Shape3D,
        at: &Transform,
        f: &QueryFilter,
    ) -> Result<Vec<ColliderId>> {
        let shared = shape.to_shared()?;
        Ok(self
            .pw
            .intersect_shape(transform_pose(at), &*shared, filter(f))
            .map(|(h, _)| collider_id(h))
            .collect())
    }

    /// Sweeps `shape` from `at` along `direction` for up to `max_distance`
    /// and returns the first collider it touches.
    ///
    /// # Errors
    ///
    /// Returns [`PhysError::InvalidShape`] when the shape cannot be built.
    pub fn shape_cast(
        &self,
        shape: &Shape3D,
        at: &Transform,
        direction: Vec3,
        max_distance: f32,
        f: &QueryFilter,
    ) -> Result<Option<ShapeHit<Vec3>>> {
        let shared = shape.to_shared()?;
        let Some(dir) = direction.try_normalize() else {
            return Ok(None);
        };
        let opts = ShapeCastOptions::with_max_time_of_impact(max_distance);
        let hit = self
            .pw
            .cast_shape(&transform_pose(at), vec(dir), &*shared, opts, filter(f));
        Ok(hit.map(|(h, hit)| ShapeHit {
            collider: collider_id(h),
            body: self.body_of(collider_id(h)),
            point: unvec(hit.witness1),
            normal: unvec(hit.normal1),
            distance: hit.time_of_impact,
        }))
    }

    /// Signed distance from `point` to the surface of a collider in the
    /// world. Returns `None` for a stale id.
    pub fn collider_distance(
        &self,
        collider: ColliderId,
        point: Vec3,
    ) -> Option<SurfaceDistance<Vec3>> {
        let c = self.pw.colliders.get(collider_handle(collider))?;
        let proj = c.shape().project_point(c.position(), vec(point), false);
        Some(distance_from(point, unvec(proj.point), proj.is_inside))
    }
}

/// Signed distance from `point` to a shape placed at `at`: positive outside,
/// negative inside. Closed shapes only; triangle meshes and heightfields
/// always report a non-negative distance.
///
/// # Errors
///
/// Returns [`PhysError::InvalidShape`] when the shape cannot be built.
pub fn signed_distance_3d(
    shape: &Shape3D,
    position: Vec3,
    rotation: Quat,
    point: Vec3,
) -> Result<SurfaceDistance<Vec3>> {
    let shared = shape.to_shared().map_err(|e| match e {
        PhysError::InvalidShape(m) => PhysError::InvalidShape(m),
        other => other,
    })?;
    let proj = shared.project_point(&pose(position, rotation), vec(point), false);
    Ok(distance_from(point, unvec(proj.point), proj.is_inside))
}

fn distance_from(point: Vec3, surface: Vec3, inside: bool) -> SurfaceDistance<Vec3> {
    let delta = point - surface;
    let len = delta.length();
    let sign = if inside { -1.0 } else { 1.0 };
    let normal = if len > 1.0e-6 {
        delta / len * sign
    } else {
        Vec3::Y
    };
    SurfaceDistance {
        distance: len * sign,
        point: surface,
        normal,
    }
}
