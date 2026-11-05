// Projected decals: boxes that colour, roughen and bump the surfaces
// inside them before lighting. Every decal of the scene is in one storage
// array, and each lit fragment of a material that receives decals tests
// itself against them. Mirrors `decals.rs`.

struct Decal {
    // World to box space: the box is the cube from -0.5 to 0.5, projected
    // down its Y axis with X and Z spanning the image.
    inv0: vec4<f32>,
    inv1: vec4<f32>,
    inv2: vec4<f32>,
    // The colour image's cell in the colour atlas: offset and size.
    color_rect: vec4<f32>,
    // The normal image's cell in the normal atlas; zero size for none.
    normal_rect: vec4<f32>,
    // Linear tint; alpha scales the decal's coverage.
    tint: vec4<f32>,
    // Roughness, how much of it replaces the surface's, normal strength,
    // unused.
    params: vec4<f32>,
}

@group(0) @binding(21) var<storage, read> decals: array<Decal>;
@group(0) @binding(22) var decal_color: texture_2d<f32>;
@group(0) @binding(23) var decal_normal: texture_2d<f32>;

// How far a decal's coordinates stay from its atlas cell's edge, so the
// lower mips do not blend in the neighbouring cell.
const DECAL_INSET: f32 = 0.03;

// Applies every decal whose box holds the surface point. `dwx` and `dwy`
// are the screen derivatives of the world position.
fn apply_decals(s: ptr<function, Surface>, dwx: vec3<f32>, dwy: vec3<f32>) {
    let count = min(frame.counts.z, arrayLength(&decals));
    let p = vec4<f32>((*s).world, 1.0);
    for (var i = 0u; i < count; i++) {
        let d = decals[i];
        let local = vec3<f32>(dot(d.inv0, p), dot(d.inv1, p), dot(d.inv2, p));
        if any(abs(local) > vec3<f32>(0.5)) {
            continue;
        }
        let up = normalize(d.inv1.xyz);
        let fade = smoothstep(0.15, 0.45, dot((*s).geometric, up));
        if fade <= 0.0 {
            continue;
        }
        let uv = clamp(vec2<f32>(local.x + 0.5, 0.5 - local.z), vec2<f32>(DECAL_INSET), vec2<f32>(1.0 - DECAL_INSET));
        let gx = vec2<f32>(dot(d.inv0.xyz, dwx), -dot(d.inv2.xyz, dwx));
        let gy = vec2<f32>(dot(d.inv0.xyz, dwy), -dot(d.inv2.xyz, dwy));
        let cs = d.color_rect.zw;
        let c = textureSampleGrad(decal_color, env_sampler, d.color_rect.xy + uv * cs, gx * cs, gy * cs);
        let a = clamp(c.a * d.tint.a * fade, 0.0, 1.0);
        if a <= 0.0 {
            continue;
        }
        (*s).base = mix((*s).base, c.rgb / max(c.a, 1e-4) * d.tint.rgb, a);
        (*s).roughness = mix((*s).roughness, d.params.x, a * d.params.y);
        if d.normal_rect.z > 0.0 {
            let ns = d.normal_rect.zw;
            let t = textureSampleGrad(decal_normal, env_sampler, d.normal_rect.xy + uv * ns, gx * ns, gy * ns).xyz * 2.0 - 1.0;
            let n = (*s).normal;
            let tx = normalize(d.inv0.xyz);
            let tz = normalize(d.inv2.xyz);
            let tangent = normalize(tx - n * dot(n, tx));
            let bitangent = normalize(tz - n * dot(n, tz));
            let k = d.params.z;
            let bumped = normalize(tangent * (t.x * k) + bitangent * (t.y * k) + n * t.z);
            (*s).normal = normalize(mix(n, bumped, a));
        }
    }
}
