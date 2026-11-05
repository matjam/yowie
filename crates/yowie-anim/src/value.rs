//! Sampled values and the targets that receive them.

use serde::{Deserialize, Serialize};
use yowie_core::{Color, Quat, Transform, Transform2D, Vec2, Vec3, Vec4};

use crate::animatable::Animatable;
use crate::curve::Curve;

/// The result of sampling one track.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A sprite frame index.
    Frame(usize),
    /// A scalar.
    Float(f32),
    /// A 2D vector.
    Vec2(Vec2),
    /// A 3D vector.
    Vec3(Vec3),
    /// A 4D vector.
    Vec4(Vec4),
    /// A rotation.
    Quat(Quat),
    /// A colour.
    Color(Color),
    /// A full 3D transform.
    Transform(Transform),
    /// A full 2D transform.
    Transform2D(Transform2D),
    /// A translation, for joints and nodes.
    Translation(Vec3),
    /// A rotation, for joints and nodes.
    Rotation(Quat),
    /// A scale, for joints and nodes.
    Scale(Vec3),
    /// Morph target weights, one per target.
    Weights(Vec<f32>),
}

impl Value {
    /// Blends `self` towards `other` by `t` (0 to 1). Values of different
    /// kinds, and frame indices, cannot be mixed: the result is `other` once
    /// `t` reaches one half, otherwise `self`.
    pub fn blend(&self, other: &Value, t: f32) -> Value {
        use Value::*;
        match (self, other) {
            (Float(a), Float(b)) => Float(Animatable::lerp(a, b, t)),
            (Vec2(a), Vec2(b)) => Vec2(Animatable::lerp(a, b, t)),
            (Vec3(a), Vec3(b)) => Vec3(Animatable::lerp(a, b, t)),
            (Vec4(a), Vec4(b)) => Vec4(Animatable::lerp(a, b, t)),
            (Quat(a), Quat(b)) => Quat(Animatable::lerp(a, b, t)),
            (Color(a), Color(b)) => Color(Animatable::lerp(a, b, t)),
            (Transform(a), Transform(b)) => Transform(Animatable::lerp(a, b, t)),
            (Transform2D(a), Transform2D(b)) => Transform2D(Animatable::lerp(a, b, t)),
            (Translation(a), Translation(b)) => Translation(Animatable::lerp(a, b, t)),
            (Rotation(a), Rotation(b)) => Rotation(Animatable::lerp(a, b, t)),
            (Scale(a), Scale(b)) => Scale(Animatable::lerp(a, b, t)),
            (Weights(a), Weights(b)) if a.len() == b.len() => Weights(
                a.iter()
                    .zip(b)
                    .map(|(x, y)| Animatable::lerp(x, y, t))
                    .collect(),
            ),
            _ => {
                if t >= 0.5 {
                    other.clone()
                } else {
                    self.clone()
                }
            }
        }
    }
}

/// A curve of one kind, driving one property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Channel {
    /// Sprite frame index; sampled as the floor of the curve value.
    Frame(Curve<f32>),
    /// A scalar.
    Float(Curve<f32>),
    /// A 2D vector.
    Vec2(Curve<Vec2>),
    /// A 3D vector.
    Vec3(Curve<Vec3>),
    /// A 4D vector.
    Vec4(Curve<Vec4>),
    /// A rotation.
    Quat(Curve<Quat>),
    /// A colour.
    Color(Curve<Color>),
    /// A full 3D transform.
    Transform(Curve<Transform>),
    /// A full 2D transform.
    Transform2D(Curve<Transform2D>),
    /// A translation, for joints and nodes.
    Translation(Curve<Vec3>),
    /// A rotation, for joints and nodes.
    Rotation(Curve<Quat>),
    /// A scale, for joints and nodes.
    Scale(Curve<Vec3>),
    /// Morph target weights: one curve per target.
    Weights(Vec<Curve<f32>>),
}

impl Channel {
    /// The time of the last key.
    pub fn duration(&self) -> f32 {
        use Channel::*;
        match self {
            Frame(c) | Float(c) => c.duration(),
            Vec2(c) => c.duration(),
            Vec3(c) | Translation(c) | Scale(c) => c.duration(),
            Vec4(c) => c.duration(),
            Quat(c) | Rotation(c) => c.duration(),
            Color(c) => c.duration(),
            Transform(c) => c.duration(),
            Transform2D(c) => c.duration(),
            Weights(cs) => cs.iter().map(Curve::duration).fold(0.0, f32::max),
        }
    }

    /// The value at `time`, or `None` when the channel has no keys.
    pub fn sample(&self, time: f32) -> Option<Value> {
        use Channel as C;
        Some(match self {
            C::Frame(c) => Value::Frame(c.sample(time)?.max(0.0).floor() as usize),
            C::Float(c) => Value::Float(c.sample(time)?),
            C::Vec2(c) => Value::Vec2(c.sample(time)?),
            C::Vec3(c) => Value::Vec3(c.sample(time)?),
            C::Vec4(c) => Value::Vec4(c.sample(time)?),
            C::Quat(c) => Value::Quat(c.sample(time)?),
            C::Color(c) => Value::Color(c.sample(time)?),
            C::Transform(c) => Value::Transform(c.sample(time)?),
            C::Transform2D(c) => Value::Transform2D(c.sample(time)?),
            C::Translation(c) => Value::Translation(c.sample(time)?),
            C::Rotation(c) => Value::Rotation(c.sample(time)?),
            C::Scale(c) => Value::Scale(c.sample(time)?),
            C::Weights(cs) => {
                if cs.is_empty() {
                    return None;
                }
                Value::Weights(cs.iter().map(|c| c.sample(time).unwrap_or(0.0)).collect())
            }
        })
    }
}

/// Something animation writes into: a component, a joint pose, a UI node.
///
/// Any `FnMut(&str, &Value)` closure is a target, so a setter closure works
/// wherever a `Target` is expected.
pub trait Target {
    /// Receives the sampled `value` of the track named `track`.
    fn set(&mut self, track: &str, value: &Value);
}

impl<F: FnMut(&str, &Value)> Target for F {
    fn set(&mut self, track: &str, value: &Value) {
        self(track, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_by_kind() {
        let f = Value::Float(0.0).blend(&Value::Float(4.0), 0.25);
        assert_eq!(f, Value::Float(1.0));
        let frame = Value::Frame(1).blend(&Value::Frame(5), 0.4);
        assert_eq!(frame, Value::Frame(1));
        let frame = Value::Frame(1).blend(&Value::Frame(5), 0.6);
        assert_eq!(frame, Value::Frame(5));
        let w = Value::Weights(vec![0.0, 1.0]).blend(&Value::Weights(vec![1.0, 1.0]), 0.5);
        assert_eq!(w, Value::Weights(vec![0.5, 1.0]));
    }

    #[test]
    fn frame_channel_floors_and_clamps() {
        let ch = Channel::Frame(Curve::linear([(0.0, 0.0), (1.0, 4.0)]));
        assert_eq!(ch.sample(0.6), Some(Value::Frame(2)));
        assert_eq!(ch.sample(-3.0), Some(Value::Frame(0)));
        assert_eq!(Channel::Float(Curve::default()).sample(0.0), None);
    }
}
