//! Models assembled in code from meshes, materials and placements.

use std::collections::HashMap;
use std::sync::Arc;

use yowie_core::{Mat4, Transform};

use super::{Model, ModelData, ModelPart, Node};
use crate::error::{GfxError, Result};
use crate::scene::bounds::Aabb;
use crate::scene::material::Material;
use crate::scene::mesh::Mesh;

impl Model {
    /// A static model built from parts: each a mesh, its material and the
    /// placement (translation, rotation and scale) of the mesh within the
    /// model. Each part gets its own node, named `part<index>`. Use it to
    /// draw, impostor-bake or level-of-detail a model made in code the
    /// same way as a loaded one. Fails when there are no parts.
    pub fn from_parts(parts: &[(Mesh, Material, Mat4)]) -> Result<Model> {
        if parts.is_empty() {
            return Err(GfxError::Mesh("a model needs at least one part".into()));
        }
        let mut nodes = Vec::with_capacity(parts.len());
        let mut model_parts = Vec::with_capacity(parts.len());
        let mut materials: Vec<Material> = Vec::new();
        let mut bounds = Aabb::EMPTY;
        for (i, (mesh, material, m)) in parts.iter().enumerate() {
            let rest = Transform::from_matrix(*m);
            nodes.push(Node {
                parent: None,
                rest,
                skin: None,
                weights: Vec::new(),
            });
            let index = match materials.iter().position(|x| x == material) {
                Some(at) => at,
                None => {
                    materials.push(material.clone());
                    materials.len() - 1
                }
            };
            bounds = bounds.union(mesh.bounds().transformed(&rest.to_matrix()));
            model_parts.push(ModelPart {
                name: format!("part{i}"),
                node: i,
                mesh: mesh.clone(),
                material: index,
            });
        }
        let node_names: Vec<String> = (0..parts.len()).map(|i| format!("part{i}")).collect();
        let node_index: HashMap<String, usize> = node_names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        Ok(Model {
            inner: Arc::new(ModelData {
                order: (0..nodes.len()).collect(),
                nodes,
                node_names,
                node_index,
                parts: model_parts,
                materials,
                skins: Vec::new(),
                clips: Vec::new(),
                bounds,
            }),
        })
    }
}
