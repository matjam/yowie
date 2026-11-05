// Declarations every post-processing pass shares: the post block (one
// per scene or 2D frame, written once a frame; `uniform.rs` mirrors it),
// its sampler and the full-screen triangle. Texture coordinates have
// their origin at the top-left; depth is reversed (1 near, 0 far).

struct Post {
    // Premultiplied linear colour behind the picture.
    background: vec4<f32>,
    // Exposure, tone curve, flag bits, time in seconds.
    tone: vec4<f32>,
    // Bloom strength, threshold, knee, ghosts.
    bloom: vec4<f32>,
    // Occlusion strength, radius in world units, 0, 0.
    occlusion: vec4<f32>,
    // The white balance matrix's columns.
    balance0: vec4<f32>,
    balance1: vec4<f32>,
    balance2: vec4<f32>,
    // Contrast, saturation, lookup table strength, lookup table size.
    grade: vec4<f32>,
    lift: vec4<f32>,
    gamma: vec4<f32>,
    gain: vec4<f32>,
    // Vignette, aberration, distortion, grain.
    lens: vec4<f32>,
    // The picture's width and height in pixels and their reciprocals.
    size: vec4<f32>,
    // The sun in texture coordinates, shaft strength, decay.
    rays0: vec4<f32>,
    // Steps, density, weight per step, 0.
    rays1: vec4<f32>,
    // The light's colour, 0.
    rays2: vec4<f32>,
    // Focus distance, focus range, blur radius in half-size pixels, taps.
    dof: vec4<f32>,
    // Motion strength, motion taps, temporal blend, history valid.
    temporal: vec4<f32>,
    // Reflection strength, roughness cut-off, distance, steps.
    ssr: vec4<f32>,
    // Exposure compensation in stops, adaptation this frame (0 to 1),
    // jitter in pixels (x, y).
    exposure: vec4<f32>,
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // This frame's clip space to the previous frame's, for the camera's
    // motion.
    reproject: mat4x4<f32>,
}

const FLAG_GRADE: u32 = 1u;
const FLAG_LUT: u32 = 2u;
const FLAG_LENS: u32 = 4u;
const FLAG_BLOOM: u32 = 8u;
const FLAG_RAYS: u32 = 16u;
const FLAG_AO: u32 = 32u;
const FLAG_SHOW_AO: u32 = 64u;
const FLAG_AUTO_EXPOSURE: u32 = 128u;
const FLAG_FLAT: u32 = 256u;

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var linear_clamp: sampler;

struct FullOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FullOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FullOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The view-space position of a point at texture coordinates `uv` and
// stored depth `d`.
fn view_pos(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let p = post.inv_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return p.xyz / p.w;
}

// Distance in front of the camera at stored depth `d`.
fn view_distance(uv: vec2<f32>, d: f32) -> f32 {
    return -view_pos(uv, d).z;
}

// Interleaved gradient noise (Jimenez): a per-pixel value in 0..1.
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(0.06711056 * p.x + 0.00583715 * p.y));
}

const PI: f32 = 3.14159265;

// Clip-space x and y to texture coordinates.
fn ndc_to_uv(ndc: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// The unit vector an octahedral pair encodes (the surface image's xy).
fn oct_decode(e: vec2<f32>) -> vec3<f32> {
    var n = vec3<f32>(e, 1.0 - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.y += select(t, -t, n.y >= 0.0);
    return normalize(n);
}

// A world-space direction in view space.
fn to_view(d: vec3<f32>) -> vec3<f32> {
    return (post.view * vec4<f32>(d, 0.0)).xyz;
}

// Whether the projection is orthographic.
fn ortho() -> bool {
    return post.proj[3][3] > 0.5;
}

// The direction from a view-space point to the eye.
fn to_eye(p: vec3<f32>) -> vec3<f32> {
    if ortho() {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    return normalize(-p);
}

// Where the point at `uv` and stored depth `d` was on screen last frame,
// following the camera alone.
fn reproject(uv: vec2<f32>, d: f32) -> vec2<f32> {
    let prev = post.reproject * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return ndc_to_uv(prev.xy / prev.w);
}
