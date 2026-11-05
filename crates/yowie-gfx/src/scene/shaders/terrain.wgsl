// Chunked terrain. Chunks have no vertex buffer: the vertex program
// turns the vertex index into a sample of the chunk's grid at its level
// (or a dropped copy of an edge sample, for the skirt) and reads its
// height from the height buffer. The fragment program blends four tiling
// layers by the splat map, projecting them from the sides on steep
// ground, and lights the result with the shared surface lighting.
// Mirrors `terrain/gpu.rs` and `terrain/render.rs`.

struct TerrainParams {
    // Columns, rows, chunk size, flags (1: triplanar).
    grid: vec4<u32>,
    // World position of sample (0, 0); w is the cell size.
    origin: vec4<f32>,
    layer_scale: vec4<f32>,
    layer_rough: vec4<f32>,
    layer_color: array<vec4<f32>, 4>,
}

@group(1) @binding(32) var<storage, read> terrain_heights: array<f32>;
@group(1) @binding(33) var<uniform> terrain: TerrainParams;
@group(1) @binding(34) var terrain_normals: texture_2d<f32>;
@group(1) @binding(35) var terrain_splat: texture_2d<f32>;
@group(1) @binding(36) var terrain_layer0: texture_2d<f32>;
@group(1) @binding(37) var terrain_layer1: texture_2d<f32>;
@group(1) @binding(38) var terrain_layer2: texture_2d<f32>;
@group(1) @binding(39) var terrain_layer3: texture_2d<f32>;
@group(1) @binding(40) var terrain_repeat: sampler;
@group(1) @binding(41) var terrain_clamp: sampler;

struct ChunkIn {
    @builtin(vertex_index) vertex: u32,
    // First sample x and z, level | shadow view << 8 | flags << 16, skirt
    // depth as float bits.
    @location(0) chunk: vec4<u32>,
}

struct TerrainOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) grid: vec2<f32>,
    @location(2) view_depth: f32,
    @location(3) @interpolate(flat) level: u32,
}

struct TerrainVertex {
    world: vec3<f32>,
    grid: vec2<f32>,
}

fn terrain_vertex(c: ChunkIn) -> TerrainVertex {
    let level = c.chunk.z & 0xffu;
    let step = 1u << level;
    let n = terrain.grid.z / step + 1u;
    var i: u32;
    var j: u32;
    var drop = 0.0;
    if c.vertex < n * n {
        i = c.vertex % n;
        j = c.vertex / n;
    } else {
        let k = c.vertex - n * n;
        let t = k % n;
        switch k / n {
            case 0u: { i = t; j = 0u; }
            case 1u: { i = t; j = n - 1u; }
            case 2u: { i = 0u; j = t; }
            default: { i = n - 1u; j = t; }
        }
        drop = bitcast<f32>(c.chunk.w);
    }
    let sx = min(c.chunk.x + i * step, terrain.grid.x - 1u);
    let sz = min(c.chunk.y + j * step, terrain.grid.y - 1u);
    let h = terrain_heights[sz * terrain.grid.x + sx];
    var v: TerrainVertex;
    let cell = terrain.origin.w;
    v.world = terrain.origin.xyz + vec3<f32>(f32(sx) * cell, h - drop, f32(sz) * cell);
    v.grid = vec2<f32>(f32(sx), f32(sz));
    return v;
}

@vertex
fn vs_terrain(c: ChunkIn) -> TerrainOut {
    let v = terrain_vertex(c);
    var o: TerrainOut;
    o.clip = frame.view_proj * vec4<f32>(v.world, 1.0);
    o.world = v.world;
    o.grid = v.grid;
    o.view_depth = -(frame.view * vec4<f32>(v.world, 1.0)).z;
    o.level = (c.chunk.z & 0xffu) | (((c.chunk.z >> 16u) & 0xffu) << 8u);
    return o;
}

@vertex
fn vs_terrain_shadow(c: ChunkIn) -> @builtin(position) vec4<f32> {
    let v = terrain_vertex(c);
    let view = frame.shadow_views[(c.chunk.z >> 8u) & 0xffu];
    return view.view_proj * vec4<f32>(v.world, 1.0);
}

// Layers are sampled with explicit gradients because which ones are read
// depends on the splat weights, which differ between neighbouring pixels.
fn layer_sample(i: u32, uv: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>) -> vec3<f32> {
    switch i {
        case 0u: { return textureSampleGrad(terrain_layer0, terrain_repeat, uv, gx, gy).rgb; }
        case 1u: { return textureSampleGrad(terrain_layer1, terrain_repeat, uv, gx, gy).rgb; }
        case 2u: { return textureSampleGrad(terrain_layer2, terrain_repeat, uv, gx, gy).rgb; }
        default: { return textureSampleGrad(terrain_layer3, terrain_repeat, uv, gx, gy).rgb; }
    }
}

// One layer's colour at a world point: from above, or blended from the
// three axis projections by `tri` when the ground is steep. A second,
// much larger sample of the same texture varies the shade so the repeat
// does not show from a distance. `dx` and `dy` are the world position's
// screen derivatives.
fn layer_color(i: u32, world: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>, tri: vec3<f32>, steep: bool) -> vec3<f32> {
    let k = terrain.layer_scale[i];
    var c: vec3<f32>;
    if steep {
        c = layer_sample(i, world.zy * k, dx.zy * k, dy.zy * k) * tri.x
            + layer_sample(i, world.xz * k, dx.xz * k, dy.xz * k) * tri.y
            + layer_sample(i, world.xy * k, dx.xy * k, dy.xy * k) * tri.z;
    } else {
        c = layer_sample(i, world.xz * k, dx.xz * k, dy.xz * k);
    }
    let kb = k * 0.11;
    let broad = layer_sample(i, world.xz * kb, dx.xz * kb, dy.xz * kb);
    let variation = 0.75 + 0.5 * dot(broad, vec3<f32>(0.333));
    return c * variation * terrain.layer_color[i].rgb;
}

fn level_tint(level: u32) -> vec3<f32> {
    switch level {
        case 0u: { return vec3<f32>(1.0, 0.25, 0.2); }
        case 1u: { return vec3<f32>(1.0, 0.85, 0.2); }
        case 2u: { return vec3<f32>(0.3, 1.0, 0.3); }
        default: { return vec3<f32>(0.3, 0.5, 1.0); }
    }
}

@fragment
fn fs_terrain(in: TerrainOut) -> @location(0) vec4<f32> {
    let size = vec2<f32>(f32(terrain.grid.x), f32(terrain.grid.y));
    let uv = (in.grid + 0.5) / size;
    let n = normalize(textureSample(terrain_normals, terrain_clamp, uv).xyz * 2.0 - 1.0);
    var w = textureSample(terrain_splat, terrain_clamp, uv);
    let total = w.r + w.g + w.b + w.a;
    if total < 1e-4 {
        w = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    } else {
        w /= total;
    }
    var tri = pow(abs(n), vec3<f32>(4.0));
    tri /= tri.x + tri.y + tri.z;
    let steep = (terrain.grid.w & 1u) != 0u && tri.y < 0.97;
    let dx = dpdx(in.world);
    let dy = dpdy(in.world);
    var base = vec3<f32>(0.0);
    var rough = 0.0;
    for (var i = 0u; i < 4u; i++) {
        if w[i] > 0.002 {
            base += layer_color(i, in.world, dx, dy, tri, steep) * w[i];
            rough += terrain.layer_rough[i] * w[i];
        }
    }
    if (in.level >> 8u) != 0u {
        base = mix(base, level_tint(in.level & 0xffu), 0.55);
    }
    let s = plain_surface(base, n, rough, in.world);
    let c = apply_fog(shade(s, in.clip, in.view_depth), in.world);
    return vec4<f32>(c, 1.0);
}
