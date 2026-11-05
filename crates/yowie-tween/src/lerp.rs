use yowie_core::{Color, Quat, Transform, Transform2D, Vec2, Vec3, Vec4};

/// A value that can be interpolated.
///
/// `t` is 0 at `self` and 1 at `other`. Implementations do not clamp `t`,
/// so eases that overshoot (Back, Elastic) extrapolate.
pub trait Lerp: Sized {
    /// Returns the value `t` of the way from `self` to `other`.
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for f64 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * f64::from(t)
    }
}

macro_rules! lerp_via_glam {
    ($($ty:ty),*) => {$(
        impl Lerp for $ty {
            fn lerp(&self, other: &Self, t: f32) -> Self {
                <$ty>::lerp(*self, *other, t)
            }
        }
    )*};
}
lerp_via_glam!(Vec2, Vec3, Vec4);

/// Spherical interpolation along the shortest arc. Both inputs must be
/// normalised.
impl Lerp for Quat {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self.slerp(*other, t)
    }
}

/// Interpolates each channel linearly in the stored (sRGB) space.
impl Lerp for Color {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Color::lerp(*self, *other, t)
    }
}

/// Translation and scale linearly, rotation spherically.
impl Lerp for Transform {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Transform::lerp(self, other, t)
    }
}

/// Position and scale linearly, rotation along the shorter way round.
impl Lerp for Transform2D {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        Transform2D::lerp(self, other, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_and_vectors() {
        assert_eq!(Lerp::lerp(&2.0_f32, &4.0, 0.5), 3.0);
        assert_eq!(Lerp::lerp(&2.0_f64, &4.0, 0.25), 2.5);
        assert_eq!(
            Lerp::lerp(&Vec2::ZERO, &Vec2::new(2.0, 4.0), 0.5),
            Vec2::new(1.0, 2.0)
        );
        assert_eq!(Lerp::lerp(&Vec3::ZERO, &Vec3::ONE, 1.0), Vec3::ONE);
        assert_eq!(Lerp::lerp(&Vec4::ZERO, &Vec4::splat(2.0), 0.5), Vec4::ONE);
        // Extrapolation is allowed.
        assert_eq!(Lerp::lerp(&0.0_f32, &1.0, 2.0), 2.0);
    }

    #[test]
    fn quat_takes_the_arc() {
        let a = Quat::IDENTITY;
        let b = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mid = Lerp::lerp(&a, &b, 0.5);
        assert!(mid.angle_between(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)) < 1e-5);
        assert!((mid.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn color_and_transforms() {
        let c = Lerp::lerp(&Color::BLACK, &Color::WHITE, 0.5);
        assert!((c.r - 0.5).abs() < 1e-6 && (c.a - 1.0).abs() < 1e-6);
        let a = Transform::from_xyz(0.0, 0.0, 0.0);
        let b = Transform::from_xyz(2.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(1.0));
        let m = Lerp::lerp(&a, &b, 0.5);
        assert!((m.translation.x - 1.0).abs() < 1e-6);
        assert!(m.rotation.angle_between(Quat::from_rotation_y(0.5)) < 1e-5);
        let p = Transform2D::from_position(Vec2::X);
        let m2 = Lerp::lerp(&Transform2D::IDENTITY, &p, 0.5);
        assert!((m2.position.x - 0.5).abs() < 1e-6);
    }
}
