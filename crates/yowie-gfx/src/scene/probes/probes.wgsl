// Reflection probes and the light probe grid: the indirect light
// `shade_ambient` in surface.wgsl reads before falling back to the
// environment, the sky or the flat ambient colour. Mirrors `ProbeBlock` in
// probes/mod.rs; `probe_sample` and binding 21 come from probe_array.wgsl
// or probe_single.wgsl.

struct ProbeGpu {
    // Capture position; w is the intensity.
    center: vec4<f32>,
    // Influence box corner; w is the blend distance.
    box_min: vec4<f32>,
    box_max: vec4<f32>,
    // Layer in the probe cube array, 1 when box projected, 0, 0.
    params: vec4<f32>,
}

struct Probes {
    // Probe count, specular mip count, 0, 0.
    info: vec4<f32>,
    // Position of cell (0, 0, 0); w is the intensity, 0 when there is no
    // grid.
    grid_origin: vec4<f32>,
    // Distance between cells.
    grid_spacing: vec4<f32>,
    // Cells along x, y and z.
    grid_counts: vec4<f32>,
    list: array<ProbeGpu, 8>,
}

@group(0) @binding(20) var<uniform> probes: Probes;
@group(0) @binding(22) var grid_sh: texture_3d<f32>;

// How much probe p covers `pos`: 1 inside its box and on its faces (a
// room's walls and floor), falling to 0 over the blend distance outside.
fn probe_weight(p: ProbeGpu, pos: vec3<f32>) -> f32 {
    // Per axis at most one of the two terms is above zero.
    let out = max(p.box_min.xyz - pos, vec3<f32>(0.0)) + max(pos - p.box_max.xyz, vec3<f32>(0.0));
    let outside = length(out);
    if p.box_min.w <= 0.0 {
        return select(0.0, 1.0, outside <= 0.0);
    }
    return clamp(1.0 - outside / p.box_min.w, 0.0, 1.0);
}

// The direction to look up in probe p's cube for light arriving at `pos`
// along -r. Box projection intersects r with the box and looks from the
// capture point towards that hit, so walls reflect where they stand
// (parallax correction); a point in the blend band outside the box
// projects from the nearest point inside. Captures store directions with z
// mirrored (see probes/capture.rs).
fn probe_dir(p: ProbeGpu, pos: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
    var d = r;
    if p.params.y > 0.5 {
        let q = clamp(pos, p.box_min.xyz, p.box_max.xyz);
        // An axis the ray runs along gets a tiny step, so its exit
        // distance is huge and never the nearest.
        let rr = select(r, vec3<f32>(1e-6), abs(r) < vec3<f32>(1e-6));
        let inv = 1.0 / rr;
        let t1 = (p.box_max.xyz - q) * inv;
        let t2 = (p.box_min.xyz - q) * inv;
        let tmax = max(t1, t2);
        let t = min(min(tmax.x, tmax.y), tmax.z);
        d = q + r * max(t, 0.0) - p.center.xyz;
    }
    return vec3<f32>(d.x, d.y, -d.z);
}

// Specular light arriving at `pos` along -r for a surface of `roughness`
// (0 to 1): the probes covering `pos`, smaller ones first, then the
// environment, sky or flat ambient light for what they leave uncovered.
fn ambient_specular(pos: vec3<f32>, r: vec3<f32>, roughness: f32) -> vec3<f32> {
    let count = u32(probes.info.x);
    let lod = roughness * (probes.info.y - 1.0);
    var c = vec3<f32>(0.0);
    var covered = 0.0;
    for (var i = 0u; i < count; i++) {
        let p = probes.list[i];
        let w = probe_weight(p, pos) * (1.0 - covered);
        if w > 0.0 {
            c += probe_sample(u32(p.params.x), probe_dir(p, pos, r), lod) * (p.center.w * w);
            covered += w;
            if covered > 0.999 {
                return c;
            }
        }
    }
    var env = frame.ambient.rgb;
    if frame.env.z > 0.5 {
        let env_lod = roughness * (frame.env.y - 1.0);
        env = textureSampleLevel(env_specular, env_sampler, r, env_lod).rgb * frame.env.x;
    }
    return c + env * (1.0 - covered);
}

// The grid's irradiance over pi towards normal n at `pos` in rgb (what a
// white surface reflects), and in a how much the grid covers `pos`: 1
// inside, falling to 0 over the half cell past the outer cells. The cells
// are filtered trilinearly; each holds nine spherical harmonic
// coefficients (L2) already convolved with the cosine lobe, packed in
// seven slabs of the texture's depth.
fn grid_irradiance(pos: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    let o = probes.grid_origin;
    if o.w <= 0.0 {
        return vec4<f32>(0.0);
    }
    let counts = probes.grid_counts.xyz;
    let g = (pos - o.xyz) / max(probes.grid_spacing.xyz, vec3<f32>(1e-4));
    let last = counts - 1.0;
    let edge = min(g + 0.5, last + 0.5 - g);
    let cover = clamp(min(min(edge.x, edge.y), edge.z) * 2.0, 0.0, 1.0);
    if cover <= 0.0 {
        return vec4<f32>(0.0);
    }
    let c = clamp(g, vec3<f32>(0.0), last);
    let xy = (c.xy + 0.5) / counts.xy;
    let depth = 7.0 * counts.z;
    var s: array<vec4<f32>, 7>;
    for (var k = 0u; k < 7u; k++) {
        // Cell centres only, so filtering never mixes two slabs.
        let z = (f32(k) * counts.z + c.z + 0.5) / depth;
        s[k] = textureSampleLevel(grid_sh, env_sampler, vec3<f32>(xy, z), 0.0);
    }
    let c0 = s[0].xyz;
    let c1 = vec3<f32>(s[0].w, s[1].xy);
    let c2 = vec3<f32>(s[1].zw, s[2].x);
    let c3 = s[2].yzw;
    let c4 = s[3].xyz;
    let c5 = vec3<f32>(s[3].w, s[4].xy);
    let c6 = vec3<f32>(s[4].zw, s[5].x);
    let c7 = s[5].yzw;
    let c8 = s[6].xyz;
    let e = c0 * 0.282095
        + c1 * (0.488603 * n.y)
        + c2 * (0.488603 * n.z)
        + c3 * (0.488603 * n.x)
        + c4 * (1.092548 * n.x * n.y)
        + c5 * (1.092548 * n.y * n.z)
        + c6 * (0.315392 * (3.0 * n.z * n.z - 1.0))
        + c7 * (1.092548 * n.x * n.z)
        + c8 * (0.546274 * (n.x * n.x - n.y * n.y));
    return vec4<f32>(max(e, vec3<f32>(0.0)) * o.w, cover);
}

// Diffuse light reaching `pos` with normal n, over pi: the light probe
// grid where it covers `pos`, blended into the environment, sky or flat
// ambient light.
fn ambient_diffuse(pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var env = frame.ambient.rgb;
    if frame.env.z > 0.5 {
        env = textureSampleLevel(env_irradiance, env_sampler, n, 0.0).rgb * frame.env.x;
    }
    let g = grid_irradiance(pos, n);
    return mix(env, g.rgb, g.a);
}
