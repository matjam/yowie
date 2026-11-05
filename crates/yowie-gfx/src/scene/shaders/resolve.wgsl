// The resolve from the HDR scene to the output: exposure, a tone curve
// (ACES or AgX) and composition over the background colour. The output
// format is sRGB, so the program writes linear light.

struct Resolve {
    // Premultiplied linear background drawn behind the scene.
    background: vec4<f32>,
    // Exposure, tone curve (0 none, 1 ACES, 2 AgX), 0, 0.
    params: vec4<f32>,
}

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var hdr_sampler: sampler;
@group(0) @binding(2) var<uniform> resolve: Resolve;

struct ResolveOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_resolve(@builtin(vertex_index) i: u32) -> ResolveOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: ResolveOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

fn aces(c: vec3<f32>) -> vec3<f32> {
    let m_in = mat3x3<f32>(
        0.59719, 0.07600, 0.02840,
        0.35458, 0.90834, 0.13383,
        0.04823, 0.01566, 0.83777);
    let m_out = mat3x3<f32>(
        1.60475, -0.10208, -0.00327,
        -0.53108, 1.10813, -0.07276,
        -0.07367, -0.00605, 1.07602);
    let v = m_in * c;
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return clamp(m_out * (a / b), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn agx_contrast(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

fn agx(c: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3<f32>(
        0.842479062253094, 0.0423282422610123, 0.0423756549057051,
        0.0784335999999992, 0.878468636469772, 0.0784336,
        0.0792237451477643, 0.0791661274605434, 0.879142973793104);
    let outset = mat3x3<f32>(
        1.19687900512017, -0.0528968517574562, -0.0529716355144438,
        -0.0980208811401368, 1.15190312990417, -0.0980434501171241,
        -0.0990297440797205, -0.0989611768448433, 1.15107367264116);
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = inset * max(c, vec3<f32>(1e-10));
    v = clamp(log2(v), vec3<f32>(min_ev), vec3<f32>(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    v = outset * agx_contrast(v);
    // The curve's output is display-encoded; return linear light.
    return pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let e = c * resolve.params.x;
    let mode = u32(resolve.params.y);
    if mode == 1u {
        return aces(e);
    }
    if mode == 2u {
        return agx(e);
    }
    return clamp(e, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_resolve(in: ResolveOut) -> @location(0) vec4<f32> {
    let s = textureSampleLevel(hdr, hdr_sampler, in.uv, 0.0);
    let a = clamp(s.a, 0.0, 1.0);
    let rgb = tonemap(s.rgb / max(a, 1e-4)) * a;
    let bg = resolve.background;
    return vec4<f32>(rgb + bg.rgb * (1.0 - a), a + bg.a * (1.0 - a));
}
