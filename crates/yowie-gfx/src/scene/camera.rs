//! 3D cameras: where the eye is, how it projects, and conversions between
//! the world and the 2D view.

use yowie_core::glam::camera::rh::proj::directx;
use yowie_core::glam::camera::rh::view::look_at_mat4;
use yowie_core::{Mat4, Rect, Vec2, Vec3, Vec4};

use super::bounds::{Frustum, Ray};

/// How a [`Camera3D`] maps the world onto the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Distance shrinks things. `fov_y` is the vertical field of view in
    /// radians.
    Perspective {
        /// Vertical field of view in radians.
        fov_y: f32,
    },
    /// Distance does not change size. `half_height` is half the view's
    /// height in world units.
    Orthographic {
        /// Half the visible height in world units.
        half_height: f32,
    },
}

impl Default for Projection {
    fn default() -> Self {
        Projection::Perspective {
            fov_y: 60f32.to_radians(),
        }
    }
}

/// A camera looking from `position` at `target` in a right-handed world
/// with +Y up.
///
/// Depth is reversed on the GPU (near maps to 1, far to 0), which keeps
/// precision at a distance; [`Camera3D::projection_matrix`] and
/// [`Camera3D::view_proj`] return that form. `viewport` is the part of the
/// 2D view the scene draws into, in view units; `None` uses the whole
/// view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera3D {
    /// The eye position.
    pub position: Vec3,
    /// The point the camera looks at.
    pub target: Vec3,
    /// The world direction that appears up; must not be parallel to the
    /// view direction.
    pub up: Vec3,
    /// Perspective or orthographic.
    pub projection: Projection,
    /// The nearest visible distance in world units, above zero.
    pub near: f32,
    /// The farthest visible distance in world units.
    pub far: f32,
    /// The rectangle of the view the scene draws into, in view units.
    pub viewport: Option<Rect>,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 5.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::default(),
            near: 0.1,
            far: 1000.0,
            viewport: None,
        }
    }
}

impl Camera3D {
    /// A perspective camera at `position` looking at `target` with a
    /// vertical field of view of `fov_y` radians.
    pub fn perspective(position: Vec3, target: Vec3, fov_y: f32) -> Self {
        Self {
            position,
            target,
            projection: Projection::Perspective { fov_y },
            ..Self::default()
        }
    }

    /// An orthographic camera at `position` looking at `target`, showing
    /// `half_height` world units above and below the centre.
    pub fn orthographic(position: Vec3, target: Vec3, half_height: f32) -> Self {
        Self {
            position,
            target,
            projection: Projection::Orthographic { half_height },
            ..Self::default()
        }
    }

    /// The same camera drawing into `viewport` (view units).
    pub fn with_viewport(self, viewport: Rect) -> Self {
        Self {
            viewport: Some(viewport),
            ..self
        }
    }

    /// The same camera with other near and far distances.
    pub fn with_depth_range(self, near: f32, far: f32) -> Self {
        Self { near, far, ..self }
    }

    /// Turns the camera to look at `target`.
    pub fn look_at(&mut self, target: Vec3) {
        self.target = target;
    }

    /// The unit direction the camera looks in.
    pub fn forward(&self) -> Vec3 {
        (self.target - self.position).normalize_or(Vec3::NEG_Z)
    }

    /// The unit direction to the camera's right.
    pub fn right(&self) -> Vec3 {
        self.forward().cross(self.up).normalize_or(Vec3::X)
    }

    fn near_far(&self) -> (f32, f32) {
        let near = if self.near > 0.0 { self.near } else { 0.1 };
        let far = if self.far > near {
            self.far
        } else {
            near * 1e4
        };
        (near, far)
    }

    /// The matrix from world space to view space (the camera at the
    /// origin looking down -Z).
    pub fn view_matrix(&self) -> Mat4 {
        let fwd = self.forward();
        let up = if fwd.cross(self.up).length_squared() < 1e-10 {
            if fwd.y.abs() < 0.9 { Vec3::Y } else { Vec3::Z }
        } else {
            self.up
        };
        look_at_mat4(self.position, self.position + fwd, up)
    }

    /// The projection for a viewport `aspect` (width over height) wide,
    /// with reversed depth: near maps to 1 and far to 0.
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        let (near, far) = self.near_far();
        let aspect = if aspect > 0.0 && aspect.is_finite() {
            aspect
        } else {
            1.0
        };
        match self.projection {
            Projection::Perspective { fov_y } => {
                let fov = if fov_y > 0.0 {
                    fov_y
                } else {
                    60f32.to_radians()
                };
                directx::perspective(fov, aspect, far, near)
            }
            Projection::Orthographic { half_height } => {
                let h = if half_height > 0.0 { half_height } else { 1.0 };
                let w = h * aspect;
                directx::orthographic(-w, w, -h, h, far, near)
            }
        }
    }

    /// Projection times view for a viewport `aspect` wide.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    /// What the camera sees in a viewport `aspect` wide.
    pub fn frustum(&self, aspect: f32) -> Frustum {
        Frustum::from_view_proj(&self.view_proj(aspect))
    }

    /// The viewport rectangle in a view of `view_size` units.
    pub fn viewport_rect(&self, view_size: Vec2) -> Rect {
        self.viewport
            .unwrap_or(Rect::new(0.0, 0.0, view_size.x, view_size.y))
    }

    /// Where a world point appears in a view of `view_size` units, or
    /// `None` when it is behind the camera. Use it to place 2D labels over
    /// 3D objects.
    pub fn world_to_screen(&self, world: Vec3, view_size: Vec2) -> Option<Vec2> {
        let vp = self.viewport_rect(view_size);
        let clip = self.view_proj(vp.w / vp.h.max(1e-6)) * world.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new(
            vp.x + (ndc.x * 0.5 + 0.5) * vp.w,
            vp.y + (0.5 - ndc.y * 0.5) * vp.h,
        ))
    }

    /// The world ray through a point of the view (in view units, like the
    /// mouse position) in a view of `view_size` units. Use it for picking.
    pub fn screen_ray(&self, screen: Vec2, view_size: Vec2) -> Ray {
        let vp = self.viewport_rect(view_size);
        let ndc = Vec2::new(
            (screen.x - vp.x) / vp.w.max(1e-6) * 2.0 - 1.0,
            1.0 - (screen.y - vp.y) / vp.h.max(1e-6) * 2.0,
        );
        let inv = self.view_proj(vp.w / vp.h.max(1e-6)).inverse();
        let unproject = |z: f32| {
            let p = inv * Vec4::new(ndc.x, ndc.y, z, 1.0);
            p.truncate() / p.w
        };
        // Reversed depth: 1 is the near plane, 0 the far one.
        let near = unproject(1.0);
        let far = unproject(0.0);
        Ray::new(near, far - near)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::vec2;

    #[test]
    fn screen_and_world_agree() {
        let view = vec2(800.0, 600.0);
        let cams = [
            Camera3D::perspective(Vec3::new(3.0, 4.0, 10.0), Vec3::ZERO, 0.9),
            Camera3D::orthographic(Vec3::new(0.0, 10.0, 10.0), Vec3::ZERO, 5.0),
            Camera3D::perspective(Vec3::new(0.0, 2.0, 6.0), Vec3::Y, 1.2)
                .with_viewport(Rect::new(400.0, 0.0, 400.0, 300.0)),
        ];
        for cam in cams {
            let p = Vec3::new(0.5, 0.25, -0.5);
            let s = cam.world_to_screen(p, view).expect("in front");
            let ray = cam.screen_ray(s, view);
            let to_p = (p - ray.origin).normalize();
            assert!(to_p.dot(ray.direction) > 0.99999, "{cam:?}");
            let centre = cam.world_to_screen(cam.target, view).expect("in front");
            assert!((centre - cam.viewport_rect(view).center()).length() < 1e-2);
        }
        let cam = Camera3D::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 1.0);
        assert!(
            cam.world_to_screen(Vec3::new(0.0, 0.0, 10.0), view)
                .is_none()
        );
    }

    #[test]
    fn reversed_depth() {
        let cam = Camera3D::perspective(Vec3::ZERO, Vec3::NEG_Z, 1.0).with_depth_range(1.0, 100.0);
        let m = cam.view_proj(1.0);
        let near = m * Vec4::new(0.0, 0.0, -1.0, 1.0);
        let far = m * Vec4::new(0.0, 0.0, -100.0, 1.0);
        assert!((near.z / near.w - 1.0).abs() < 1e-5);
        assert!((far.z / far.w).abs() < 1e-5);
    }
}
