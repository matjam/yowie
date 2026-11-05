//! glTF import of skeletons (skins) and animation clips.
//!
//! Meshes and materials are not read here; the renderer imports them. To
//! connect the two, every imported skeleton carries the mapping between
//! glTF node indices and joint indices: a mesh's skin uses joint order
//! `joint_nodes`, and animation tracks are named after nodes through
//! [`GltfImport::node_names`].

mod animation;

use std::path::Path;

use gltf::Gltf;
use yowie_core::{Mat4, Quat, Transform, Vec3};

use crate::clip::Clip;
use crate::error::{AnimError, Result};
use crate::skeleton::{Joint, Skeleton};

/// A skeleton built from one glTF skin.
#[derive(Clone, Debug)]
pub struct GltfSkeleton {
    /// The joint hierarchy, parents first, with the file's inverse bind
    /// matrices.
    pub skeleton: Skeleton,
    /// Index of the skin in the glTF file.
    pub skin_index: usize,
    /// For every glTF node, the joint index it became, or `None` when the
    /// node is not part of this skin.
    pub node_to_joint: Vec<Option<usize>>,
    /// For every joint, the glTF node index it came from.
    pub joint_nodes: Vec<usize>,
    /// For each entry of the skin's own `joints` list, in file order, the
    /// joint index it became. A mesh's `JOINTS_0` attribute indexes this
    /// list, so `skin_order[attribute]` gives the joint matrix to use.
    pub skin_order: Vec<usize>,
}

/// Everything imported from a glTF file.
#[derive(Clone, Debug, Default)]
pub struct GltfImport {
    /// A unique name for every node, by node index: the node's own name,
    /// `node<index>` when it has none, or `<name>#<index>` when an earlier
    /// node already has that name. Track targets in the imported clips use
    /// these names.
    pub node_names: Vec<String>,
    /// One skeleton per skin.
    pub skeletons: Vec<GltfSkeleton>,
    /// One clip per animation. Clips loop by default.
    pub clips: Vec<Clip>,
}

/// Reads a `.gltf` or `.glb` file. External buffers resolve relative to
/// the file.
pub fn import_path(path: impl AsRef<Path>) -> Result<GltfImport> {
    let path = path.as_ref();
    let mut file = Gltf::open(path)?;
    let blob = file.blob.take();
    let buffers = gltf::import_buffers(&file.document, path.parent(), blob)?;
    build(&file.document, &buffers)
}

/// Reads glTF or GLB data from memory. Buffers must be embedded (a GLB
/// binary chunk or base64 data URIs).
pub fn import_slice(bytes: &[u8]) -> Result<GltfImport> {
    let mut file = Gltf::from_slice(bytes)?;
    let blob = file.blob.take();
    let buffers = gltf::import_buffers(&file.document, None, blob)?;
    build(&file.document, &buffers)
}

fn unique_names(doc: &gltf::Document) -> Vec<String> {
    let mut names: Vec<String> = doc
        .nodes()
        .map(|n| match n.name() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => format!("node{}", n.index()),
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    for (i, name) in names.iter_mut().enumerate() {
        if !seen.insert(name.clone()) {
            *name = format!("{name}#{i}");
            seen.insert(name.clone());
        }
    }
    names
}

fn node_transform(node: &gltf::Node) -> Transform {
    let (t, r, s) = node.transform().decomposed();
    Transform {
        translation: Vec3::from_array(t),
        rotation: Quat::from_array(r).normalize(),
        scale: Vec3::from_array(s),
    }
}

fn build(doc: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Result<GltfImport> {
    let node_names = unique_names(doc);
    let mut parents: Vec<Option<usize>> = vec![None; node_names.len()];
    for node in doc.nodes() {
        for child in node.children() {
            parents[child.index()] = Some(node.index());
        }
    }
    let skeletons = doc
        .skins()
        .map(|skin| build_skeleton(&skin, &node_names, &parents, buffers))
        .collect::<Result<Vec<_>>>()?;
    let clips = doc
        .animations()
        .map(|a| animation::build_clip(&a, &node_names, buffers))
        .collect::<Result<Vec<_>>>()?;
    Ok(GltfImport {
        node_names,
        skeletons,
        clips,
    })
}

fn build_skeleton(
    skin: &gltf::Skin,
    names: &[String],
    parents: &[Option<usize>],
    buffers: &[gltf::buffer::Data],
) -> Result<GltfSkeleton> {
    let nodes: Vec<gltf::Node> = skin.joints().collect();
    let reader = skin.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
    let ibm: Vec<Mat4> = match reader.read_inverse_bind_matrices() {
        Some(it) => it.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
        None => vec![Mat4::IDENTITY; nodes.len()],
    };
    if ibm.len() < nodes.len() {
        return Err(AnimError::Unsupported(
            "skin has fewer inverse bind matrices than joints".into(),
        ));
    }
    let in_skin = |n: usize| nodes.iter().any(|j| j.index() == n);
    let depth = |mut n: usize| {
        let mut d = 0;
        while let Some(p) = parents[n] {
            d += 1;
            n = p;
        }
        d
    };
    // Parents come before children: order by depth in the node tree.
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| depth(nodes[i].index()));

    let mut node_to_joint = vec![None; names.len()];
    for (joint, &i) in order.iter().enumerate() {
        node_to_joint[nodes[i].index()] = Some(joint);
    }
    let joints: Vec<Joint> = order
        .iter()
        .map(|&i| {
            let node = &nodes[i];
            let mut ancestor = parents[node.index()];
            while let Some(a) = ancestor.filter(|a| !in_skin(*a)) {
                ancestor = parents[a];
            }
            Joint {
                name: names[node.index()].clone(),
                parent: ancestor.and_then(|a| node_to_joint[a]),
                rest: node_transform(node),
                inverse_bind: ibm[i],
            }
        })
        .collect();
    let skin_order = nodes
        .iter()
        .filter_map(|n| node_to_joint[n.index()])
        .collect();
    Ok(GltfSkeleton {
        skeleton: Skeleton::new(joints)?,
        skin_index: skin.index(),
        node_to_joint,
        joint_nodes: order.iter().map(|&i| nodes[i].index()).collect(),
        skin_order,
    })
}

#[cfg(test)]
mod tests;
