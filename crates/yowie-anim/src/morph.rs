//! Morph target (blend shape) weights.

use crate::curve::{Curve, Interp, Key};
use crate::value::{Channel, Value};

/// The weights of a mesh's morph targets, addressed by index or name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MorphWeights {
    names: Vec<String>,
    weights: Vec<f32>,
}

impl MorphWeights {
    /// Weights for targets called `names`, all zero.
    pub fn new(names: Vec<String>) -> Self {
        let weights = vec![0.0; names.len()];
        Self { names, weights }
    }

    /// Number of targets.
    pub fn len(&self) -> usize {
        self.weights.len()
    }

    /// True when there are no targets.
    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }

    /// The weights, one per target.
    pub fn weights(&self) -> &[f32] {
        &self.weights
    }

    /// The weight of the target called `name`.
    pub fn get(&self, name: &str) -> Option<f32> {
        self.names.iter().position(|n| n == name).map(|i| self.weights[i])
    }

    /// Sets the weight of the target called `name`. Returns false when no
    /// target has that name.
    pub fn set(&mut self, name: &str, weight: f32) -> bool {
        match self.names.iter().position(|n| n == name) {
            Some(i) => {
                self.weights[i] = weight;
                true
            }
            None => false,
        }
    }

    /// Copies animated weights in. Extra values are ignored and missing
    /// ones leave the current weight.
    pub fn set_all(&mut self, values: &[f32]) {
        for (w, v) in self.weights.iter_mut().zip(values) {
            *w = *v;
        }
    }

    /// Takes the weights from a sampled [`Value::Weights`]; other values
    /// are ignored.
    pub fn apply(&mut self, value: &Value) {
        if let Value::Weights(w) = value {
            self.set_all(w);
        }
    }

    /// Moves every weight towards `other` by `t` (0 to 1), for expression
    /// changes chosen by the game.
    pub fn blend(&mut self, other: &MorphWeights, t: f32) {
        for (w, o) in self.weights.iter_mut().zip(&other.weights) {
            *w += (o - *w) * t;
        }
    }
}

/// Builds a morph weight channel from keys: `times[i]` has `values[i]`
/// weights, one per target. All value lists should have equal length;
/// shorter ones read as zero for the missing targets.
pub fn weights_channel(times: &[f32], values: &[Vec<f32>], interp: Interp) -> Channel {
    let targets = values.iter().map(Vec::len).max().unwrap_or(0);
    let curves = (0..targets)
        .map(|k| {
            let keys = times.iter().zip(values).map(|(t, v)| Key {
                time: *t,
                value: v.get(k).copied().unwrap_or(0.0),
                interp,
                tangents: None,
            });
            Curve::new(keys.collect())
        })
        .collect();
    Channel::Weights(curves)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_access_and_blend() {
        let mut m = MorphWeights::new(vec!["smile".into(), "blink".into()]);
        assert!(m.set("blink", 1.0));
        assert!(!m.set("frown", 1.0));
        assert_eq!(m.get("blink"), Some(1.0));
        let mut other = MorphWeights::new(vec!["smile".into(), "blink".into()]);
        other.set("smile", 1.0);
        m.blend(&other, 0.5);
        assert_eq!(m.weights(), [0.5, 0.5]);
    }

    #[test]
    fn weights_track_samples_per_target() {
        let ch = weights_channel(&[0.0, 1.0], &[vec![0.0, 1.0], vec![1.0, 1.0]], Interp::Linear);
        let mut m = MorphWeights::new(vec!["a".into(), "b".into()]);
        m.apply(&ch.sample(0.5).unwrap());
        assert_eq!(m.weights(), [0.5, 1.0]);
    }
}
