//! Keyframe curves.

use serde::{Deserialize, Serialize};

use crate::animatable::Animatable;

/// How a curve moves from one key to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interp {
    /// Hold the earlier key's value until the next key.
    Step,
    /// Interpolate linearly; rotations take the shortest arc.
    #[default]
    Linear,
    /// Hermite spline using the keys' [`Tangents`], as glTF cubic spline
    /// interpolation. A key without tangents contributes a zero tangent.
    Cubic,
}

/// Spline tangents of one key, in value units per second.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tangents<T> {
    /// Tangent arriving at the key from the previous segment.
    pub incoming: T,
    /// Tangent leaving the key into the next segment.
    pub outgoing: T,
}

/// One keyframe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct Key<T> {
    /// Time in seconds.
    pub time: f32,
    /// Value at `time`.
    pub value: T,
    /// How the curve moves from this key to the next.
    #[serde(default)]
    pub interp: Interp,
    /// Spline tangents, used when `interp` is [`Interp::Cubic`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangents: Option<Tangents<T>>,
}

impl<T> Key<T> {
    /// A linear key.
    pub fn new(time: f32, value: T) -> Self {
        Self {
            time,
            value,
            interp: Interp::Linear,
            tangents: None,
        }
    }

    /// A key that holds its value until the next key.
    pub fn step(time: f32, value: T) -> Self {
        Self {
            interp: Interp::Step,
            ..Self::new(time, value)
        }
    }

    /// A cubic spline key with its tangents.
    pub fn cubic(time: f32, incoming: T, value: T, outgoing: T) -> Self {
        Self {
            time,
            value,
            interp: Interp::Cubic,
            tangents: Some(Tangents { incoming, outgoing }),
        }
    }
}

/// Keys sorted by time. Sampling outside the key range holds the first or
/// last value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Vec<Key<T>>", into = "Vec<Key<T>>")]
pub struct Curve<T: Animatable> {
    keys: Vec<Key<T>>,
}

impl<T: Animatable> From<Vec<Key<T>>> for Curve<T> {
    fn from(keys: Vec<Key<T>>) -> Self {
        Curve::new(keys)
    }
}

impl<T: Animatable> From<Curve<T>> for Vec<Key<T>> {
    fn from(curve: Curve<T>) -> Self {
        curve.keys
    }
}

impl<T: Animatable> Default for Curve<T> {
    fn default() -> Self {
        Self { keys: Vec::new() }
    }
}

impl<T: Animatable> Curve<T> {
    /// Builds a curve, sorting the keys by time. Keys with equal times keep
    /// their given order.
    pub fn new(mut keys: Vec<Key<T>>) -> Self {
        keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        Self { keys }
    }

    /// A curve of linear keys from `(time, value)` pairs.
    pub fn linear(keys: impl IntoIterator<Item = (f32, T)>) -> Self {
        Self::new(keys.into_iter().map(|(t, v)| Key::new(t, v)).collect())
    }

    /// A curve of step keys from `(time, value)` pairs.
    pub fn step(keys: impl IntoIterator<Item = (f32, T)>) -> Self {
        Self::new(keys.into_iter().map(|(t, v)| Key::step(t, v)).collect())
    }

    /// The keys in time order.
    pub fn keys(&self) -> &[Key<T>] {
        &self.keys
    }

    /// True when the curve has no keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The time of the last key, or 0 for an empty curve.
    pub fn duration(&self) -> f32 {
        self.keys.last().map_or(0.0, |k| k.time)
    }

    /// The value at `time` seconds, or `None` for an empty curve.
    pub fn sample(&self, time: f32) -> Option<T> {
        let first = self.keys.first()?;
        let last = self.keys.last()?;
        if time.is_nan() || time <= first.time {
            return Some(first.value.clone());
        }
        if time >= last.time {
            return Some(last.value.clone());
        }
        let i = self
            .keys
            .partition_point(|k| k.time <= time)
            .saturating_sub(1);
        let (a, b) = (&self.keys[i], self.keys.get(i + 1)?);
        let dt = b.time - a.time;
        if dt <= 0.0 {
            return Some(a.value.clone());
        }
        let u = (time - a.time) / dt;
        Some(match a.interp {
            Interp::Step => a.value.clone(),
            Interp::Linear => a.value.lerp(&b.value, u),
            Interp::Cubic => hermite(a, b, dt, u),
        })
    }
}

/// Evaluates the cubic Hermite segment from `a` to `b`.
fn hermite<T: Animatable>(a: &Key<T>, b: &Key<T>, dt: f32, u: f32) -> T {
    let (u2, u3) = (u * u, u * u * u);
    let p0 = 2.0 * u3 - 3.0 * u2 + 1.0;
    let m0 = (u3 - 2.0 * u2 + u) * dt;
    let p1 = -2.0 * u3 + 3.0 * u2;
    let m1 = (u3 - u2) * dt;
    let out = a
        .tangents
        .as_ref()
        .map_or((0.0, &a.value), |t| (m0, &t.outgoing));
    let inc = b
        .tangents
        .as_ref()
        .map_or((0.0, &b.value), |t| (m1, &t.incoming));
    T::weighted_sum([(p0, &a.value), out, (p1, &b.value), inc])
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::{Color, Quat, Transform, Vec2, Vec3, Vec4};

    #[test]
    fn scalar_interpolation() {
        let linear = Curve::linear([(0.0, 0.0f32), (1.0, 10.0), (3.0, 20.0)]);
        let step = Curve::step([(0.0, 0.0f32), (1.0, 10.0)]);
        // (curve, time, expected)
        let cases = [
            (&linear, -1.0, 0.0),
            (&linear, 0.0, 0.0),
            (&linear, 0.5, 5.0),
            (&linear, 1.0, 10.0),
            (&linear, 2.0, 15.0),
            (&linear, 3.0, 20.0),
            (&linear, 9.0, 20.0),
            (&step, 0.99, 0.0),
            (&step, 1.0, 10.0),
        ];
        for (curve, t, want) in cases {
            let got = curve.sample(t).unwrap();
            assert!((got - want).abs() < 1e-5, "t={t}: {got} != {want}");
        }
        assert_eq!(Curve::<f32>::default().sample(1.0), None);
    }

    #[test]
    fn vector_and_color_types() {
        let v2 = Curve::linear([(0.0, Vec2::ZERO), (2.0, Vec2::new(2.0, 4.0))]);
        assert_eq!(v2.sample(1.0), Some(Vec2::new(1.0, 2.0)));
        let v3 = Curve::linear([(0.0, Vec3::ZERO), (1.0, Vec3::ONE)]);
        assert_eq!(v3.sample(0.5), Some(Vec3::splat(0.5)));
        let v4 = Curve::linear([(0.0, Vec4::ZERO), (1.0, Vec4::ONE)]);
        assert_eq!(v4.sample(0.25), Some(Vec4::splat(0.25)));
        let c = Curve::linear([(0.0, Color::BLACK), (1.0, Color::WHITE)]);
        assert!((c.sample(0.5).unwrap().r - 0.5).abs() < 1e-6);
        let t = Curve::linear([
            (0.0, Transform::IDENTITY),
            (1.0, Transform::from_xyz(2.0, 0.0, 0.0)),
        ]);
        assert_eq!(t.sample(0.5).unwrap().translation, Vec3::X);
    }

    #[test]
    fn quaternion_takes_shortest_path() {
        let a = Quat::from_rotation_z(0.1);
        // The same rotation as 0.1 + 0.4 radians, written with the opposite sign.
        let b = -Quat::from_rotation_z(0.5);
        let curve = Curve::linear([(0.0, a), (1.0, b)]);
        let mid = curve.sample(0.5).unwrap();
        let want = Quat::from_rotation_z(0.3);
        assert!(mid.dot(want).abs() > 0.99999, "{mid:?}");
        assert!((mid.length() - 1.0).abs() < 1e-5);
        // Far-apart rotations still interpolate along the short arc.
        let c = Curve::linear([
            (0.0, Quat::from_rotation_y(0.0)),
            (1.0, Quat::from_rotation_y(3.0)),
        ]);
        let q = c.sample(0.5).unwrap();
        assert!(q.dot(Quat::from_rotation_y(1.5)).abs() > 0.99999);
    }

    #[test]
    fn cubic_matches_hermite_at_keys_and_midpoint() {
        // Zero tangents on a 0 -> 1 segment give smoothstep.
        let curve = Curve::new(vec![
            Key::cubic(0.0, 0.0f32, 0.0, 0.0),
            Key::cubic(2.0, 0.0, 1.0, 0.0),
        ]);
        assert_eq!(curve.sample(0.0), Some(0.0));
        assert_eq!(curve.sample(2.0), Some(1.0));
        assert!((curve.sample(1.0).unwrap() - 0.5).abs() < 1e-6);
        assert!((curve.sample(0.5).unwrap() - 0.15625).abs() < 1e-6);
        // A slope of 1 per second out of key 0 and into key 1 over 1 s is a straight line.
        let line = Curve::new(vec![
            Key::cubic(0.0, 1.0f32, 0.0, 1.0),
            Key::cubic(1.0, 1.0, 1.0, 1.0),
        ]);
        assert!((line.sample(0.3).unwrap() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn new_sorts_keys_and_serde_round_trips() {
        let curve = Curve::linear([(2.0, 2.0f32), (0.0, 0.0)]);
        assert_eq!(curve.keys()[0].time, 0.0);
        assert_eq!(curve.duration(), 2.0);
        let json = serde_json::to_string(&curve).unwrap();
        let back: Curve<f32> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, curve);
        let from_data: Curve<f32> =
            serde_json::from_str(r#"[{"time":1,"value":3},{"time":0,"value":1}]"#).unwrap();
        assert_eq!(from_data.sample(0.5), Some(2.0));
    }
}
