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
//!    Decals are records the surface program reads (`decals.rs`).
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
//!   documentation; the record has spare flag bits and two spare vectors.
//! - **Game surface shaders** ([`SurfaceShader`]) are WGSL appended to the
//!   engine's mesh program (`pipelines::mesh_source`); the engine's own
//!   hook is the identity `material` function.
//! - **Render bundles** for static geometry: `passes.rs` records every
//!   batch through `SceneRenderer::draw_batches`, which only reads the
//!   batch list and bind groups, so a bundle encoder can replay a cached
//!   batch list for meshes the game marks static.
//! - **GPU particles** and other extensions queue a [`GpuDraw`](crate::GpuDraw)
//!   with [`Scene::draw_gpu`]; `passes.rs` (`encode_scene`) runs their
//!   compute work before the HDR pass and their draws after the blended
//!   meshes, beside the lines, with the frame bind group bound.

mod bounds;
mod camera;
mod classes;
mod cluster;
mod decals;
mod environment;
mod features;
mod frame_data;
mod ibl;
mod layers;
mod layouts;
mod light;
mod material;
mod mesh;
mod model;
mod morph;
mod panorama;
mod passes;
mod pipelines;
mod prepare;
mod primitives;
mod record;
mod render;
mod resolve;
mod shadow;
mod sky;
mod slot;
mod stencil;
mod submit;
mod surface_shader;
mod textures;
mod transmission;
mod transparency;
mod upload;
mod vertex;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod gpu_tests_masks;
#[cfg(test)]
mod gpu_tests_materials;
#[cfg(test)]
mod gpu_tests_models;

pub use bounds::{Aabb, Frustum, Ray};
pub use camera::{Camera3D, Projection};
pub use cluster::MAX_LIGHTS;
pub use decals::{Decal, MAX_DECALS};
pub use environment::{Environment, EnvironmentOptions};
pub use layers::{
    Anisotropy, Clearcoat, EXT_ANISOTROPY, EXT_CLEARCOAT, EXT_FUR, EXT_IRIDESCENCE, EXT_OUTLINE,
    EXT_SHEEN, EXT_SPECULAR, EXT_SUBSURFACE, EXT_TRANSMISSION, EXT_XRAY, Fur, Iridescence, Outline,
    Sheen, Specular, Transmission,
};
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use material::{AlphaMode, Material, MaterialGpu, TextureTransform, UvSets};
pub use mesh::{Mesh, RayHit};
pub use model::{Model, ModelInstance, ModelPart};
pub use morph::{MAX_MORPH_WEIGHTS, MorphTarget};
pub use panorama::Panorama;
pub use shadow::{MAX_POINT_SHADOWS, MAX_SPOT_SHADOWS};
pub use sky::{Atmosphere, Fog, Sky, Tonemap};
pub use stencil::{Stencil, StencilOp, StencilTest};
pub use submit::{DrawId, Placement, Scene, ScenePick, pick};
pub use surface_shader::SurfaceShader;
pub use transparency::Transparency;
pub use vertex::{MeshData, Vertex3D};

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
