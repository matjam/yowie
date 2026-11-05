//! One posed copy of a model: an animation player, the node transforms it
//! produces, joint matrices and morph weights, and material overrides.

use yowie_anim::{Player, Value};
use yowie_core::{Mat4, Transform};

use super::load::world_matrices;
use super::{Model, ModelPart};
use crate::scene::material::Material;

/// A model with its own pose, for drawing with
/// [`Scene::draw_model`](crate::scene::Scene::draw_model).
///
/// `player` is a `yowie_anim::Player` playing the model's clips; call
/// [`advance`](Self::advance) each update to run it and recompute the
/// pose. Without a clip the instance shows the rest pose.
#[derive(Debug)]
pub struct ModelInstance {
    model: Model,
    /// The animation player; its tracks name nodes.
    pub player: Player,
    locals: Vec<Transform>,
    pub(crate) world: Vec<Mat4>,
    /// Joint matrices per skin, in the skin's joint order.
    pub(crate) joints: Vec<Vec<Mat4>>,
    /// Morph weights per node.
    pub(crate) weights: Vec<Vec<f32>>,
    pub(crate) overrides: Vec<Option<Material>>,
}

impl ModelInstance {
    /// An instance of `model` in its rest pose.
    pub fn new(model: &Model) -> Self {
        let data = &model.inner;
        let mut inst = Self {
            model: model.clone(),
            player: Player::new(),
            locals: data.nodes.iter().map(|n| n.rest).collect(),
            world: Vec::new(),
            joints: vec![Vec::new(); data.skins.len()],
            weights: data.nodes.iter().map(|n| n.weights.clone()).collect(),
            overrides: vec![None; data.parts.len()],
        };
        inst.update_pose();
        inst
    }

    /// The model this instance poses.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Starts the clip called `name` from its beginning. Returns false
    /// when the model has no such clip.
    pub fn play(&mut self, name: &str) -> bool {
        match self.model.clip(name) {
            Some(c) => {
                self.player.play(c.clone());
                true
            }
            None => false,
        }
    }

    /// Fades from the current clip to the one called `name` over
    /// `seconds`. Returns false when the model has no such clip.
    pub fn crossfade(&mut self, name: &str, seconds: f32) -> bool {
        match self.model.clip(name) {
            Some(c) => {
                self.player.crossfade(c.clone(), seconds);
                true
            }
            None => false,
        }
    }

    /// Runs the player `dt` seconds and recomputes the pose.
    pub fn advance(&mut self, dt: f32) {
        self.player.advance(dt);
        self.update_pose();
    }

    /// Recomputes node matrices, joint matrices and morph weights from the
    /// player's current values over the rest pose. [`advance`](Self::advance)
    /// calls it; call it after changing the player or a node directly.
    pub fn update_pose(&mut self) {
        let data = &self.model.inner;
        for (l, n) in self.locals.iter_mut().zip(&data.nodes) {
            *l = n.rest;
        }
        for (w, n) in self.weights.iter_mut().zip(&data.nodes) {
            w.clone_from(&n.weights);
        }
        let (locals, weights) = (&mut self.locals, &mut self.weights);
        self.player.evaluate(|name, value| {
            let Some(&i) = data.node_index.get(name) else {
                return;
            };
            let local = &mut locals[i];
            match value {
                Value::Translation(v) | Value::Vec3(v) => local.translation = v,
                Value::Rotation(q) | Value::Quat(q) => local.rotation = q.normalize(),
                Value::Scale(s) => local.scale = s,
                Value::Transform(t) => *local = t,
                Value::Weights(w) => weights[i] = w,
                _ => {}
            }
        });
        world_matrices(data, &self.locals, &mut self.world);
        for (skin, out) in data.skins.iter().zip(&mut self.joints) {
            out.clear();
            out.extend(
                skin.joints
                    .iter()
                    .map(|(node, ibm)| self.world[*node] * *ibm),
            );
        }
    }

    /// Sets a node's local transform until the next
    /// [`update_pose`](Self::update_pose) or [`advance`](Self::advance),
    /// and recomputes the matrices; for inverse kinematics or a turret
    /// aimed by code.
    pub fn set_node_local(&mut self, node: usize, local: Transform) {
        if node < self.locals.len() {
            self.locals[node] = local;
            let data = &self.model.inner;
            world_matrices(data, &self.locals, &mut self.world);
            for (skin, out) in data.skins.iter().zip(&mut self.joints) {
                out.clear();
                out.extend(skin.joints.iter().map(|(n, ibm)| self.world[*n] * *ibm));
            }
        }
    }

    /// A node's current model-space matrix, for attaching things to a hand
    /// or a muzzle.
    pub fn node_matrix(&self, node: usize) -> Option<Mat4> {
        self.world.get(node).copied()
    }

    /// The current morph weights of a node's mesh.
    pub fn morph_weights(&self, node: usize) -> &[f32] {
        self.weights.get(node).map_or(&[], Vec::as_slice)
    }

    /// Sets the morph weights of a node's mesh until the next pose update.
    pub fn set_morph_weights(&mut self, node: usize, weights: &[f32]) {
        if let Some(w) = self.weights.get_mut(node) {
            w.clear();
            w.extend_from_slice(weights);
        }
    }

    /// Draws part `part` with `material` instead of the model's own, or
    /// with its own again for `None`. The model is not changed.
    pub fn set_material_override(&mut self, part: usize, material: Option<Material>) {
        if let Some(o) = self.overrides.get_mut(part) {
            *o = material;
        }
    }

    /// The parts, with the index each override uses.
    pub fn parts(&self) -> &[ModelPart] {
        self.model.parts()
    }
}
