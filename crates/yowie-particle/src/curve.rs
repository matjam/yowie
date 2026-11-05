//! Curves and ranges that describe values over a particle's life.

use serde::{Deserialize, Deserializer, Serialize};
use yowie_core::Color;
use yowie_rng::Rng;

/// A piecewise-linear curve over a particle's normalised age, 0 at birth to
/// 1 at death. Keys are `(t, value)` pairs in ascending `t`; before the
/// first key and after the last the curve holds the end values. An empty
/// curve is the constant 1, so an unset multiplier changes nothing.
///
/// In JSON a curve is a list of `[t, value]` pairs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve(pub Vec<(f32, f32)>);

impl Curve {
    /// A curve with the constant value `v`.
    pub fn constant(v: f32) -> Self {
        Curve(vec![(0.0, v)])
    }

    /// A curve running from `a` at birth to `b` at death.
    pub fn linear(a: f32, b: f32) -> Self {
        Curve(vec![(0.0, a), (1.0, b)])
    }

    /// A curve through the given `(t, value)` keys.
    pub fn keys(keys: &[(f32, f32)]) -> Self {
        Curve(keys.to_vec())
    }

    /// Whether the curve has no keys.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The value at `t`.
    pub fn at(&self, t: f32) -> f32 {
        let k = &self.0;
        match k.len() {
            0 => return 1.0,
            1 => return k[0].1,
            _ => {}
        }
        if t <= k[0].0 {
            return k[0].1;
        }
        for w in k.windows(2) {
            let ((t0, v0), (t1, v1)) = (w[0], w[1]);
            if t <= t1 {
                return if t1 == t0 {
                    v1
                } else {
                    v0 + (v1 - v0) * (t - t0) / (t1 - t0)
                };
            }
        }
        k[k.len() - 1].1
    }
}

/// A colour at a point of a particle's life.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorKey {
    /// Normalised age, 0 to 1.
    pub t: f32,
    /// The colour, sRGB with straight alpha.
    pub color: Color,
}

/// The colour a gradient of `keys` gives at `t`, or `None` for no keys.
pub fn gradient_at(keys: &[ColorKey], t: f32) -> Option<Color> {
    let first = keys.first()?;
    if t <= first.t {
        return Some(first.color);
    }
    for w in keys.windows(2) {
        if t <= w[1].t {
            return Some(if w[1].t == w[0].t {
                w[1].color
            } else {
                w[0].color
                    .lerp(w[1].color, (t - w[0].t) / (w[1].t - w[0].t))
            });
        }
    }
    keys.last().map(|k| k.color)
}

/// A random value between `min` and `max`. A range whose `max` is not
/// above `min` is the constant `min`.
///
/// In JSON a range is `{"min": 1, "max": 2}`, a pair `[1, 2]` or a bare
/// number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Range {
    /// The smallest value.
    pub min: f32,
    /// The largest value.
    pub max: f32,
}

impl Range {
    /// A range from `min` to `max`.
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// The constant `v`.
    pub const fn constant(v: f32) -> Self {
        Self { min: v, max: v }
    }

    /// A random value in the range.
    pub fn pick(&self, rng: &mut Rng) -> f32 {
        if self.max <= self.min {
            self.min
        } else {
            rng.between(self.min, self.max)
        }
    }
}

impl From<f32> for Range {
    fn from(v: f32) -> Self {
        Range::constant(v)
    }
}

impl<'de> Deserialize<'de> for Range {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Number(f32),
            Pair([f32; 2]),
            Object {
                #[serde(default)]
                min: f32,
                #[serde(default)]
                max: f32,
            },
        }
        Ok(match Form::deserialize(d)? {
            Form::Number(v) => Range::constant(v),
            Form::Pair([min, max]) => Range { min, max },
            Form::Object { min, max } => Range { min, max },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_interpolates_and_clamps() {
        let c = Curve::keys(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]);
        let cases = [
            (-1.0, 0.0),
            (0.0, 0.0),
            (0.25, 0.5),
            (0.5, 1.0),
            (0.75, 0.5),
            (2.0, 0.0),
        ];
        for (t, want) in cases {
            assert!((c.at(t) - want).abs() < 1e-6, "t={t}");
        }
        assert_eq!(Curve::default().at(0.3), 1.0);
        assert_eq!(Curve::constant(4.0).at(0.9), 4.0);
    }

    #[test]
    fn gradient_blends_between_keys() {
        let keys = [
            ColorKey {
                t: 0.0,
                color: Color::rgb(1.0, 0.0, 0.0),
            },
            ColorKey {
                t: 1.0,
                color: Color::rgb(0.0, 0.0, 1.0),
            },
        ];
        let mid = gradient_at(&keys, 0.5).map(|c| (c.r, c.b));
        assert_eq!(mid, Some((0.5, 0.5)));
        assert!(gradient_at(&[], 0.5).is_none());
    }

    #[test]
    fn range_accepts_three_json_forms() {
        let forms = [
            (r#"{"min": 1, "max": 3}"#, Range::new(1.0, 3.0)),
            ("[2, 5]", Range::new(2.0, 5.0)),
            ("7", Range::constant(7.0)),
        ];
        for (json, want) in forms {
            assert_eq!(
                serde_json::from_str::<Range>(json).ok(),
                Some(want),
                "{json}"
            );
        }
    }
}
