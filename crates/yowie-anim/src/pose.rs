//! Poses: one local transform per joint, with blending operations.

use std::collections::BTreeMap;

use yowie_core::{Quat, Transform, Vec3};

use crate::animatable::Animatable;
use crate::layers::JointMask;

/// The local transform of every joint of a skeleton, plus morph weights
/// keyed by node name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    /// Local transforms, indexed like the skeleton's joints.
    pub locals: Vec<Transform>,
    /// Morph target weights by the name of the node that owns the mesh.
    pub morph: BTreeMap<String, Vec<f32>>,
}

impl Pose {
    /// The weighted average of poses. Weights are normalised to sum to one;
    /// rotations are averaged along the shortest arc. Returns `None` for an
    /// empty list or a list with no positive weight. All poses must have the
    /// same joint count; extra joints in later poses are ignored.
    pub fn weighted(poses: &[(&Pose, f32)]) -> Option<Pose> {
        let total: f32 = poses.iter().map(|(_, w)| w.max(0.0)).sum();
        let first = poses.first()?.0;
        if total <= 0.0 {
            return None;
        }
        let mut out = first.clone();
        for (j, local) in out.locals.iter_mut().enumerate() {
            let mut t = Vec3::ZERO;
            let mut s = Vec3::ZERO;
            let mut r = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
            let reference = first.locals[j].rotation;
            for (p, w) in poses {
                let (w, Some(l)) = (w.max(0.0) / total, p.locals.get(j)) else {
                    continue;
                };
                t += l.translation * w;
                s += l.scale * w;
                let q = if reference.dot(l.rotation) < 0.0 {
                    -l.rotation
                } else {
                    l.rotation
                };
                r += q * w;
            }
            let rotation = if r.length_squared() > 1e-12 {
                r.normalize()
            } else {
                Quat::IDENTITY
            };
            *local = Transform {
                translation: t,
                rotation,
                scale: s,
            };
        }
        let mut sums: BTreeMap<&str, (Vec<f32>, f32)> = BTreeMap::new();
        for (p, w) in poses {
            let w = w.max(0.0);
            for (name, ws) in &p.morph {
                let e = sums
                    .entry(name)
                    .or_insert_with(|| (vec![0.0; ws.len()], 0.0));
                e.1 += w;
                for (a, b) in e.0.iter_mut().zip(ws) {
                    *a += b * w;
                }
            }
        }
        out.morph = sums
            .into_iter()
            .filter(|(_, (_, w))| *w > 0.0)
            .map(|(n, (v, w))| (n.to_string(), v.into_iter().map(|x| x / w).collect()))
            .collect();
        Some(out)
    }

    /// Replaces this pose with `other` by `weight`, scaled per joint by
    /// `mask`: 0 leaves the joint alone, 1 takes `other`'s transform.
    pub fn blend(&mut self, other: &Pose, weight: f32, mask: &JointMask) {
        for (j, local) in self.locals.iter_mut().enumerate() {
            if let Some(o) = other.locals.get(j) {
                *local = Animatable::lerp(local, o, (weight * mask.weight(j)).clamp(0.0, 1.0));
            }
        }
        let w = weight.clamp(0.0, 1.0);
        for (name, ws) in &other.morph {
            let own = self.morph.entry(name.clone()).or_insert_with(|| ws.clone());
            for (a, b) in own.iter_mut().zip(ws) {
                *a += (b - *a) * w;
            }
        }
    }

    /// Adds `layer`'s difference from `rest` on top of this pose, scaled by
    /// `weight` and the per-joint `mask`. A layer equal to the rest pose
    /// changes nothing.
    pub fn add(&mut self, layer: &Pose, rest: &Pose, weight: f32, mask: &JointMask) {
        for (j, local) in self.locals.iter_mut().enumerate() {
            let (Some(l), Some(r)) = (layer.locals.get(j), rest.locals.get(j)) else {
                continue;
            };
            let f = weight * mask.weight(j);
            let delta = r.rotation.inverse() * l.rotation;
            local.translation += (l.translation - r.translation) * f;
            local.rotation =
                (local.rotation * Animatable::lerp(&Quat::IDENTITY, &delta, f)).normalize();
            local.scale *= Vec3::ONE.lerp(l.scale / r.scale, f);
        }
        for (name, ws) in &layer.morph {
            let own = self
                .morph
                .entry(name.clone())
                .or_insert_with(|| vec![0.0; ws.len()]);
            for (a, b) in own.iter_mut().zip(ws) {
                *a += b * weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(x: f32, angle: f32) -> Pose {
        Pose {
            locals: vec![
                Transform::from_xyz(x, 0.0, 0.0).with_rotation(Quat::from_rotation_z(angle)),
            ],
            morph: BTreeMap::new(),
        }
    }

    #[test]
    fn weighted_normalises_and_uses_short_arc() {
        let a = pose(0.0, 0.0);
        let b = pose(4.0, 1.0);
        let m = Pose::weighted(&[(&a, 1.0), (&b, 1.0)]).unwrap();
        assert!((m.locals[0].translation.x - 2.0).abs() < 1e-5);
        assert!(m.locals[0].rotation.dot(Quat::from_rotation_z(0.5)).abs() > 0.9999);
        // Unnormalised weights give the same answer.
        let m = Pose::weighted(&[(&a, 3.0), (&b, 1.0)]).unwrap();
        assert!((m.locals[0].translation.x - 1.0).abs() < 1e-5);
        assert!(Pose::weighted(&[(&a, 0.0)]).is_none());
        assert!(Pose::weighted(&[]).is_none());
    }

    #[test]
    fn additive_layer_equal_to_rest_is_identity() {
        let rest = pose(1.0, 0.3);
        let mut base = pose(5.0, -0.8);
        let before = base.clone();
        base.add(&rest, &rest, 1.0, &JointMask::all(1));
        assert!((base.locals[0].translation - before.locals[0].translation).length() < 1e-6);
        assert!(base.locals[0].rotation.dot(before.locals[0].rotation).abs() > 0.99999);
        assert!((base.locals[0].scale - before.locals[0].scale).length() < 1e-6);
    }

    #[test]
    fn additive_layer_adds_difference() {
        let rest = pose(1.0, 0.0);
        let layer = pose(3.0, 0.5);
        let mut base = pose(10.0, 0.25);
        base.add(&layer, &rest, 0.5, &JointMask::all(1));
        assert!((base.locals[0].translation.x - 11.0).abs() < 1e-5);
        let want = Quat::from_rotation_z(0.25 + 0.25);
        assert!(base.locals[0].rotation.dot(want).abs() > 0.99999);
    }

    #[test]
    fn override_respects_mask() {
        let mut base = pose(0.0, 0.0);
        let other = pose(10.0, 0.0);
        base.blend(&other, 1.0, &JointMask::none(1));
        assert_eq!(base.locals[0].translation.x, 0.0);
        base.blend(&other, 0.5, &JointMask::all(1));
        assert_eq!(base.locals[0].translation.x, 5.0);
    }
}
