//! glTF models: meshes, materials, the node hierarchy, skins, morph
//! targets and animation clips, drawn through a [`ModelInstance`] that
//! holds one pose.

mod draw;
mod extensions;
mod instance;
mod load;
mod parts;
mod textures;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use yowie_anim::Clip;
use yowie_core::{Mat4, Transform};

use super::bounds::Aabb;
use super::material::Material;
use super::mesh::Mesh;
use crate::Graphics;
use crate::error::Result;

pub use instance::ModelInstance;

/// One node of a model's hierarchy.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) parent: Option<usize>,
    pub(crate) rest: Transform,
    pub(crate) skin: Option<usize>,
    /// Default morph weights of the node's mesh.
    pub(crate) weights: Vec<f32>,
}

/// One skin: for each entry of its joint list (the order `JOINTS_0`
/// indexes), the joint's node and inverse bind matrix.
#[derive(Clone, Debug)]
pub(crate) struct Skin {
    pub(crate) joints: Vec<(usize, Mat4)>,
}

/// One drawable piece of a model: a glTF primitive placed by a node.
#[derive(Clone, Debug)]
pub struct ModelPart {
    /// The material's name in the file, or the mesh's when the material
    /// has none.
    pub name: String,
    /// The index of the node that places the part.
    pub node: usize,
    /// The geometry.
    pub mesh: Mesh,
    /// The index of the part's material in [`Model::materials`].
    pub material: usize,
}

#[derive(Debug)]
pub(crate) struct ModelData {
    pub(crate) nodes: Vec<Node>,
    /// Node indices, parents before children.
    pub(crate) order: Vec<usize>,
    pub(crate) node_names: Vec<String>,
    pub(crate) node_index: HashMap<String, usize>,
    pub(crate) parts: Vec<ModelPart>,
    pub(crate) materials: Vec<Material>,
    pub(crate) skins: Vec<Skin>,
    pub(crate) clips: Vec<Arc<Clip>>,
    pub(crate) bounds: Aabb,
}

/// A glTF model on the GPU. Cloning shares it. Draw it through a
/// [`ModelInstance`], which holds a pose and an animation player.
///
/// Loading reads `.gltf` (with external or embedded buffers) and `.glb`
/// files: every triangle primitive becomes a [`Mesh`], every image a
/// texture, and materials keep their metallic-roughness factors and
/// textures, alpha mode, double-sidedness, `KHR_materials_unlit`,
/// `KHR_texture_transform` (the base colour texture's transform applies
/// to all of a material's textures), `KHR_materials_emissive_strength`,
/// and the layer extensions `KHR_materials_clearcoat`, `_sheen`,
/// `_transmission`, `_volume`, `_ior`, `_specular`, `_iridescence` and
/// `_anisotropy` (their textures read the base colour's UV set; factors
/// kept in two images are packed into one texture). Skins, morph targets
/// and animation clips come through `yowie_anim::gltf_import`.
#[derive(Clone, Debug)]
pub struct Model {
    pub(crate) inner: Arc<ModelData>,
}

impl Model {
    /// Loads a `.gltf` or `.glb` file; external buffers and images resolve
    /// relative to it.
    pub fn load(gfx: &Graphics, path: impl AsRef<Path>) -> Result<Model> {
        load::load_path(gfx, path.as_ref())
    }

    /// Loads glTF or GLB data from memory. Buffers and images must be
    /// embedded (a GLB binary chunk or data URIs).
    pub fn from_slice(gfx: &Graphics, bytes: &[u8]) -> Result<Model> {
        load::load_slice(gfx, bytes)
    }

    /// The drawable parts, in file order.
    pub fn parts(&self) -> &[ModelPart] {
        &self.inner.parts
    }

    /// The materials, in file order (plus a default one at the end when a
    /// primitive names none).
    pub fn materials(&self) -> &[Material] {
        &self.inner.materials
    }

    /// The animation clips, in file order. Track targets are node names.
    pub fn clips(&self) -> &[Arc<Clip>] {
        &self.inner.clips
    }

    /// The clip called `name`.
    pub fn clip(&self, name: &str) -> Option<&Arc<Clip>> {
        self.inner.clips.iter().find(|c| c.name == name)
    }

    /// The number of nodes.
    pub fn node_count(&self) -> usize {
        self.inner.nodes.len()
    }

    /// A node's unique name: its own, or `node<index>` when it has none or
    /// shares it.
    pub fn node_name(&self, node: usize) -> Option<&str> {
        self.inner.node_names.get(node).map(String::as_str)
    }

    /// The index of the node called `name`.
    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.inner.node_index.get(name).copied()
    }

    /// A node's parent.
    pub fn node_parent(&self, node: usize) -> Option<usize> {
        self.inner.nodes.get(node).and_then(|n| n.parent)
    }

    /// The box holding the model in its rest pose, in model space. Use it
    /// to frame a camera on a model whose size is unknown.
    pub fn bounds(&self) -> Aabb {
        self.inner.bounds
    }

    /// A new instance in the rest pose.
    pub fn instance(&self) -> ModelInstance {
        ModelInstance::new(self)
    }
}
