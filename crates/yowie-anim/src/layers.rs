//! Animation layers with per-joint masks.

use crate::error::{AnimError, Result};
use crate::player::Player;
use crate::pose::Pose;
use crate::skeleton::Skeleton;

/// A weight from 0 to 1 per joint, used to restrict a layer to part of a
/// skeleton.
#[derive(Clone, Debug, PartialEq)]
pub struct JointMask {
    weights: Vec<f32>,
}

impl JointMask {
    /// A mask that applies fully to `joints` joints.
    pub fn all(joints: usize) -> Self {
        Self {
            weights: vec![1.0; joints],
        }
    }

    /// A mask that applies to no joint.
    pub fn none(joints: usize) -> Self {
        Self {
            weights: vec![0.0; joints],
        }
    }

    /// A mask of weight 1 on the named joints and 0 elsewhere.
    pub fn from_names(skeleton: &Skeleton, names: &[&str]) -> Result<Self> {
        let mut mask = Self::none(skeleton.len());
        for name in names {
            let i = skeleton
                .index_of(name)
                .ok_or_else(|| AnimError::UnknownName((*name).to_string()))?;
            mask.weights[i] = 1.0;
        }
        Ok(mask)
    }

    /// A mask of weight 1 on the named joint and everything below it.
    pub fn subtree(skeleton: &Skeleton, name: &str) -> Result<Self> {
        let root = skeleton
            .index_of(name)
            .ok_or_else(|| AnimError::UnknownName(name.to_string()))?;
        let mut mask = Self::none(skeleton.len());
        for (i, joint) in skeleton.joints().iter().enumerate() {
            // Parents precede children, so the parent's flag is already set.
            let inside = i == root || joint.parent.is_some_and(|p| mask.weights[p] > 0.0);
            if inside {
                mask.weights[i] = 1.0;
            }
        }
        Ok(mask)
    }

    /// Sets the weight of one joint, clamped to 0 to 1.
    pub fn with_weight(mut self, joint: usize, weight: f32) -> Self {
        if let Some(w) = self.weights.get_mut(joint) {
            *w = weight.clamp(0.0, 1.0);
        }
        self
    }

    /// The weight of a joint; 0 past the end of the mask.
    pub fn weight(&self, joint: usize) -> f32 {
        self.weights.get(joint).copied().unwrap_or(0.0)
    }
}

/// How a layer combines with the pose below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerMode {
    /// Replace the masked joints' transforms by the layer's, scaled by
    /// weight.
    #[default]
    Override,
    /// Add the layer's difference from the rest pose, for breathing or
    /// recoil over any base clip.
    Additive,
}

/// A second player applied over part of a skeleton.
///
/// A layer whose player has no clip is skipped. To hold a layer's last
/// pose after it ends, give its clip the `Clamp` loop mode; a finished
/// `Once` clip stops contributing and the layer then shows the rest pose.
#[derive(Clone, Debug)]
pub struct Layer {
    /// Plays the layer's clip.
    pub player: Player,
    /// Joints the layer applies to.
    pub mask: JointMask,
    /// Overall strength, 0 to 1; change it over time to fade the layer.
    pub weight: f32,
    /// Override or additive.
    pub mode: LayerMode,
}

impl Layer {
    /// A full-weight override layer.
    pub fn new(player: Player, mask: JointMask) -> Self {
        Self {
            player,
            mask,
            weight: 1.0,
            mode: LayerMode::Override,
        }
    }
}

impl Skeleton {
    /// The base player's pose with `layers` applied in order.
    pub fn evaluate(&self, base: &Player, layers: &[Layer]) -> Pose {
        let mut pose = self.pose_from_player(base);
        let rest = self.rest_pose();
        for layer in layers {
            if layer.weight <= 0.0 || layer.player.clip().is_none() {
                continue;
            }
            let layer_pose = self.pose_from_player(&layer.player);
            match layer.mode {
                LayerMode::Override => pose.blend(&layer_pose, layer.weight, &layer.mask),
                LayerMode::Additive => pose.add(&layer_pose, &rest, layer.weight, &layer.mask),
            }
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::clip::Clip;
    use crate::curve::Curve;
    use crate::skeleton::tests::arm;
    use crate::value::Channel;
    use yowie_core::Vec3;

    fn shifted(joint: &str, x: f32) -> Arc<Clip> {
        Arc::new(Clip::new(joint).with_track(
            joint,
            Channel::Translation(Curve::linear([(0.0, Vec3::new(x, 1.0, 0.0))])),
        ))
    }

    #[test]
    fn subtree_and_names_masks() {
        let s = arm();
        let sub = JointMask::subtree(&s, "elbow").unwrap();
        assert_eq!(
            (sub.weight(0), sub.weight(1), sub.weight(2)),
            (0.0, 1.0, 1.0)
        );
        let named = JointMask::from_names(&s, &["hand"]).unwrap();
        assert_eq!((named.weight(1), named.weight(2)), (0.0, 1.0));
        assert!(JointMask::subtree(&s, "tail").is_err());
    }

    #[test]
    fn layer_overrides_only_masked_joints() {
        let s = arm();
        let mut base = Player::new();
        base.play(shifted("shoulder", 5.0));
        let mut over = Player::new();
        over.play(shifted("hand", 9.0));
        let mut layer = Layer::new(over, JointMask::from_names(&s, &["hand"]).unwrap());
        layer.weight = 0.5;
        let pose = s.evaluate(&base, &[layer]);
        assert_eq!(pose.locals[0].translation.x, 5.0);
        assert_eq!(pose.locals[2].translation.x, 4.5);
    }

    #[test]
    fn additive_layer_of_rest_pose_changes_nothing() {
        let s = arm();
        let mut base = Player::new();
        base.play(shifted("shoulder", 5.0));
        let mut rest_player = Player::new();
        rest_player.play(shifted("hand", 0.0));
        let mut layer = Layer::new(rest_player, JointMask::all(s.len()));
        layer.mode = LayerMode::Additive;
        let with = s.evaluate(&base, &[layer]);
        let without = s.evaluate(&base, &[]);
        for (a, b) in with.locals.iter().zip(&without.locals) {
            assert!((a.translation - b.translation).length() < 1e-6);
            assert!(a.rotation.dot(b.rotation).abs() > 0.99999);
            assert!((a.scale - b.scale).length() < 1e-6);
        }
    }
}
