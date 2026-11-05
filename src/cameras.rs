//! Cameras driven by input: a free-flying camera for looking around a
//! scene and an orbit camera for inspecting a model.

use yowie_core::{Vec2, Vec3};
use yowie_gfx::scene::Camera3D;
use yowie_input::{InputState, Key, MouseButton};

/// A free-flying camera: W, A, S and D move, Q and E go down and up,
/// Shift goes faster, and the view turns while the right mouse button is
/// held (or with every pointer movement when `always_look` is set, for a
/// captured cursor). Call [`update`](Self::update) each update and draw
/// with [`camera`](Self::camera).
///
/// ```no_run
/// # use yowie::{FlyCamera, Vec3};
/// let mut fly = FlyCamera::new(Vec3::new(0.0, 5.0, 20.0));
/// fly.look_at(Vec3::ZERO);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyCamera {
    /// The eye position.
    pub position: Vec3,
    /// Turn about +Y in radians; zero looks along -Z.
    pub yaw: f32,
    /// Turn up (positive) or down in radians, within about a right angle.
    pub pitch: f32,
    /// Movement speed in world units per second.
    pub speed: f32,
    /// Multiplies the speed while Shift is held.
    pub fast: f32,
    /// Radians of turn per view unit of pointer travel (or per unit of raw
    /// motion with `always_look`).
    pub sensitivity: f32,
    /// Turns the view with every pointer movement, using raw motion, not
    /// only while the right button is held.
    pub always_look: bool,
    /// The camera the position and angles are applied to: projection,
    /// depth range and viewport.
    pub lens: Camera3D,
}

impl FlyCamera {
    /// A camera at `position` looking along -Z.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            speed: 10.0,
            fast: 4.0,
            sensitivity: 0.004,
            always_look: false,
            lens: Camera3D::default(),
        }
    }

    /// The unit direction the camera looks along.
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(-sy * cp, sp, -cy * cp)
    }

    /// The unit direction to the camera's right, level with the ground.
    pub fn right(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cy, 0.0, -sy)
    }

    /// Turns the camera towards `target`.
    pub fn look_at(&mut self, target: Vec3) {
        let d = target - self.position;
        if d.length_squared() < 1e-12 {
            return;
        }
        let d = d.normalize();
        self.yaw = (-d.x).atan2(-d.z);
        self.pitch = d.y.clamp(-1.0, 1.0).asin();
    }

    /// Moves and turns the camera from this update's input over `dt`
    /// seconds.
    pub fn update(&mut self, input: &InputState, dt: f32) {
        let look = if self.always_look {
            Some(input.mouse_motion())
        } else if input.mouse_down(MouseButton::Right) {
            Some(input.mouse_delta())
        } else {
            None
        };
        if let Some(d) = look {
            self.yaw -= d.x * self.sensitivity;
            self.pitch = (self.pitch - d.y * self.sensitivity).clamp(-1.55, 1.55);
        }
        let mut speed = self.speed;
        if input.key_down(Key::LeftShift) || input.key_down(Key::RightShift) {
            speed *= self.fast;
        }
        let (f, r) = (self.forward(), self.right());
        let mut m = Vec3::ZERO;
        for (key, dir) in [
            (Key::W, f),
            (Key::S, -f),
            (Key::D, r),
            (Key::A, -r),
            (Key::E, Vec3::Y),
            (Key::Q, Vec3::NEG_Y),
        ] {
            if input.key_down(key) {
                m += dir;
            }
        }
        if m.length_squared() > 0.0 {
            self.position += m.normalize() * speed * dt;
        }
    }

    /// The camera to draw the scene with.
    pub fn camera(&self) -> Camera3D {
        Camera3D {
            position: self.position,
            target: self.position + self.forward(),
            up: Vec3::Y,
            ..self.lens
        }
    }
}

/// A camera circling a target: dragging with the left mouse button turns
/// it, the wheel moves it in and out, dragging with the right or middle
/// button moves the target across the view, and the arrow keys turn it.
/// Call [`update`](Self::update) each update and draw with
/// [`camera`](Self::camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// The point the camera circles and looks at.
    pub target: Vec3,
    /// Turn about +Y in radians; zero puts the camera on the +Z side.
    pub yaw: f32,
    /// Height angle in radians; positive looks down from above.
    pub pitch: f32,
    /// Distance from the target in world units.
    pub distance: f32,
    /// The nearest the wheel brings the camera.
    pub min_distance: f32,
    /// The farthest the wheel takes the camera.
    pub max_distance: f32,
    /// Radians of turn per view unit of drag.
    pub sensitivity: f32,
    /// The fraction of the distance one wheel line moves.
    pub zoom_step: f32,
    /// Radians per second the arrow keys turn.
    pub key_speed: f32,
    /// The camera the position is applied to: projection, depth range and
    /// viewport.
    pub lens: Camera3D,
}

impl OrbitCamera {
    /// A camera `distance` from `target` at the given angles.
    pub fn new(target: Vec3, yaw: f32, pitch: f32, distance: f32) -> Self {
        Self {
            target,
            yaw,
            pitch,
            distance,
            min_distance: 0.1,
            max_distance: 1.0e4,
            sensitivity: 0.008,
            zoom_step: 0.1,
            key_speed: 1.5,
            lens: Camera3D::default(),
        }
    }

    /// The eye position.
    pub fn position(&self) -> Vec3 {
        let pitch = self.pitch.clamp(-1.55, 1.55);
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target + Vec3::new(cp * sy, sp, cp * cy) * self.distance
    }

    /// Turns, zooms and pans from this update's input over `dt` seconds.
    pub fn update(&mut self, input: &InputState, dt: f32) {
        let d = input.mouse_delta();
        if input.mouse_down(MouseButton::Left) {
            self.turn(d * self.sensitivity);
        }
        let mut keys = Vec2::ZERO;
        for (key, v) in [
            (Key::Left, Vec2::new(-1.0, 0.0)),
            (Key::Right, Vec2::new(1.0, 0.0)),
            (Key::Up, Vec2::new(0.0, 1.0)),
            (Key::Down, Vec2::new(0.0, -1.0)),
        ] {
            if input.key_down(key) {
                keys += v;
            }
        }
        self.turn(keys * self.key_speed * dt);
        if input.mouse_down(MouseButton::Right) || input.mouse_down(MouseButton::Middle) {
            let cam = self.camera();
            let scale = self.distance * 0.0015;
            self.target += (cam.right() * -d.x + cam.right().cross(cam.forward()) * d.y) * scale;
        }
        let wheel = input.wheel().y;
        if wheel != 0.0 {
            self.distance = (self.distance * (1.0 - self.zoom_step).powf(wheel))
                .clamp(self.min_distance, self.max_distance);
        }
    }

    fn turn(&mut self, by: Vec2) {
        self.yaw -= by.x;
        self.pitch = (self.pitch + by.y).clamp(-1.55, 1.55);
    }

    /// The camera to draw the scene with.
    pub fn camera(&self) -> Camera3D {
        Camera3D {
            position: self.position(),
            target: self.target,
            up: Vec3::Y,
            ..self.lens
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fly_look_at_faces_the_target() {
        let mut fly = FlyCamera::new(Vec3::new(3.0, 4.0, 5.0));
        fly.look_at(Vec3::ZERO);
        let want = (Vec3::ZERO - fly.position).normalize();
        assert!((fly.forward() - want).length() < 1e-5);
        assert!(fly.right().dot(fly.forward()).abs() < 1e-5);
    }

    #[test]
    fn orbit_keeps_its_distance() {
        let orbit = OrbitCamera::new(Vec3::new(1.0, 2.0, 3.0), 0.7, 0.4, 5.0);
        assert!(((orbit.position() - orbit.target).length() - 5.0).abs() < 1e-4);
        let cam = orbit.camera();
        assert!((cam.forward() - (orbit.target - orbit.position()).normalize()).length() < 1e-5);
    }
}
