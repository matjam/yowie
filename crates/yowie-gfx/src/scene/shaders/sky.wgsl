// The background behind the scene: the procedural sky or the
// environment's radiance, drawn on a full-screen triangle at the far
// plane after the opaque meshes, so only uncovered pixels run it.

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: SkyOut;
    // Reversed depth: 0 is the far plane.
    o.clip = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

fn view_dir(ndc: vec2<f32>) -> vec3<f32> {
    let near = frame.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let far = frame.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    return normalize(far.xyz / far.w - near.xyz / near.w);
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let d = view_dir(in.ndc);
    if frame.env.w > 1.5 {
        let c = textureSampleLevel(env_specular, env_sampler, d, 0.0).rgb * frame.env.x;
        return vec4<f32>(c, 1.0);
    }
    return vec4<f32>(sky_with_sun(d), 1.0);
}

// Debug lines: world-space segments with a linear colour.

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var o: LineOut;
    o.clip = frame.view_proj * vec4<f32>(v.position, 1.0);
    o.color = v.color;
    return o;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}
