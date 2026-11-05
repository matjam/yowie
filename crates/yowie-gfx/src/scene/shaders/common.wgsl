// Declarations every scene shader shares: the frame block, the storage
// records and group 0. Mirrors `frame_data.rs`, `material.rs` and
// `light.rs`; a change here is a change there.

struct ShadowView {
    view_proj: mat4x4<f32>,
    // Atlas texture coordinates: offset in xy, scale in zw.
    rect: vec4<f32>,
    // Normal offset in world units, 0, 0, 0.
    params: vec4<f32>,
}

struct Frame {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // Camera position, time in seconds.
    camera: vec4<f32>,
    // Target width, height, and their reciprocals, in pixels.
    target_size: vec4<f32>,
    // Unit vector towards the sun; w is 1 when there is a sun.
    sun_dir: vec4<f32>,
    // Linear sun radiance; w is the shadow strength.
    sun_color: vec4<f32>,
    // Linear ambient light used when no maps are bound.
    ambient: vec4<f32>,
    // Intensity, specular mip count, 1 when maps are bound, background
    // (0 none, 1 procedural sky, 2 environment cube).
    env: vec4<f32>,
    // Sky up axis; w is the star brightness.
    sky_up: vec4<f32>,
    sky_zenith: vec4<f32>,
    // w is the air (1 minus vacuum).
    sky_horizon: vec4<f32>,
    sky_ground: vec4<f32>,
    // Linear sun disc radiance; w is its angular radius.
    sun_disc: vec4<f32>,
    // Planet radius, air height, Rayleigh and Mie scale heights.
    atmos: vec4<f32>,
    // Rayleigh scattering per unit; w is the intensity.
    beta_r: vec4<f32>,
    // Mie scattering, forward lobe, altitude, 1 when enabled.
    beta_m: vec4<f32>,
    // Linear fog colour; w is the exponential density.
    fog_color: vec4<f32>,
    // Linear start, linear end, height, height falloff.
    fog_params: vec4<f32>,
    // Tiles across, tiles down, depth slices, tile size in pixels.
    cluster: vec4<f32>,
    // Near, far, scale and bias: slice = log(depth) * scale + bias.
    cluster_depth: vec4<f32>,
    // 1 / atlas width, 1 / atlas height, cascade count, unused.
    shadow: vec4<f32>,
    // The view depth where each cascade ends.
    cascade_far: vec4<f32>,
    // Light count, 1 for an orthographic camera, decal count, 0.
    counts: vec4<u32>,
    shadow_views: array<ShadowView, 31>,
}

struct Material {
    base_color: vec4<f32>,
    // Emissive times strength; w is the normal scale.
    emissive: vec4<f32>,
    // Metallic, roughness, occlusion strength, alpha cutoff.
    params: vec4<f32>,
    uv_row0: vec4<f32>,
    uv_row1: vec4<f32>,
    // Flags, coordinate set bits, layered feature flags, layered feature
    // textures present.
    flags: vec4<u32>,
    // The layered features' parameters (see `layers.rs`).
    ext: array<vec4<f32>, 10>,
}

const MAT_UNLIT: u32 = 1u;
const MAT_VERTEX_COLORS: u32 = 2u;
const MAT_DOUBLE_SIDED: u32 = 4u;
const MAT_BLEND: u32 = 8u;
const MAT_NORMAL_MAP: u32 = 16u;
const MAT_DECALS: u32 = 32u;

// Bits of `flags.z`, one per layered feature.
const EXT_CLEARCOAT: u32 = 1u;
const EXT_SHEEN: u32 = 2u;
const EXT_TRANSMISSION: u32 = 4u;
const EXT_IRIDESCENCE: u32 = 8u;
const EXT_ANISOTROPY: u32 = 16u;
const EXT_SPECULAR: u32 = 32u;
const EXT_SUBSURFACE: u32 = 64u;
const EXT_FUR: u32 = 128u;

struct Light {
    // Position and range.
    pos_range: vec4<f32>,
    // Linear radiance; w is 0 for a point light, 1 for a spot light.
    color_kind: vec4<f32>,
    // Spot direction (the way the light travels) and cos(outer angle).
    dir_cos_outer: vec4<f32>,
    // cos(inner angle), first shadow view or -1, 0, 0.
    params: vec4<f32>,
}

struct MorphWeight {
    target_index: u32,
    weight: f32,
}

struct MorphData {
    vertex_count: u32,
    target_count: u32,
    pad0: u32,
    pad1: u32,
    deltas: array<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> lights: array<Light>;
@group(0) @binding(2) var<storage, read> clusters: array<u32>;
@group(0) @binding(3) var<storage, read> light_indices: array<u32>;
@group(0) @binding(4) var<storage, read> materials: array<Material>;
@group(0) @binding(5) var<storage, read> joints: array<mat4x4<f32>>;
@group(0) @binding(6) var<storage, read> morph_weights: array<MorphWeight>;
@group(0) @binding(7) var shadow_atlas: texture_depth_2d;
@group(0) @binding(8) var shadow_sampler: sampler_comparison;
@group(0) @binding(9) var env_specular: texture_cube<f32>;
@group(0) @binding(10) var env_irradiance: texture_cube<f32>;
@group(0) @binding(11) var env_sampler: sampler;

const PI: f32 = 3.14159265359;
