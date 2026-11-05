//! Static geometry: groups of draws a game builds once and shows every
//! frame without paying for them draw by draw.
//!
//! A [`StaticGroup`] holds meshes, materials and placements. The first
//! time a scene draws it, its items are ordered by a bounding volume
//! hierarchy (`bvh.rs`) and those with equal mesh, material and pipeline
//! are merged into instanced batches (`layout.rs`). The renderer then
//! keeps, per group, the items' instance data and bounds on the GPU, and
//! per scene slot two render bundles that replay every batch as an
//! indirect draw (`gpu.rs`, `bundle.rs`). Bundles are recorded again only
//! when the group, a material's textures, a mesh or the scene's bind
//! group changes.
//!
//! Each frame a compute pass culls the items against the view and against
//! a depth pyramid (two-phase occlusion culling, `cull.wgsl`): items that
//! were visible last frame and are inside the view draw first; the depth
//! they and the ordinary draws left is reduced into the pyramid
//! (`hiz.wgsl`); every item inside the view is tested against it, those
//! that became visible draw in a second pass, and the result is next
//! frame's visibility. Only depth drawn this frame hides an item, so
//! nothing visible is skipped.
//!
//! Shadow maps take the group's casters from the hierarchy on the
//! processor. Blended items (which need sorting) draw as ordinary draws.
//! On adapters without compute shaders or indirect draws every item draws
//! as an ordinary draw, culled against the view on the processor.

mod bundle;
mod bvh;
mod compute;
mod gpu;
mod layout;
mod state;

#[cfg(test)]
mod gpu_tests;

use std::sync::{Arc, OnceLock};

use yowie_core::{Mat4, Vec3};

pub(crate) use state::StaticState;

use super::bounds::{Aabb, Frustum};
use super::material::Material;
use super::mesh::Mesh;
use super::pipelines::{MeshKey, MeshPass};
use super::prepare::{Batch, pack_key};
use super::record::SceneRecord;
use super::submit::{Placement, Scene};
use super::vertex::{GpuInstance, affine_rows};
use crate::texture::next_id;
use layout::Layout;

/// One mesh drawn with a material at a placement.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) mesh: Mesh,
    pub(crate) material: Material,
    pub(crate) model: Mat4,
}

/// A group's items and, once a scene has drawn it, their layout.
#[derive(Debug)]
pub(crate) struct StaticData {
    /// Names this content; every edit gives the group a new id, which
    /// makes the renderer rebuild its GPU data.
    pub(crate) id: u64,
    pub(crate) items: Vec<Item>,
    layout: OnceLock<Layout>,
}

impl Clone for StaticData {
    fn clone(&self) -> Self {
        Self {
            id: next_id(),
            items: self.items.clone(),
            layout: OnceLock::new(),
        }
    }
}

impl StaticData {
    pub(crate) fn layout(&self) -> &Layout {
        self.layout.get_or_init(|| Layout::build(&self.items))
    }

    /// The items drawn through GPU culling (all but the ordinary ones).
    pub(crate) fn gpu_items(&self) -> u32 {
        self.layout().order.len() as u32
    }
}

/// Geometry that does not move, built once and drawn every frame with
/// [`Scene::draw_static`]: level architecture, props, terrain pieces.
///
/// Drawing a group costs the processor almost nothing per item: the
/// renderer orders its items in a bounding volume hierarchy, merges those
/// sharing a mesh and material into instanced batches, and replays them
/// from render bundles while the GPU culls them against the view and
/// against what nearer geometry hides
/// ([`FrameStats::occluded_3d`](crate::FrameStats::occluded_3d)). Editing
/// the group rebuilds that on the next draw, so keep moving things out of
/// it. Cloning shares the items until one clone is edited.
///
/// ```no_run
/// # use yowie_gfx::{Graphics, scene::*};
/// # use yowie_core::{Color, Vec3};
/// # fn f(gfx: &mut Graphics, cube: &Mesh, cam: &Camera3D) {
/// let mut city = StaticGroup::new();
/// let stone = Material::color(Color::rgb8(150, 140, 130), 0.0, 0.9);
/// for i in 0..1000 {
///     city.add(cube, &stone, Vec3::new((i % 40) as f32 * 3.0, 0.0, (i / 40) as f32 * 3.0));
/// }
/// gfx.scene(cam, |s| s.draw_static(&city));
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct StaticGroup {
    data: Arc<StaticData>,
}

impl Default for StaticGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticGroup {
    /// An empty group.
    pub fn new() -> Self {
        Self {
            data: Arc::new(StaticData {
                id: next_id(),
                items: Vec::new(),
                layout: OnceLock::new(),
            }),
        }
    }

    fn edit(&mut self) -> &mut StaticData {
        let d = Arc::make_mut(&mut self.data);
        d.id = next_id();
        d.layout = OnceLock::new();
        d
    }

    /// Adds `mesh` drawn with `material` at `placement` and returns its
    /// index. Skinned and morphed meshes draw in their rest shape.
    pub fn add(&mut self, mesh: &Mesh, material: &Material, placement: impl Placement) -> usize {
        let d = self.edit();
        d.items.push(Item {
            mesh: mesh.clone(),
            material: material.clone(),
            model: placement.model_matrix(),
        });
        d.items.len() - 1
    }

    /// Moves item `index`; does nothing when there is no such item.
    pub fn set_placement(&mut self, index: usize, placement: impl Placement) {
        if index < self.data.items.len() {
            self.edit().items[index].model = placement.model_matrix();
        }
    }

    /// Removes every item.
    pub fn clear(&mut self) {
        self.edit().items.clear();
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.data.items.len()
    }

    /// Whether the group has no items.
    pub fn is_empty(&self) -> bool {
        self.data.items.is_empty()
    }

    /// The world box holding every item.
    pub fn bounds(&self) -> Aabb {
        self.data.items.iter().fold(Aabb::EMPTY, |b, i| {
            b.union(i.mesh.bounds().transformed(&i.model))
        })
    }
}

/// A static group a scene draws, with its meshes and materials' slots in
/// the scene's tables.
#[derive(Clone, Debug)]
pub(crate) struct StaticUse {
    pub(crate) data: Arc<StaticData>,
    /// Scene mesh slot of each of the layout's meshes.
    pub(crate) meshes: Vec<u32>,
    /// Scene material slot of each of the layout's materials.
    pub(crate) materials: Vec<u32>,
}

impl Scene<'_> {
    /// Draws a static group. Its opaque and cut-out items are culled and
    /// drawn on the GPU from render bundles. Items whose material blends,
    /// refracts, uses the stencil, has fur, an outline, an x-ray tint or a
    /// surface shader, and every item on GPUs without compute shaders or
    /// indirect draws, draw as ordinary draws.
    pub fn draw_static(&mut self, group: &StaticGroup) {
        let data = &group.data;
        let layout = data.layout();
        let fallback: Vec<u32> = if self.rec.gpu_statics {
            layout.ordinary.clone()
        } else {
            (0..data.items.len() as u32).collect()
        };
        for i in fallback {
            let it = &data.items[i as usize];
            self.push(&it.mesh, &it.material, it.model, u32::MAX, &[]);
        }
        if !self.rec.gpu_statics || layout.order.is_empty() {
            return;
        }
        let meshes = layout
            .meshes
            .iter()
            .map(|m| self.rec.mesh_slot(m))
            .collect();
        let materials = layout
            .materials
            .iter()
            .map(|m| self.rec.material_slot(m))
            .collect();
        self.rec.statics.push(StaticUse {
            data: data.clone(),
            meshes,
            materials,
        });
    }
}

/// How far towards the sun the scene's static groups reach (for the
/// cascade near planes); negative infinity without groups.
pub(crate) fn caster_reach(rec: &SceneRecord, to_sun: Vec3) -> f32 {
    rec.statics
        .iter()
        .map(|s| {
            let b = s.data.layout().bounds;
            if b.is_empty() {
                return f32::NEG_INFINITY;
            }
            b.center().dot(to_sun) + b.half_extents().dot(to_sun.abs())
        })
        .fold(f32::NEG_INFINITY, f32::max)
}

/// Appends the instances and batches of the static items whose boxes
/// touch shadow view `view`'s frustum. Returns the instances added.
pub(crate) fn emit_casters(
    rec: &SceneRecord,
    frustum: &Frustum,
    view: u32,
    instances: &mut Vec<GpuInstance>,
    batches: &mut Vec<Batch>,
) -> u32 {
    let mut found: Vec<(u64, MeshKey, u32, u32, Mat4)> = Vec::new();
    for s in &rec.statics {
        let layout = s.data.layout();
        layout.bvh.walk(frustum, |k| {
            // The same sphere test ordinary casters get.
            let b = layout.boxes[k as usize];
            if !frustum.intersects_sphere(b.center(), b.radius()) {
                return;
            }
            let i = layout.order[k as usize] as usize;
            let mat = s.materials[layout.item_material[i] as usize];
            let mesh = s.meshes[layout.item_mesh[i] as usize];
            let pass = if layout.item_key[i].pass == MeshPass::Mask {
                MeshPass::ShadowMask
            } else {
                MeshPass::ShadowOpaque
            };
            let key = MeshKey {
                pass,
                double_sided: true,
                ..MeshKey::default()
            };
            // Opaque casters ignore the material, so they batch by mesh.
            let m = if pass == MeshPass::ShadowMask { mat } else { 0 };
            let model = s.data.items[i].model;
            found.push((pack_key(&key, m, mesh), key, mesh, mat, model));
        });
    }
    found.sort_unstable_by_key(|f| f.0);
    let added = found.len() as u32;
    for (sort, key, mesh, material, model) in found {
        let at = instances.len() as u32;
        instances.push(GpuInstance {
            rows: affine_rows(&model),
            params: [material, u32::MAX, 0, view << 8],
        });
        match batches.last_mut() {
            Some(b) if b.sort == sort && b.first + b.count == at => b.count += 1,
            _ => batches.push(Batch {
                key,
                sort,
                mesh,
                material,
                first: at,
                count: 1,
            }),
        }
    }
    added
}
