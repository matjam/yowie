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
//!    the scene into an HDR colour image with depth and stencil: opaque,
//!    cut-out and fur meshes, the sky, outlines, refracting meshes over a
//!    blurred copy of the scene (`transmission.rs`), x-ray tints, blended
//!    meshes through order-independent transparency (`transparency.rs`)
//!    or back to front, debug lines (`render.rs` prepares and uploads,
//!    `features.rs` makes the feature targets, `passes.rs` records).
//!    Decals are records the surface program reads (`decals.rs`). The
//!    transparency composite runs inside this pass, before the post chain.
//! 3. Pass 2 runs the scene's post chain from the HDR image into the
//!    output (the `post` module, set up by [`PostSettings`]); with the
//!    default settings that is exposure and a tone curve alone. The 2D
//!    pass then draws over it.
//!
//! The world is right-handed with +Y up; distances are world units and
//! angles radians. Colours given to the API are sRGB with straight alpha;
//! the renderer lights in linear light.
//!
//! # Extension points for later work
//!
//! - **Pipelines that draw in the HDR pass** (meshes, sky, lines,
//!   particles, decals) must match its attachments, which depend on the
//!   post settings: multisampling, and the surface and specular images
//!   that ambient occlusion and reflections read (`targets.rs`). Key such
//!   a pipeline by the slot's `SceneFormat` and take its colour targets
//!   from `SceneFormat::color_targets` and its multisample state from
//!   `SceneFormat::multisample`; begin extra passes into the slot's
//!   targets with `Targets::color_attachments` and
//!   `Targets::depth_attachment`.
//! - **New post effects** are steps in `post/steps.rs`: a program in
//!   `post/pipelines.rs` (its texture bindings and shader file), an image
//!   kind in `post/targets.rs` and a field in `PostSettings`.
//! - **New material features** follow the steps in the `material` module
//!   documentation; the record has spare flag bits and two spare vectors.
//! - **Game surface shaders** ([`SurfaceShader`]) are WGSL appended to the
//!   engine's mesh program (`pipelines::mesh_source`); the engine's own
//!   hook is the identity `material` function.
//! - **Static geometry** ([`StaticGroup`], `statics/`) is culled on the GPU
//!   and replayed from render bundles; with static groups the HDR pass of
//!   `passes.rs` splits in two around the depth pyramid build. Items whose
//!   material needs a pass of its own (blending, glass, stencil, outlines,
//!   x-ray, fur, a surface shader) draw as ordinary draws
//!   (`classes::bundles_can_draw`).
//! - **Reflection probes and the light probe grid** (`probes/`) capture
//!   the scene through these same passes into cube faces
//!   (`SceneRenderer::render_view` with a capture slot) and are read by
//!   `ambient_specular` and `ambient_diffuse` in `probes/probes.wgsl`,
//!   which the base, clearcoat and glass reflections all go through.
//!   Captures composite blended surfaces sorted rather than through OIT,
//!   and glass in a capture has no scene copy: it blends its reflection
//!   over what lies behind it without refraction.
//! - **GPU particles** and other extensions queue a [`GpuDraw`](crate::GpuDraw)
//!   with [`Scene::draw_gpu`]; `passes.rs` (`encode_scene`) runs their
//!   compute work before the HDR pass and their draws after the blended
//!   meshes, beside the lines, with the frame bind group bound. Probe
//!   captures leave them out.
//! - **Large-scene classes** (terrain, water, billboards and impostors,
//!   dithered cross-fades) draw with their own pipelines inside the same
//!   passes; `large.rs` describes the calls that hook them in. Water and
//!   blended billboards join the order-independent transparency pass when
//!   the scene uses it. Probe captures leave the classes out too.

mod billboard;
mod billboard_render;
mod bounds;
mod camera;
mod classes;
mod cluster;
mod decals;
mod environment;
mod fade;
mod features;
mod frame_data;
mod ibl;
mod impostor;
mod label;
mod large;
mod large_pipelines;
mod layers;
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
mod post;
mod prepare;
mod primitives;
mod probes;
mod record;
mod render;
mod shadow;
mod sky;
mod slot;
mod statics;
mod stencil;
mod submit;
mod surface_shader;
mod targets;
mod terrain;
mod textures;
mod transmission;
mod transparency;
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
mod gpu_tests_masks;
#[cfg(test)]
mod gpu_tests_materials;
#[cfg(test)]
mod gpu_tests_models;
#[cfg(test)]
mod gpu_tests_post_materials;

pub use billboard::{Billboard, BillboardStyle, Facing};
pub use bounds::{Aabb, Frustum, Ray};
pub use camera::{Camera3D, Projection};
pub use cluster::MAX_LIGHTS;
pub use decals::{Decal, MAX_DECALS};
pub use environment::{Environment, EnvironmentOptions};
pub use impostor::{Impostor, ImpostorOptions, MAX_IMPOSTOR_VIEWS};
pub use label::LabelStyle;
pub use layers::{
    Anisotropy, Clearcoat, EXT_ANISOTROPY, EXT_CLEARCOAT, EXT_FUR, EXT_IRIDESCENCE, EXT_OUTLINE,
    EXT_SHEEN, EXT_SPECULAR, EXT_SUBSURFACE, EXT_TRANSMISSION, EXT_XRAY, Fur, Iridescence, Outline,
    Sheen, Specular, Transmission,
};
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use lod::{Lod, LodState};
pub use material::{AlphaMode, Material, MaterialGpu, TextureTransform, UvSets};
pub use mesh::{Mesh, RayHit};
pub use model::{Model, ModelInstance, ModelPart};
pub use morph::{MAX_MORPH_WEIGHTS, MorphTarget};
pub use panorama::Panorama;
pub use post::{Lut, PostSettings};
pub use probes::{
    LightProbeGrid, MAX_LIGHT_PROBES, MAX_REFLECTION_PROBES, ProbeUpdate, ReflectionProbe,
};
pub use shadow::{MAX_POINT_SHADOWS, MAX_SPOT_SHADOWS};
pub use sky::{Atmosphere, Fog, Sky, Tonemap};
pub use statics::StaticGroup;
pub use stencil::{Stencil, StencilOp, StencilTest};
pub use submit::{DrawId, Placement, Scene, ScenePick, pick};
pub use surface_shader::SurfaceShader;
pub use terrain::{
    Brush, BrushMode, HeightMap, LayerRule, SplatRules, Terrain, TerrainDrawOptions, TerrainLayer,
    TerrainOptions,
};
pub use transparency::Transparency;
pub use vertex::{MeshData, Vertex3D};
pub use water::{Water, WaterOptions};

pub(crate) use post::{HDR as FRAME_POST_FORMAT, PostRenderer};
pub(crate) use render::SceneRenderer;

/// Scenes recorded for one output.
pub(crate) type SceneRecordList = Vec<record::SceneRecord>;

impl crate::Graphics {
    /// The scene renderer, created the first time a scene is recorded.
    pub(crate) fn scene_renderer(&mut self) -> &mut SceneRenderer {
        let shared = &self.shared;
        self.scenes
            .get_or_insert_with(|| Box::new(SceneRenderer::new(shared)))
    }
}
