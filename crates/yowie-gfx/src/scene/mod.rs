//! Physically based 3D: cameras, meshes, materials, lights and shadows,
//! sky and image-based lighting, glTF models, picking and debug lines.
//!
//! 3D is immediate-mode like 2D. During `draw` the game records a scene
//! with [`Graphics::scene`](crate::Graphics::scene); the renderer resolves
//! it when the frame ends:
//!
//! 1. The draws are culled against the camera, sorted by a packed key and
//!    merged into instanced runs (`prepare.rs`); lights are clustered
//!    (`cluster.rs`); shadow maps are placed in one atlas and each map
//!    gets the casters its frustum reaches (`shadow.rs`).
//! 2. Pass 1 of the frame (`frame.rs`) renders the shadow atlas and then
//!    the scene into an HDR colour image with depth: opaque and cut-out
//!    meshes, the sky, blended meshes back to front, debug lines
//!    (`render.rs` prepares and uploads, `passes.rs` records).
//! 3. Pass 2 resolves the HDR image into the output with exposure and a
//!    tone curve (`resolve.rs`); the 2D pass then draws over it.
//!
//! The world is right-handed with +Y up; distances are world units and
//! angles radians. Colours given to the API are sRGB with straight alpha;
//! the renderer lights in linear light.
//!
//! # Extension points for later work
//!
//! - **Post-processing** replaces [`resolve::resolve_output`]: it reads
//!   each scene slot's HDR colour and depth (`slot.rs`, `Targets`) and
//!   writes the output. Everything before it stays the same.
//! - **New material features** follow the steps in the `material` module
//!   documentation; the record has spare vectors and flag bits.
//! - **Static geometry** ([`StaticGroup`], `statics/`) is culled on the GPU
//!   and replayed from render bundles; with static groups the HDR pass of
//!   `passes.rs` splits in two around the depth pyramid build.
//! - **Reflection probes and the light probe grid** (`probes/`) capture
//!   the scene through these same passes into cube faces
//!   (`SceneRenderer::render_view` with a capture slot) and are read by
//!   `ambient_specular` and `ambient_diffuse` in `probes/probes.wgsl`.
//! - **GPU particles** and other extensions queue a [`GpuDraw`](crate::GpuDraw)
//!   with [`Scene::draw_gpu`]; `passes.rs` (`encode_scene`) runs their
//!   compute work before the HDR pass and their draws after the blended
//!   meshes, beside the lines, with the frame bind group bound. Probe
//!   captures leave them out.
//! - **Large-scene classes** (terrain, water, billboards and impostors,
//!   dithered cross-fades) draw with their own pipelines inside the same
//!   passes; `large.rs` describes the three calls that hook them in.
//!   Probe captures leave them out too.
//! - **Order-independent transparency** replaces the sorted blended
//!   batches (`Prepared::blend`, built in `prepare.rs`, drawn in
//!   `passes.rs`) with an accumulation pass and a composite before the
//!   resolve.

mod billboard;
mod billboard_render;
mod bounds;
mod camera;
mod cluster;
mod environment;
mod fade;
mod frame_data;
mod ibl;
mod impostor;
mod label;
mod large;
mod large_pipelines;
mod layouts;
mod light;
mod lod;
mod material;
mod mesh;
mod model;
mod morph;
mod panorama;
mod passes;
mod pipelines;
mod prepare;
mod primitives;
mod probes;
mod record;
mod render;
mod resolve;
mod shadow;
mod sky;
mod slot;
mod statics;
mod submit;
mod terrain;
mod textures;
mod upload;
mod vertex;
mod water;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod gpu_tests_air;
#[cfg(test)]
mod gpu_tests_large;
#[cfg(test)]
mod gpu_tests_models;

pub use billboard::{Billboard, BillboardStyle, Facing};
pub use bounds::{Aabb, Frustum, Ray};
pub use camera::{Camera3D, Projection};
pub use cluster::MAX_LIGHTS;
pub use environment::{Environment, EnvironmentOptions};
pub use impostor::{Impostor, ImpostorOptions, MAX_IMPOSTOR_VIEWS};
pub use label::LabelStyle;
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use lod::{Lod, LodState};
pub use material::{
    AlphaMode, EXT_ANISOTROPY, EXT_CLEARCOAT, EXT_FUR, EXT_IRIDESCENCE, EXT_SHEEN, EXT_SPECULAR,
    EXT_SUBSURFACE, EXT_TRANSMISSION, Material, MaterialGpu, TextureTransform, UvSets,
};
pub use mesh::{Mesh, RayHit};
pub use model::{Model, ModelInstance, ModelPart};
pub use morph::{MAX_MORPH_WEIGHTS, MorphTarget};
pub use panorama::Panorama;
pub use probes::{
    LightProbeGrid, MAX_LIGHT_PROBES, MAX_REFLECTION_PROBES, ProbeUpdate, ReflectionProbe,
};
pub use shadow::{MAX_POINT_SHADOWS, MAX_SPOT_SHADOWS};
pub use sky::{Atmosphere, Fog, Sky, Tonemap};
pub use statics::StaticGroup;
pub use submit::{DrawId, Placement, Scene, ScenePick, pick};
pub use terrain::{
    Brush, BrushMode, HeightMap, LayerRule, SplatRules, Terrain, TerrainDrawOptions, TerrainLayer,
    TerrainOptions,
};
pub use vertex::{MeshData, Vertex3D};
pub use water::{Water, WaterOptions};

pub(crate) use render::SceneRenderer;

/// Scenes recorded for one output.
pub(crate) type SceneRecordList = Vec<record::SceneRecord>;
pub(crate) use resolve::resolve_output;

impl crate::Graphics {
    /// The scene renderer, created the first time a scene is recorded.
    pub(crate) fn scene_renderer(&mut self) -> &mut SceneRenderer {
        let shared = &self.shared;
        self.scenes
            .get_or_insert_with(|| Box::new(SceneRenderer::new(shared)))
    }
}
