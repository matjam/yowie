// Mesh draws dithered out for cross-fades: the static mesh program with
// the instance's hidden fraction (its third parameter, as float bits)
// carried to the fragment program, which drops the pixels whose dither
// threshold is below it and lights the rest like `fs_mask`. Mirrors
// `fade.rs`.

struct FadeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) color: vec4<f32>,
    @location(6) @interpolate(flat) material: u32,
    @location(7) view_depth: f32,
    @location(8) @interpolate(flat) hidden: f32,
}

@vertex
fn vs_fade(v: VertexIn) -> FadeOut {
    var plain = v;
    // The hidden fraction rides in the morph slot; no morph is applied.
    plain.params.w = 0u;
    let o = lit_out(plain, place(plain, mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0))));
    var f: FadeOut;
    f.clip = o.clip;
    f.world = o.world;
    f.normal = o.normal;
    f.tangent = o.tangent;
    f.uv = o.uv;
    f.uv2 = o.uv2;
    f.color = o.color;
    f.material = o.material;
    f.view_depth = o.view_depth;
    f.hidden = bitcast<f32>(v.params.z);
    return f;
}

@fragment
fn fs_fade(f: FadeOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if dither_threshold(f.clip.xy) < f.hidden {
        discard;
    }
    var v: VertexOut;
    v.clip = f.clip;
    v.world = f.world;
    v.normal = f.normal;
    v.tangent = f.tangent;
    v.uv = f.uv;
    v.uv2 = f.uv2;
    v.color = f.color;
    v.material = f.material;
    v.view_depth = f.view_depth;
    let c = lit(v, front);
    let cutoff = materials[f.material].params.w;
    if cutoff >= 0.0 && c.a < cutoff {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}
