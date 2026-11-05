//! A 2D camera: which part of the world the view shows.

use yowie_core::{Mat4, Rect, Vec2, Vec3};

/// A 2D camera over a world measured in the same units as the view.
///
/// `position` is the world point drawn at the centre of the viewport.
/// `zoom` scales the world: 2 shows half as much. `rotation` turns the
/// camera clockwise by that many radians, so the world appears turned the
/// other way. `viewport` is the rectangle of the view the camera draws
/// into, in view units, for split screens and minimaps; `None` uses the
/// whole view.
///
/// Draw through a camera with
/// [`Graphics::with_camera`](crate::Graphics::with_camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera2D {
    /// The world point at the centre of the viewport.
    pub position: Vec2,
    /// World-to-view scale; zero or less means 1.
    pub zoom: f32,
    /// Clockwise turn of the camera in radians.
    pub rotation: f32,
    /// The part of the view the camera draws into, in view units.
    pub viewport: Option<Rect>,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            rotation: 0.0,
            viewport: None,
        }
    }
}

impl Camera2D {
    /// A camera centred on `position` at zoom 1.
    pub fn new(position: Vec2) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }

    /// The same camera with another zoom.
    pub fn with_zoom(self, zoom: f32) -> Self {
        Self { zoom, ..self }
    }

    /// The same camera with another rotation in radians.
    pub fn with_rotation(self, rotation: f32) -> Self {
        Self { rotation, ..self }
    }

    /// The same camera drawing into `viewport` (view units).
    pub fn with_viewport(self, viewport: Rect) -> Self {
        Self {
            viewport: Some(viewport),
            ..self
        }
    }

    fn zoom_or_one(&self) -> f32 {
        if self.zoom > 0.0 && self.zoom.is_finite() {
            self.zoom
        } else {
            1.0
        }
    }

    /// The viewport rectangle in a view of `view_size` units.
    pub fn viewport_rect(&self, view_size: Vec2) -> Rect {
        self.viewport
            .unwrap_or(Rect::new(0.0, 0.0, view_size.x, view_size.y))
    }

    /// The matrix from world coordinates to coordinates local to a viewport
    /// of `viewport_size` units, with the origin at its top-left.
    pub fn view_matrix(&self, viewport_size: Vec2) -> Mat4 {
        let half = (viewport_size * 0.5).extend(0.0);
        Mat4::from_translation(half)
            * Mat4::from_scale(Vec3::new(self.zoom_or_one(), self.zoom_or_one(), 1.0))
            * Mat4::from_rotation_z(-self.rotation)
            * Mat4::from_translation(-self.position.extend(0.0))
    }

    /// Where a world point appears, in view units of a view `view_size`
    /// across.
    pub fn world_to_screen(&self, world: Vec2, view_size: Vec2) -> Vec2 {
        let vp = self.viewport_rect(view_size);
        let local =
            Vec2::from_angle(-self.rotation).rotate(world - self.position) * self.zoom_or_one();
        local + vp.center()
    }

    /// The world point under a position in view units, such as the mouse.
    pub fn screen_to_world(&self, screen: Vec2, view_size: Vec2) -> Vec2 {
        let vp = self.viewport_rect(view_size);
        let local = (screen - vp.center()) / self.zoom_or_one();
        Vec2::from_angle(self.rotation).rotate(local) + self.position
    }

    /// The smallest world rectangle containing everything the camera shows;
    /// use it to skip drawing what is off screen.
    pub fn visible_rect(&self, view_size: Vec2) -> Rect {
        let vp = self.viewport_rect(view_size);
        let corners = [
            vp.min(),
            Vec2::new(vp.x + vp.w, vp.y),
            vp.max(),
            Vec2::new(vp.x, vp.y + vp.h),
        ];
        Rect::bounding(corners.map(|c| self.screen_to_world(c, view_size))).unwrap_or(Rect::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::vec2;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn world_screen_round_trip_and_matrix_agree() {
        let view = vec2(800.0, 600.0);
        let cams = [
            Camera2D::new(vec2(100.0, 50.0)),
            Camera2D::new(vec2(-30.0, 20.0)).with_zoom(2.0),
            Camera2D::new(vec2(5.0, 5.0))
                .with_rotation(0.7)
                .with_zoom(0.5),
            Camera2D::new(vec2(0.0, 0.0)).with_viewport(Rect::new(400.0, 0.0, 400.0, 300.0)),
        ];
        for cam in cams {
            let w = vec2(123.0, -45.0);
            let s = cam.world_to_screen(w, view);
            assert!(close(cam.screen_to_world(s, view), w), "{cam:?}");
            let vp = cam.viewport_rect(view);
            let local = cam.view_matrix(vp.size()).transform_point3(w.extend(0.0));
            assert!(close(local.truncate() + vp.min(), s), "{cam:?}");
        }
    }

    #[test]
    fn centre_and_visible_rect() {
        let view = vec2(800.0, 600.0);
        let cam = Camera2D::new(vec2(100.0, 100.0)).with_zoom(2.0);
        assert!(close(
            cam.world_to_screen(vec2(100.0, 100.0), view),
            vec2(400.0, 300.0)
        ));
        let r = cam.visible_rect(view);
        assert!(close(r.min(), vec2(-100.0, -50.0)) && close(r.size(), vec2(400.0, 300.0)));
        let turned = Camera2D::new(Vec2::ZERO).with_rotation(std::f32::consts::FRAC_PI_2);
        let r = turned.visible_rect(view);
        assert!(close(r.size(), vec2(600.0, 800.0)));
    }
}
