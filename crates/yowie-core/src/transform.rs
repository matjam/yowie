//! Position, rotation and scale in 2D and 3D.

use glam::{Affine2, Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// A 3D placement: scale, then rotate, then translate.
///
/// Composition follows matrix order: `parent * child` places `child` in
/// `parent`'s space. Non-uniform scale under a rotated parent cannot be
/// represented exactly (it shears); [`Transform::mul`] keeps the per-axis
/// product, which is what scene hierarchies conventionally do.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub const fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    pub const fn from_rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            ..Self::IDENTITY
        }
    }

    pub const fn from_scale(scale: Vec3) -> Self {
        Self {
            scale,
            ..Self::IDENTITY
        }
    }

    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self::from_translation(Vec3::new(x, y, z))
    }

    /// Decomposes an affine matrix. Shear and projection are lost.
    pub fn from_matrix(m: Mat4) -> Self {
        let (scale, rotation, translation) = m.to_scale_rotation_translation();
        Self {
            translation,
            rotation,
            scale,
        }
    }

    pub fn with_translation(self, translation: Vec3) -> Self {
        Self {
            translation,
            ..self
        }
    }

    pub fn with_rotation(self, rotation: Quat) -> Self {
        Self { rotation, ..self }
    }

    pub fn with_scale(self, scale: Vec3) -> Self {
        Self { scale, ..self }
    }

    /// Turns to face `target` with `up` as the approximate up direction.
    /// The local -Z axis points at the target, matching cameras and glTF.
    pub fn looking_at(self, target: Vec3, up: Vec3) -> Self {
        let forward = (target - self.translation).normalize_or_zero();
        if forward == Vec3::ZERO {
            return self;
        }
        let right = forward.cross(up).normalize_or(Vec3::X);
        let up = right.cross(forward);
        let rotation = Quat::from_mat3(&glam::Mat3::from_cols(right, up, -forward));
        Self { rotation, ..self }
    }

    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// `self * child`: `child` expressed in `self`'s space.
    pub fn mul(&self, child: &Transform) -> Transform {
        Transform {
            translation: self.transform_point(child.translation),
            rotation: self.rotation * child.rotation,
            scale: self.scale * child.scale,
        }
    }

    /// The transform that undoes this one. Exact for uniform scale.
    pub fn inverse(&self) -> Transform {
        let rotation = self.rotation.inverse();
        let scale = self.scale.recip();
        let translation = rotation * (-self.translation) * scale;
        Transform {
            translation,
            rotation,
            scale,
        }
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        self.rotation * (p * self.scale) + self.translation
    }

    /// Rotates and scales a direction; translation does not apply.
    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        self.rotation * (v * self.scale)
    }

    /// Local -Z in world space.
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// Interpolates translation and scale linearly and rotation spherically.
    pub fn lerp(&self, other: &Transform, t: f32) -> Transform {
        Transform {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation.slerp(other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

/// A 2D placement: scale, then rotate by `rotation` radians, then translate.
/// Positive rotation turns from +X towards +Y, which is clockwise on a
/// screen where `y` grows downwards.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform2D {
    pub position: Vec2,
    pub rotation: f32,
    pub scale: Vec2,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    pub const IDENTITY: Self = Self {
        position: Vec2::ZERO,
        rotation: 0.0,
        scale: Vec2::ONE,
    };

    pub const fn from_position(position: Vec2) -> Self {
        Self {
            position,
            ..Self::IDENTITY
        }
    }

    pub fn with_rotation(self, rotation: f32) -> Self {
        Self { rotation, ..self }
    }

    pub fn with_scale(self, scale: Vec2) -> Self {
        Self { scale, ..self }
    }

    pub fn to_affine(&self) -> Affine2 {
        Affine2::from_scale_angle_translation(self.scale, self.rotation, self.position)
    }

    pub fn transform_point(&self, p: Vec2) -> Vec2 {
        Vec2::from_angle(self.rotation).rotate(p * self.scale) + self.position
    }

    /// `self * child`: `child` expressed in `self`'s space.
    pub fn mul(&self, child: &Transform2D) -> Transform2D {
        Transform2D {
            position: self.transform_point(child.position),
            rotation: self.rotation + child.rotation,
            scale: self.scale * child.scale,
        }
    }

    pub fn lerp(&self, other: &Transform2D, t: f32) -> Transform2D {
        let turn = (other.rotation - self.rotation + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        Transform2D {
            position: self.position.lerp(other.position, t),
            rotation: self.rotation + turn * t,
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn mul_matches_matrices() {
        let parent = Transform::from_xyz(1.0, 2.0, 3.0)
            .with_rotation(Quat::from_rotation_y(0.7))
            .with_scale(Vec3::splat(2.0));
        let child = Transform::from_xyz(-1.0, 0.5, 4.0).with_rotation(Quat::from_rotation_x(0.3));
        let p = Vec3::new(0.2, -0.4, 1.0);
        let via_mul = parent.mul(&child).transform_point(p);
        let via_mat = (parent.to_matrix() * child.to_matrix()).transform_point3(p);
        assert!(close(via_mul, via_mat));
    }

    #[test]
    fn inverse_undoes() {
        let t = Transform::from_xyz(4.0, -2.0, 1.0)
            .with_rotation(Quat::from_rotation_z(1.1))
            .with_scale(Vec3::splat(3.0));
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert!(close(t.inverse().transform_point(t.transform_point(p)), p));
    }

    #[test]
    fn looking_at_points_forward() {
        let t = Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y);
        assert!(close(t.forward(), Vec3::NEG_Z));
        let t = Transform::from_xyz(0.0, 0.0, 0.0).looking_at(Vec3::X, Vec3::Y);
        assert!(close(t.forward(), Vec3::X));
        assert!(close(t.up(), Vec3::Y));
    }

    #[test]
    fn transform2d_rotation_and_lerp() {
        let t = Transform2D::from_position(Vec2::new(10.0, 0.0)).with_rotation(FRAC_PI_2);
        assert!((t.transform_point(Vec2::X) - Vec2::new(10.0, 1.0)).length() < 1e-5);
        assert!((t.to_affine().transform_point2(Vec2::X) - Vec2::new(10.0, 1.0)).length() < 1e-5);
        // Lerp takes the short way round the circle.
        let a = Transform2D::IDENTITY.with_rotation(3.0);
        let b = Transform2D::IDENTITY.with_rotation(-3.0);
        let mid = a.lerp(&b, 0.5).rotation;
        assert!((mid.rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs() < 1e-4);
    }
}
