//! Laying out a static group's items: interning meshes and materials,
//! ordering the items by the hierarchy, and merging equal ones into
//! instanced batches.

use super::Item;
use super::bvh::Bvh;
use crate::scene::bounds::Aabb;
use crate::scene::material::{AlphaMode, Material};
use crate::scene::mesh::Mesh;
use crate::scene::pipelines::{MeshKey, MeshPass};

/// A run of instances sharing a pipeline, mesh and material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutBatch {
    pub(crate) key: MeshKey,
    /// Index into `Layout::meshes`.
    pub(crate) mesh: u32,
    /// Index into `Layout::materials`.
    pub(crate) material: u32,
    /// First position in `Layout::order`.
    pub(crate) first: u32,
    pub(crate) count: u32,
}

/// A group's items arranged for drawing.
#[derive(Debug, Default)]
pub(crate) struct Layout {
    /// Distinct meshes, by id.
    pub(crate) meshes: Vec<Mesh>,
    /// Distinct materials.
    pub(crate) materials: Vec<Material>,
    /// Per item: its mesh, material and pipeline.
    pub(crate) item_mesh: Vec<u32>,
    pub(crate) item_material: Vec<u32>,
    pub(crate) item_key: Vec<MeshKey>,
    /// The items drawn through GPU culling, in instance order: grouped by
    /// batch, and in hierarchy order within each batch.
    pub(crate) order: Vec<u32>,
    /// The world box of each entry of `order`.
    pub(crate) boxes: Vec<Aabb>,
    /// The batch of each entry of `order`.
    pub(crate) order_batch: Vec<u32>,
    pub(crate) batches: Vec<LayoutBatch>,
    /// The hierarchy over `order` positions, for shadow casters.
    pub(crate) bvh: Bvh,
    /// The box holding every entry of `order`.
    pub(crate) bounds: Aabb,
    /// Blended items, drawn as ordinary draws.
    pub(crate) blended: Vec<u32>,
}

impl Layout {
    pub(crate) fn build(items: &[Item]) -> Self {
        let mut l = Layout::default();
        let mut gpu: Vec<u32> = Vec::new();
        let mut boxes: Vec<Aabb> = Vec::new();
        for (i, it) in items.iter().enumerate() {
            let mesh = match l.meshes.iter().position(|m| m == &it.mesh) {
                Some(m) => m,
                None => {
                    l.meshes.push(it.mesh.clone());
                    l.meshes.len() - 1
                }
            };
            let material = match l.materials.iter().position(|m| m == &it.material) {
                Some(m) => m,
                None => {
                    l.materials.push(it.material.clone());
                    l.materials.len() - 1
                }
            };
            let pass = match it.material.alpha_mode {
                AlphaMode::Opaque => MeshPass::Opaque,
                AlphaMode::Mask(_) => MeshPass::Mask,
                AlphaMode::Blend => MeshPass::Blend,
            };
            l.item_mesh.push(mesh as u32);
            l.item_material.push(material as u32);
            l.item_key.push(MeshKey {
                pass,
                double_sided: it.material.double_sided,
                skinned: false,
            });
            if pass == MeshPass::Blend {
                l.blended.push(i as u32);
            } else {
                gpu.push(i as u32);
                boxes.push(it.mesh.bounds().transformed(&it.model));
            }
        }
        // Hierarchy order keeps neighbours together inside each batch.
        let spatial = Bvh::build(&boxes);
        let mut order: Vec<u32> = spatial.items.iter().map(|&k| gpu[k as usize]).collect();
        let batch_key = |i: u32| {
            let i = i as usize;
            (l.item_key[i].index(), l.item_material[i], l.item_mesh[i])
        };
        order.sort_by_key(|&i| batch_key(i));
        let mut box_of = vec![Aabb::EMPTY; items.len()];
        for (k, &i) in gpu.iter().enumerate() {
            box_of[i as usize] = boxes[k];
        }
        for (pos, &i) in order.iter().enumerate() {
            let fresh = l
                .batches
                .last()
                .is_none_or(|b| batch_key(order[b.first as usize]) != batch_key(i));
            if fresh {
                l.batches.push(LayoutBatch {
                    key: l.item_key[i as usize],
                    mesh: l.item_mesh[i as usize],
                    material: l.item_material[i as usize],
                    first: pos as u32,
                    count: 0,
                });
            }
            if let Some(b) = l.batches.last_mut() {
                b.count += 1;
            }
            l.order_batch.push((l.batches.len() - 1) as u32);
            l.boxes.push(box_of[i as usize]);
        }
        l.bounds = l.boxes.iter().fold(Aabb::EMPTY, |b, x| b.union(*x));
        l.bvh = Bvh::build(&l.boxes);
        l.order = order;
        l
    }
}
