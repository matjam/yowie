//! Blend spaces and trees: clips mixed by named parameters.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use yowie_core::Vec2;

use crate::clip::Clip;
use crate::pose::Pose;
use crate::skeleton::Skeleton;

/// Parameter values by name, such as `"speed"`.
pub type Params = HashMap<String, f32>;

/// Clips available to a blend, by name.
pub type ClipSet = HashMap<String, Arc<Clip>>;

/// The share of the pose one clip contributes.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipWeight {
    /// Clip name.
    pub clip: String,
    /// Weight from 0 to 1; a full evaluation sums to one.
    pub weight: f32,
}

fn add(out: &mut Vec<ClipWeight>, clip: &str, weight: f32) {
    if weight <= 0.0 {
        return;
    }
    match out.iter_mut().find(|c| c.clip == clip) {
        Some(c) => c.weight += weight,
        None => out.push(ClipWeight {
            clip: clip.to_string(),
            weight,
        }),
    }
}

/// A clip placed at a value of one parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendPoint1D {
    /// Clip name.
    pub clip: String,
    /// Parameter value at which the clip plays alone.
    pub at: f32,
}

/// Clips along one parameter: idle at 0, walk at 1, run at 2. Between two
/// clips the pose mixes linearly; past the ends the nearest clip plays
/// alone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlendSpace1D {
    /// Name of the parameter the space reads.
    pub parameter: String,
    /// The placed clips, in any order.
    pub clips: Vec<BlendPoint1D>,
}

impl BlendSpace1D {
    /// Adds this space's weights, multiplied by `scale`, to `out`.
    pub fn weights(&self, params: &Params, scale: f32, out: &mut Vec<ClipWeight>) {
        let v = params.get(&self.parameter).copied().unwrap_or(0.0);
        let at: Vec<(f32, &str)> = {
            let mut p: Vec<_> = self.clips.iter().map(|c| (c.at, c.clip.as_str())).collect();
            p.sort_by(|a, b| a.0.total_cmp(&b.0));
            p
        };
        let (Some(first), Some(last)) = (at.first(), at.last()) else {
            return;
        };
        if v <= first.0 {
            return add(out, first.1, scale);
        }
        if v >= last.0 {
            return add(out, last.1, scale);
        }
        let hi = at.partition_point(|p| p.0 <= v);
        let (a, b) = (at[hi - 1], at[hi]);
        let t = (v - a.0) / (b.0 - a.0);
        add(out, a.1, scale * (1.0 - t));
        add(out, b.1, scale * t);
    }
}

/// A clip placed at a point of a 2D parameter plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendPoint2D {
    /// Clip name.
    pub clip: String,
    /// Point at which the clip plays alone.
    pub at: Vec2,
}

/// Clips in a plane of two parameters, such as a strafe set read from a
/// velocity.
///
/// Weights are gradient bands: a clip at the current point plays alone, a
/// point on the line between two clips mixes them linearly and clips drop
/// out as the point moves past them. Weights are normalised to sum to one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlendSpace2D {
    /// Name of the horizontal parameter.
    pub x: String,
    /// Name of the vertical parameter.
    pub y: String,
    /// The placed clips, in any order.
    pub clips: Vec<BlendPoint2D>,
}

impl BlendSpace2D {
    /// Adds this space's weights, multiplied by `scale`, to `out`.
    pub fn weights(&self, params: &Params, scale: f32, out: &mut Vec<ClipWeight>) {
        let get = |n: &String| params.get(n).copied().unwrap_or(0.0);
        let p = Vec2::new(get(&self.x), get(&self.y));
        let raw: Vec<f32> = self
            .clips
            .iter()
            .enumerate()
            .map(|(i, ci)| {
                self.clips
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .filter_map(|(_, cj)| {
                        let d = cj.at - ci.at;
                        let l2 = d.length_squared();
                        (l2 > 0.0).then(|| (1.0 - (p - ci.at).dot(d) / l2).clamp(0.0, 1.0))
                    })
                    .fold(1.0, f32::min)
            })
            .collect();
        let total: f32 = raw.iter().sum();
        if total > 0.0 {
            for (c, w) in self.clips.iter().zip(raw) {
                add(out, &c.clip, scale * w / total);
            }
        }
    }
}

/// A subtree placed at a parameter value inside a [`BlendNode::Mix`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendChild {
    /// Parameter value at which this child plays alone.
    pub at: f32,
    /// The child.
    pub node: BlendNode,
}

/// A node of a blend tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlendNode {
    /// One clip.
    Clip(String),
    /// A 1D blend space.
    Space1D(BlendSpace1D),
    /// A 2D blend space.
    Space2D(BlendSpace2D),
    /// Children placed along a parameter and mixed like a 1D space, such as
    /// a crouch amount fading standing locomotion into crouched.
    Mix {
        /// Name of the parameter the children are placed along.
        parameter: String,
        /// The children, in any order.
        children: Vec<BlendChild>,
    },
}

impl BlendNode {
    /// Adds this node's weights, multiplied by `scale`, to `out`.
    pub fn weights(&self, params: &Params, scale: f32, out: &mut Vec<ClipWeight>) {
        match self {
            BlendNode::Clip(c) => add(out, c, scale),
            BlendNode::Space1D(s) => s.weights(params, scale, out),
            BlendNode::Space2D(s) => s.weights(params, scale, out),
            BlendNode::Mix { parameter, children } => {
                // Reuse the 1D placement by mixing child indices.
                let proxy = BlendSpace1D {
                    parameter: parameter.clone(),
                    clips: children
                        .iter()
                        .enumerate()
                        .map(|(i, c)| BlendPoint1D {
                            clip: i.to_string(),
                            at: c.at,
                        })
                        .collect(),
                };
                let mut shares = Vec::new();
                proxy.weights(params, scale, &mut shares);
                for s in shares {
                    if let Some(child) = s.clip.parse::<usize>().ok().and_then(|i| children.get(i)) {
                        child.node.weights(params, s.weight, out);
                    }
                }
            }
        }
    }
}

/// A running blend: a tree, its parameters and a shared phase.
///
/// All mixed clips play at one phase of their own length, advancing at the
/// weighted rate of the blended cycle, so a walk's and a run's feet land
/// together.
#[derive(Clone, Debug)]
pub struct Blend {
    /// The tree evaluated every update.
    pub root: BlendNode,
    params: Params,
    phase: f64,
}

impl Blend {
    /// A blend of `root` with no parameters set (all read as 0).
    pub fn new(root: BlendNode) -> Self {
        Self {
            root,
            params: Params::new(),
            phase: 0.0,
        }
    }

    /// Sets a parameter.
    pub fn set(&mut self, name: &str, value: f32) {
        self.params.insert(name.to_string(), value);
    }

    /// Reads a parameter; 0 when unset.
    pub fn get(&self, name: &str) -> f32 {
        self.params.get(name).copied().unwrap_or(0.0)
    }

    /// The shared phase, 0 to 1.
    pub fn phase(&self) -> f64 {
        self.phase
    }

    /// Sets the shared phase; values wrap into 0 to 1.
    pub fn set_phase(&mut self, phase: f64) {
        self.phase = phase.rem_euclid(1.0);
    }

    /// The clips and weights for the current parameters, summing to one
    /// unless the tree has no clips.
    pub fn weights(&self) -> Vec<ClipWeight> {
        let mut out = Vec::new();
        self.root.weights(&self.params, 1.0, &mut out);
        let total: f32 = out.iter().map(|c| c.weight).sum();
        if total > 0.0 {
            out.iter_mut().for_each(|c| c.weight /= total);
        }
        out
    }

    /// Advances the phase by `dt` seconds.
    pub fn advance(&mut self, dt: f32, clips: &ClipSet) {
        let rate: f32 = self
            .weights()
            .iter()
            .filter_map(|w| clips.get(&w.clip).map(|c| (w.weight, c.duration())))
            .filter(|(_, d)| *d > 0.0)
            .map(|(w, d)| w / d)
            .sum();
        self.set_phase(self.phase + f64::from(dt) * f64::from(rate));
    }

    /// The mixed pose at the current phase; `None` when no weighted clip is
    /// in `clips`.
    pub fn pose(&self, skeleton: &Skeleton, clips: &ClipSet) -> Option<Pose> {
        let poses: Vec<(Pose, f32)> = self
            .weights()
            .into_iter()
            .filter_map(|w| {
                let clip = clips.get(&w.clip)?;
                let t = self.phase as f32 * clip.duration();
                Some((skeleton.pose_from_clip(clip, t), w.weight))
            })
            .collect();
        let refs: Vec<(&Pose, f32)> = poses.iter().map(|(p, w)| (p, *w)).collect();
        Pose::weighted(&refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weight_of(ws: &[ClipWeight], clip: &str) -> f32 {
        ws.iter().find(|c| c.clip == clip).map_or(0.0, |c| c.weight)
    }

    fn space_2d() -> BlendSpace2D {
        let pt = |clip: &str, x, y| BlendPoint2D {
            clip: clip.into(),
            at: Vec2::new(x, y),
        };
        BlendSpace2D {
            x: "x".into(),
            y: "y".into(),
            clips: vec![pt("a", 0.0, 0.0), pt("b", 1.0, 0.0), pt("c", 0.0, 1.0), pt("d", 1.0, 1.0)],
        }
    }

    fn eval2(s: &BlendSpace2D, x: f32, y: f32) -> Vec<ClipWeight> {
        let params = Params::from([("x".to_string(), x), ("y".to_string(), y)]);
        let mut out = Vec::new();
        s.weights(&params, 1.0, &mut out);
        out
    }

    #[test]
    fn one_d_weights() {
        let s = BlendSpace1D {
            parameter: "speed".into(),
            clips: ["idle", "walk", "run"]
                .iter()
                .enumerate()
                .map(|(i, c)| BlendPoint1D { clip: (*c).into(), at: i as f32 })
                .collect(),
        };
        // (speed, idle, walk, run)
        let cases = [(-1.0, 1.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0), (0.5, 0.5, 0.5, 0.0), (1.5, 0.0, 0.5, 0.5), (9.0, 0.0, 0.0, 1.0)];
        for (v, i, w, r) in cases {
            let mut out = Vec::new();
            s.weights(&Params::from([("speed".to_string(), v)]), 1.0, &mut out);
            assert!((weight_of(&out, "idle") - i).abs() < 1e-6, "idle at {v}");
            assert!((weight_of(&out, "walk") - w).abs() < 1e-6, "walk at {v}");
            assert!((weight_of(&out, "run") - r).abs() < 1e-6, "run at {v}");
        }
    }

    #[test]
    fn two_d_corners_edges_and_centre() {
        let s = space_2d();
        // (x, y, a, b, c, d)
        let cases = [
            (0.0, 0.0, 1.0, 0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 0.0, 1.0, 0.0),
            (1.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            (0.5, 0.5, 0.25, 0.25, 0.25, 0.25),
            (0.5, 0.0, 0.5, 0.5, 0.0, 0.0),
        ];
        for (x, y, a, b, c, d) in cases {
            let out = eval2(&s, x, y);
            for (name, want) in [("a", a), ("b", b), ("c", c), ("d", d)] {
                let got = weight_of(&out, name);
                assert!((got - want).abs() < 1e-5, "({x},{y}) {name}: {got} != {want}");
            }
            assert!((out.iter().map(|c| c.weight).sum::<f32>() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn tree_mixes_children_and_round_trips_serde() {
        let walk = BlendNode::Clip("walk".into());
        let crouch = BlendNode::Clip("crouch".into());
        let tree = BlendNode::Mix {
            parameter: "crouch".into(),
            children: vec![
                BlendChild { at: 0.0, node: walk },
                BlendChild { at: 1.0, node: crouch },
            ],
        };
        let mut blend = Blend::new(tree.clone());
        blend.set("crouch", 0.25);
        let w = blend.weights();
        assert!((weight_of(&w, "walk") - 0.75).abs() < 1e-6);
        assert!((weight_of(&w, "crouch") - 0.25).abs() < 1e-6);
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(serde_json::from_str::<BlendNode>(&json).unwrap(), tree);
    }

    #[test]
    fn phase_advances_at_the_blended_rate() {
        use crate::curve::Curve;
        use crate::value::Channel;
        let clip = |name: &str, len: f32| {
            Arc::new(Clip::new(name).with_track("j", Channel::Float(Curve::linear([(0.0, 0.0), (len, 1.0)]))))
        };
        let clips: ClipSet = [("slow", clip("slow", 2.0)), ("fast", clip("fast", 1.0))]
            .into_iter()
            .map(|(n, c)| (n.to_string(), c))
            .collect();
        let tree = BlendNode::Space1D(BlendSpace1D {
            parameter: "s".into(),
            clips: vec![
                BlendPoint1D { clip: "slow".into(), at: 0.0 },
                BlendPoint1D { clip: "fast".into(), at: 1.0 },
            ],
        });
        let mut blend = Blend::new(tree);
        blend.set("s", 0.5);
        blend.advance(0.4, &clips);
        // Rate is 0.5 / 2 + 0.5 / 1 = 0.75 cycles per second.
        assert!((blend.phase() - 0.3).abs() < 1e-6);
    }
}
