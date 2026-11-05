//! A camera that follows a target, stays inside bounds and shakes.

use yowie_core::{Rect, Vec2, vec2};

use crate::Camera2D;

impl Camera2D {
    /// Moves the camera so the view stays inside `bounds`, a world
    /// rectangle. `view_size` is the size of the view in view units. When
    /// the bounds are smaller than the view along an axis, the camera
    /// centres on the bounds along that axis. Rotation is ignored.
    pub fn clamp_to(&mut self, bounds: Rect, view_size: Vec2) {
        let zoom = if self.zoom > 0.0 && self.zoom.is_finite() {
            self.zoom
        } else {
            1.0
        };
        let half = view_size * 0.5 / zoom;
        let axis = |pos: f32, lo: f32, size: f32, half: f32| {
            if size <= 2.0 * half {
                lo + size * 0.5
            } else {
                pos.clamp(lo + half, lo + size - half)
            }
        };
        self.position = vec2(
            axis(self.position.x, bounds.x, bounds.w, half.x),
            axis(self.position.y, bounds.y, bounds.h, half.y),
        );
    }
}

/// Moves a [`Camera2D`] after a target with a dead zone and smoothing,
/// keeps it inside bounds, and adds screen shake driven by trauma.
///
/// Call [`follow`](Self::follow) and [`update`](Self::update) once per
/// update step, then draw through [`camera`](Self::camera), which includes
/// the shake. [`base`](Self::base) is the camera without the shake, for
/// converting mouse positions to the world without jitter.
///
/// Trauma is a value from 0 to 1. The shake strength is the square of the
/// trauma, so small hits barely move the view and large ones shake hard;
/// trauma falls by [`trauma_decay`](Self::trauma_decay) per second.
///
/// ```
/// use yowie_core::{Rect, vec2};
/// use yowie_gfx::CameraRig;
///
/// let mut rig = CameraRig::new(vec2(0.0, 0.0), vec2(960.0, 540.0));
/// rig.bounds = Some(Rect::new(0.0, 0.0, 4000.0, 2000.0));
/// rig.smoothing = 6.0;
/// for _ in 0..120 {
///     rig.follow(vec2(1500.0, 900.0), 1.0 / 60.0);
///     rig.update(1.0 / 60.0);
/// }
/// assert!((rig.base().position - vec2(1500.0, 900.0)).length() < 1.0);
/// rig.add_trauma(0.8);
/// ```
#[derive(Clone, Debug)]
pub struct CameraRig {
    camera: Camera2D,
    /// The size of the view in view units, used to clamp to the bounds.
    pub view_size: Vec2,
    /// Size of the rectangle around the camera centre, in world units,
    /// inside which the target can move without moving the camera. Zero
    /// follows the target exactly.
    pub dead_zone: Vec2,
    /// How fast the camera closes on its goal, in 1/seconds. The remaining
    /// distance shrinks by `exp(-smoothing * dt)` each step; zero or less
    /// snaps to the goal.
    pub smoothing: f32,
    /// World rectangle the view must stay inside; `None` for no limit.
    pub bounds: Option<Rect>,
    /// How much trauma is lost each second.
    pub trauma_decay: f32,
    /// Largest shake offset in world units at full strength.
    pub max_offset: Vec2,
    /// Largest shake rotation in radians at full strength.
    pub max_angle: f32,
    /// Shake noise frequency in cycles per second.
    pub shake_frequency: f32,
    trauma: f32,
    time: f32,
}

impl CameraRig {
    /// A rig centred on `position` for a view of `view_size` units, with
    /// no dead zone, no smoothing and no bounds.
    pub fn new(position: Vec2, view_size: Vec2) -> Self {
        Self {
            camera: Camera2D::new(position),
            view_size,
            dead_zone: Vec2::ZERO,
            smoothing: 0.0,
            bounds: None,
            trauma_decay: 1.0,
            max_offset: vec2(16.0, 16.0),
            max_angle: 0.04,
            shake_frequency: 22.0,
            trauma: 0.0,
            time: 0.0,
        }
    }

    /// Sets the zoom. Zero or less means 1.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.camera.zoom = zoom;
    }

    /// Sets the base rotation in radians, clockwise.
    pub fn set_rotation(&mut self, rotation: f32) {
        self.camera.rotation = rotation;
    }

    /// Puts the camera on `position` at once, then applies the bounds.
    pub fn snap_to(&mut self, position: Vec2) {
        self.camera.position = position;
        self.apply_bounds();
    }

    /// The camera without shake.
    pub fn base(&self) -> Camera2D {
        self.camera
    }

    /// Mutable access to the camera without shake, for setting zoom,
    /// rotation or a viewport.
    pub fn base_mut(&mut self) -> &mut Camera2D {
        &mut self.camera
    }

    /// Moves the camera towards `target` (a world position) for a step of
    /// `dt` seconds: the camera first takes the smallest move that puts
    /// the target on the dead zone's edge, then closes on that goal by the
    /// smoothing rate, then the bounds apply.
    pub fn follow(&mut self, target: Vec2, dt: f32) {
        let half = self.dead_zone * 0.5;
        let delta = target - self.camera.position;
        let goal = self.camera.position
            + vec2(
                outside(delta.x, half.x.max(0.0)),
                outside(delta.y, half.y.max(0.0)),
            );
        self.camera.position = if self.smoothing <= 0.0 || dt <= 0.0 {
            goal
        } else {
            let t = 1.0 - (-self.smoothing * dt).exp();
            self.camera.position + (goal - self.camera.position) * t
        };
        self.apply_bounds();
    }

    /// Adds trauma, clamped to 1.
    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount.max(0.0)).min(1.0);
    }

    /// The trauma left, from 0 to 1.
    pub fn trauma(&self) -> f32 {
        self.trauma
    }

    /// Whether any shake remains.
    pub fn shaking(&self) -> bool {
        self.trauma > 0.0
    }

    /// Advances the shake by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        self.time += dt.max(0.0);
        self.trauma = (self.trauma - self.trauma_decay * dt.max(0.0)).max(0.0);
    }

    /// The camera to draw through: the base camera moved and turned by the
    /// current shake.
    pub fn camera(&self) -> Camera2D {
        let strength = self.trauma * self.trauma;
        if strength <= 0.0 {
            return self.camera;
        }
        let t = self.time * self.shake_frequency;
        let n = |channel: u32| noise(t, channel);
        Camera2D {
            position: self.camera.position
                + vec2(n(0) * self.max_offset.x, n(1) * self.max_offset.y) * strength,
            rotation: self.camera.rotation + n(2) * self.max_angle * strength,
            ..self.camera
        }
    }

    fn apply_bounds(&mut self) {
        if let Some(b) = self.bounds {
            self.camera.clamp_to(b, self.view_size);
        }
    }
}

/// How far `delta` lies beyond `half` on one side, signed; zero inside.
fn outside(delta: f32, half: f32) -> f32 {
    if delta > half {
        delta - half
    } else if delta < -half {
        delta + half
    } else {
        0.0
    }
}

/// Smooth value noise in -1..1 along `t`, one independent stream per
/// `channel`.
fn noise(t: f32, channel: u32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let s = f * f * (3.0 - 2.0 * f);
    let a = lattice(i as i32, channel);
    let b = lattice(i as i32 + 1, channel);
    a + (b - a) * s
}

fn lattice(i: i32, channel: u32) -> f32 {
    let mut x = (i as u32).wrapping_mul(0x9e37_79b1) ^ channel.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn follow_converges_on_a_moving_then_still_target() {
        let mut rig = CameraRig::new(Vec2::ZERO, vec2(800.0, 600.0));
        rig.smoothing = 5.0;
        rig.dead_zone = vec2(40.0, 20.0);
        let target = vec2(500.0, -300.0);
        let mut last = f32::MAX;
        for _ in 0..300 {
            rig.follow(target, DT);
            let d = (rig.base().position - target).length();
            assert!(d <= last + 1e-3, "distance grew from {last} to {d}");
            last = d;
        }
        // The camera stops at the dead zone's edge, half its size away.
        let p = rig.base().position;
        assert!(
            (p.x - 480.0).abs() < 0.5 && (p.y - (-290.0)).abs() < 0.5,
            "{p:?}"
        );
    }

    #[test]
    fn dead_zone_holds_the_camera_still() {
        let mut rig = CameraRig::new(vec2(100.0, 100.0), vec2(800.0, 600.0));
        rig.dead_zone = vec2(100.0, 100.0);
        rig.follow(vec2(130.0, 80.0), DT);
        assert_eq!(rig.base().position, vec2(100.0, 100.0));
        rig.follow(vec2(200.0, 100.0), DT);
        assert_eq!(rig.base().position, vec2(150.0, 100.0));
    }

    #[test]
    fn bounds_clamp_and_centre() {
        let mut rig = CameraRig::new(Vec2::ZERO, vec2(200.0, 100.0));
        rig.bounds = Some(Rect::new(0.0, 0.0, 1000.0, 90.0));
        rig.follow(vec2(-500.0, 500.0), DT);
        // Left edge: half the view from the bounds; vertical: centred, as
        // the bounds are shorter than the view.
        assert_eq!(rig.base().position, vec2(100.0, 45.0));
        rig.set_zoom(2.0);
        rig.follow(vec2(5000.0, 0.0), DT);
        assert_eq!(rig.base().position.x, 950.0);
    }

    #[test]
    fn shake_decays_to_nothing() {
        let mut rig = CameraRig::new(vec2(10.0, 10.0), vec2(800.0, 600.0));
        rig.add_trauma(1.0);
        let mut worst = Vec::new();
        for step in 0..70 {
            let off = (rig.camera().position - rig.base().position).length();
            if step % 10 == 0 {
                worst.push(off);
            }
            rig.update(DT);
        }
        assert!(worst[0] > 0.0, "a full-trauma shake moves the camera");
        assert!(!rig.shaking());
        assert_eq!(rig.camera(), rig.base());
        let smooth = |trauma: f32| {
            let mut r = CameraRig::new(Vec2::ZERO, vec2(1.0, 1.0));
            r.add_trauma(trauma);
            let mut peak = 0.0f32;
            for _ in 0..30 {
                peak = peak.max(r.camera().position.length());
                r.update(0.0);
                r.time += DT;
            }
            peak
        };
        assert!(smooth(0.9) > smooth(0.3), "more trauma shakes harder");
    }
}
