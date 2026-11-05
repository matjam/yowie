//! Scene queries: rays, overlaps, shape casts and signed distance.

use rapier2d::geometry::Ray;
use rapier2d::parry::query::ShapeCastOptions;
use rapier2d::pipeline::{QueryFilter as RFilter, QueryFilterFlags};
use yowie_core::{Transform2D, Vec2};

use super::convert::*;
use super::shape::Shape2D;
use super::world::World2D;
use crate::error::Result;
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

impl World2D {
    /// Casts a ray and returns the nearest hit within `max_distance`.
    ///
    /// `direction` need not be normalised. A ray that starts inside a solid
    /// collider reports that collider at distance 0. Returns `None` when
    /// `direction` is zero.
    pub fn ray_cast(
        &self,
        origin: Vec2,
        direction: Vec2,
        max_distance: f32,
        f: &QueryFilter,
    ) -> Option<RayHit<Vec2>> {
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
        origin: Vec2,
        direction: Vec2,
        max_distance: f32,
        f: &QueryFilter,
    ) -> Vec<RayHit<Vec2>> {
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
    pub fn overlap_point(&self, point: Vec2, f: &QueryFilter) -> Vec<ColliderId> {
        self.pw
            .intersect_point(vec(point), filter(f))
            .map(|(h, _)| collider_id(h))
            .collect()
    }

    /// Returns the colliders that overlap `shape` placed at `at`.
    ///
    /// # Errors
    ///
    /// Returns an error when the shape cannot be built.
    pub fn overlap_shape(
        &self,
        shape: &Shape2D,
        at: &Transform2D,
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
    /// Returns an error when the shape cannot be built.
    pub fn shape_cast(
        &self,
        shape: &Shape2D,
        at: &Transform2D,
        direction: Vec2,
        max_distance: f32,
        f: &QueryFilter,
    ) -> Result<Option<ShapeHit<Vec2>>> {
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
        point: Vec2,
    ) -> Option<SurfaceDistance<Vec2>> {
        let c = self.pw.colliders.get(collider_handle(collider))?;
        let proj = c.shape().project_point(c.position(), vec(point), false);
        Some(distance_from(point, unvec(proj.point), proj.is_inside))
    }
}

/// Signed distance from `point` to a shape placed at `position` and
/// `angle`: positive outside, negative inside. Segments and polylines always
/// report a non-negative distance.
///
/// # Errors
///
/// Returns an error when the shape cannot be built.
pub fn signed_distance_2d(
    shape: &Shape2D,
    position: Vec2,
    angle: f32,
    point: Vec2,
) -> Result<SurfaceDistance<Vec2>> {
    let shared = shape.to_shared()?;
    let proj = shared.project_point(&pose(position, angle), vec(point), false);
    Ok(distance_from(point, unvec(proj.point), proj.is_inside))
}

fn distance_from(point: Vec2, surface: Vec2, inside: bool) -> SurfaceDistance<Vec2> {
    let delta = point - surface;
    let len = delta.length();
    let sign = if inside { -1.0 } else { 1.0 };
    let normal = if len > 1.0e-6 {
        delta / len * sign
    } else {
        Vec2::Y
    };
    SurfaceDistance {
        distance: len * sign,
        point: surface,
        normal,
    }
}
