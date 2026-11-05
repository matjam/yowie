//! The value operations curves need.

use yowie_core::{Color, Quat, Transform, Transform2D, Vec2, Vec3, Vec4};

/// A value a [`Curve`](crate::Curve) can interpolate.
pub trait Animatable: Clone {
    /// The weighted sum of four values. Cubic interpolation is built from
    /// this; rotations normalise the result.
    fn weighted_sum(terms: [(f32, &Self); 4]) -> Self;

    /// Linear interpolation from `self` to `other`; `t` is 0 to 1.
    /// Rotations take the shortest arc.
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

macro_rules! linear_animatable {
    ($($ty:ty),*) => {$(
        impl Animatable for $ty {
            fn weighted_sum(t: [(f32, &Self); 4]) -> Self {
                *t[0].1 * t[0].0 + *t[1].1 * t[1].0 + *t[2].1 * t[2].0 + *t[3].1 * t[3].0
            }
            fn lerp(&self, other: &Self, t: f32) -> Self {
                *self + (*other - *self) * t
            }
        }
    )*};
}

linear_animatable!(f32, Vec2, Vec3, Vec4);

/// Normalises a quaternion, or returns identity when it has no length.
fn normalized(q: Quat) -> Quat {
    if q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

impl Animatable for Quat {
    fn weighted_sum(t: [(f32, &Self); 4]) -> Self {
        normalized(*t[0].1 * t[0].0 + *t[1].1 * t[1].0 + *t[2].1 * t[2].0 + *t[3].1 * t[3].0)
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        let other = if self.dot(*other) < 0.0 {
            -*other
        } else {
            *other
        };
        normalized(self.slerp(other, t))
    }
}

impl Animatable for Color {
    fn weighted_sum(t: [(f32, &Self); 4]) -> Self {
        let mut out = [0.0; 4];
        for (w, c) in t {
            for (o, v) in out.iter_mut().zip(c.to_array()) {
                *o += v * w;
            }
        }
        Color::rgba(out[0], out[1], out[2], out[3])
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        Color::lerp(*self, *other, t)
    }
}

impl Animatable for Transform {
    fn weighted_sum(t: [(f32, &Self); 4]) -> Self {
        Transform {
            translation: Vec3::weighted_sum(t.map(|(w, x)| (w, &x.translation))),
            rotation: Quat::weighted_sum(t.map(|(w, x)| (w, &x.rotation))),
            scale: Vec3::weighted_sum(t.map(|(w, x)| (w, &x.scale))),
        }
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        Transform {
            translation: self.translation.lerp(other.translation, t),
            rotation: Animatable::lerp(&self.rotation, &other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

impl Animatable for Transform2D {
    fn weighted_sum(t: [(f32, &Self); 4]) -> Self {
        Transform2D {
            position: Vec2::weighted_sum(t.map(|(w, x)| (w, &x.position))),
            rotation: f32::weighted_sum(t.map(|(w, x)| (w, &x.rotation))),
            scale: Vec2::weighted_sum(t.map(|(w, x)| (w, &x.scale))),
        }
    }

    fn lerp(&self, other: &Self, t: f32) -> Self {
        Transform2D::lerp(self, other, t)
    }
}
