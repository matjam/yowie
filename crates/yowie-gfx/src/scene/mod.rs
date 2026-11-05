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
//! - **Order-independent transparency** replaces the sorted blended
//!   batches (`Prepared::blend`, built in `prepare.rs`, drawn in
//!   `passes.rs`) with an accumulation pass and a composite before the
//!   post chain.

mod bounds;
mod camera;
mod cluster;
mod environment;
mod frame_data;
mod ibl;
mod layouts;
mod light;
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
mod submit;
mod targets;
mod textures;
mod upload;
mod vertex;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod gpu_tests_models;

pub use bounds::{Aabb, Frustum, Ray};
pub use camera::{Camera3D, Projection};
pub use cluster::MAX_LIGHTS;
pub use environment::{Environment, EnvironmentOptions};
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use material::{
    AlphaMode, EXT_ANISOTROPY, EXT_CLEARCOAT, EXT_FUR, EXT_IRIDESCENCE, EXT_SHEEN, EXT_SPECULAR,
    EXT_SUBSURFACE, EXT_TRANSMISSION, Material, MaterialGpu, TextureTransform, UvSets,
};
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
pub use submit::{DrawId, Placement, Scene, ScenePick, pick};
pub use vertex::{MeshData, Vertex3D};

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
