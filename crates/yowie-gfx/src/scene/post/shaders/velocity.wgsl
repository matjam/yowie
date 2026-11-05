// Motion of moved objects: each draw the game gave a previous transform
// renders again with this frame's and last frame's model matrices, and
// writes how far its pixels moved on screen since last frame (in texture
// coordinates), with 1 in alpha. A fragment behind the visible surface
// (the chain's depth image) is dropped, so only what is seen writes.

@group(1) @binding(0) var depth_tex: texture_2d<f32>;

struct VelocityIn {
    @location(0) position: vec3<f32>,
    @location(1) row0: vec4<f32>,
    @location(2) row1: vec4<f32>,
    @location(3) row2: vec4<f32>,
    @location(4) prev0: vec4<f32>,
    @location(5) prev1: vec4<f32>,
    @location(6) prev2: vec4<f32>,
}

struct VelocityOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) prev: vec4<f32>,
}

fn place(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, p: vec3<f32>) -> vec4<f32> {
    let h = vec4<f32>(p, 1.0);
    return vec4<f32>(dot(r0, h), dot(r1, h), dot(r2, h), 1.0);
}

@vertex
fn vs_velocity(v: VelocityIn) -> VelocityOut {
    var o: VelocityOut;
    o.clip = post.view_proj * place(v.row0, v.row1, v.row2, v.position);
    // reproject * view_proj is last frame's view-projection.
    o.prev = post.reproject * post.view_proj * place(v.prev0, v.prev1, v.prev2, v.position);
    return o;
}

@fragment
fn fs_velocity(in: VelocityOut) -> @location(0) vec4<f32> {
    let d = textureLoad(depth_tex, vec2<i32>(in.clip.xy), 0).r;
    if in.clip.z < d - max(d * 2e-3, 1e-6) {
        discard;
    }
    let uv = in.clip.xy * post.size.zw;
    return vec4<f32>(uv - ndc_to_uv(in.prev.xy / in.prev.w), 0.0, 1.0);
}
