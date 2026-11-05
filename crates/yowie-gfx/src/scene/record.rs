//! What one scene recorded during `draw`: camera, settings, lights and
//! draws, resolved by the renderer in `end_frame`.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use bytemuck::{Pod, Zeroable};
use yowie_core::{Color, Mat4, Vec2};

use super::camera::Camera3D;
use super::decals::Decal;
use super::environment::Environment;
use super::layouts::MATERIAL_TEXTURES;
use super::light::{DirectionalLight, LocalLight};
use super::material::{Material, MaterialGpu};
use super::mesh::Mesh;
use super::morph::GpuMorphWeight;
use super::sky::{Fog, Sky, Tonemap};
use super::stencil::Stencil;
use super::transparency::Transparency;

/// A small multiplicative hasher for the per-draw lookups; the keys are
/// ids and material bytes, not attacker input.
#[derive(Default, Clone, Copy)]
pub(crate) struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.write_u64(u64::from_le_bytes(*w));
        }
        for b in rest {
            self.write_u64(u64::from(*b));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

/// A material as the intern table compares it: its GPU record's bits, its
/// texture ids, its stencil, and its surface shader's id and uniform
/// version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MaterialKey {
    bits: [u32; 64],
    textures: [u64; MATERIAL_TEXTURES as usize],
    stencil: Stencil,
    shader: (u64, u32),
}

/// One mesh draw: which mesh and material, where, and its joint and morph
/// data in the record's tables.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawRecord {
    pub(crate) mesh: u32,
    pub(crate) material: u32,
    pub(crate) model: Mat4,
    /// First joint matrix, or `u32::MAX`.
    pub(crate) joints: u32,
    pub(crate) morph_first: u32,
    pub(crate) morph_count: u32,
}

/// A line vertex: world position and linear, straight colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct LineVertex {
    pub(crate) position: [f32; 3],
    pub(crate) color: [f32; 4],
}

/// Everything one scene recorded.
#[derive(Debug, Default)]
pub(crate) struct SceneRecord {
    /// The draw list (output) the scene renders into.
    pub(crate) list: usize,
    pub(crate) camera: Camera3D,
    /// The output's view size in view units when the scene was recorded.
    pub(crate) view_size: Vec2,
    pub(crate) sun: Option<DirectionalLight>,
    pub(crate) ambient: Color,
    pub(crate) sky: Option<Sky>,
    pub(crate) environment: Option<Environment>,
    pub(crate) background: bool,
    pub(crate) fog: Option<Fog>,
    pub(crate) exposure: f32,
    pub(crate) tonemap: Tonemap,
    pub(crate) lights: Vec<LocalLight>,
    pub(crate) meshes: Vec<Mesh>,
    mesh_slots: FastMap<u64, u32>,
    last_mesh: Option<(u64, u32)>,
    pub(crate) materials: Vec<Material>,
    pub(crate) material_gpu: Vec<MaterialGpu>,
    material_slots: FastMap<MaterialKey, u32>,
    pub(crate) draws: Vec<DrawRecord>,
    pub(crate) joints: Vec<Mat4>,
    pub(crate) morph_weights: Vec<GpuMorphWeight>,
    pub(crate) lines: Vec<LineVertex>,
    pub(crate) lines_on_top: Vec<LineVertex>,
    /// The scene's time in seconds, which shaders read.
    pub(crate) time: f32,
    /// How blended surfaces are composited.
    pub(crate) transparency: Transparency,
    /// The projected decals and the matrices placing their boxes.
    pub(crate) decals: Vec<(Decal, Mat4)>,
    /// Snapshots of surface shader uniform blocks, one per material that
    /// has one.
    pub(crate) user_blocks: Vec<Vec<u8>>,
    /// Each material's uniform block in `user_blocks`, or `u32::MAX`.
    pub(crate) material_blocks: Vec<u32>,
    /// Custom GPU draws queued with `Scene::draw_gpu`.
    pub(crate) customs: Vec<crate::custom::CustomEntry>,
    /// The colour behind the scene in its viewport, premultiplied linear;
    /// set when the frame renders.
    pub(crate) background_color: [f32; 4],
    /// The output's clear colour, taken over from its draw list by the
    /// first scene of each output when the frame renders.
    pub(crate) output_clear: Option<Color>,
    /// Terrain, water, billboards, impostors and labels.
    pub(crate) large: super::large::LargeRecord,
    /// Reflection probes the scene shows, in the order added.
    pub(crate) probes: Vec<super::probes::ProbeUse>,
    /// The light probe grid lighting the scene.
    pub(crate) light_grid: Option<super::probes::GridUse>,
    /// Draws probe captures skip, in increasing order.
    pub(crate) probe_hidden: Vec<u32>,
    /// Static groups drawn through render bundles and GPU culling.
    pub(crate) statics: Vec<super::statics::StaticUse>,
    /// Whether static groups may use GPU culling and render bundles; when
    /// not, their items are recorded as ordinary draws.
    pub(crate) gpu_statics: bool,
}

impl SceneRecord {
    /// Empties the record for reuse, keeping its allocations.
    pub(crate) fn reset(&mut self, list: usize, camera: Camera3D, view_size: Vec2) {
        self.list = list;
        self.camera = camera;
        self.view_size = view_size;
        self.sun = None;
        self.ambient = Color::rgb(0.25, 0.27, 0.3);
        self.sky = None;
        self.environment = None;
        self.background = true;
        self.fog = None;
        self.exposure = 1.0;
        self.tonemap = Tonemap::default();
        self.lights.clear();
        self.meshes.clear();
        self.mesh_slots.clear();
        self.last_mesh = None;
        self.materials.clear();
        self.material_gpu.clear();
        self.material_slots.clear();
        self.draws.clear();
        self.joints.clear();
        self.morph_weights.clear();
        self.lines.clear();
        self.lines_on_top.clear();
        self.time = 0.0;
        self.transparency = Transparency::default();
        self.decals.clear();
        self.user_blocks.clear();
        self.material_blocks.clear();
        self.customs.clear();
        self.background_color = [0.0; 4];
        self.output_clear = None;
        self.large.clear();
        self.probes.clear();
        self.light_grid = None;
        self.probe_hidden.clear();
        self.statics.clear();
    }

    /// The static group items drawn through GPU culling, which count as
    /// draws but are not in `draws`.
    pub(crate) fn static_draws(&self) -> u32 {
        self.statics.iter().map(|s| s.data.gpu_items()).sum()
    }

    pub(crate) fn mesh_slot(&mut self, mesh: &Mesh) -> u32 {
        let id = mesh.id();
        if let Some((last, slot)) = self.last_mesh
            && last == id
        {
            return slot;
        }
        let next = self.meshes.len() as u32;
        let slot = *self.mesh_slots.entry(id).or_insert(next);
        if slot == next {
            self.meshes.push(mesh.clone());
        }
        self.last_mesh = Some((id, slot));
        slot
    }

    pub(crate) fn material_slot(&mut self, material: &Material) -> u32 {
        let gpu = material.gpu();
        let shader = material
            .shader
            .as_ref()
            .map_or((0, 0), |s| (s.id(), s.uniform_version()));
        let key = MaterialKey {
            bits: bytemuck::cast(gpu),
            textures: material.texture_ids(),
            stencil: material.stencil,
            shader,
        };
        let next = self.materials.len() as u32;
        let slot = *self.material_slots.entry(key).or_insert(next);
        if slot == next {
            self.materials.push(material.clone());
            self.material_gpu.push(gpu);
            let block = match &material.shader {
                Some(s) if s.uniform_size() > 0 => {
                    let (_, bytes) = s.uniforms();
                    self.user_blocks.push(bytes);
                    (self.user_blocks.len() - 1) as u32
                }
                _ => u32::MAX,
            };
            self.material_blocks.push(block);
        }
        slot
    }

    /// Appends a draw and returns its index.
    pub(crate) fn push_draw(&mut self, draw: DrawRecord) -> u32 {
        self.draws.push(draw);
        (self.draws.len() - 1) as u32
    }

    pub(crate) fn push_line(&mut self, a: [f32; 3], b: [f32; 3], color: Color, on_top: bool) {
        let c = color.to_linear();
        let list = if on_top {
            &mut self.lines_on_top
        } else {
            &mut self.lines
        };
        list.push(LineVertex {
            position: a,
            color: c,
        });
        list.push(LineVertex {
            position: b,
            color: c,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_materials_share_a_slot() {
        let mut rec = SceneRecord::default();
        let a = Material::color(Color::RED, 0.0, 0.5);
        let b = Material::color(Color::BLUE, 0.0, 0.5);
        assert_eq!(rec.material_slot(&a), 0);
        assert_eq!(rec.material_slot(&b), 1);
        assert_eq!(rec.material_slot(&a.clone()), 0);
        assert_eq!(rec.materials.len(), 2);
    }
}
