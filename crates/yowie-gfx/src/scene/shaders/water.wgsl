// Water: one quad at the water level. The depth under each pixel comes
// from the terrain's height buffer; it picks the colour between shallow
// and deep and how much of the ground shows through. A sum of moving
// waves tilts the normal, so the sky and sun reflect off a moving
// surface; reflection strengthens towards grazing angles. Mirrors
// `water.rs`.

struct WaterParams {
    // x0, z0, x1, z1.
    area: vec4<f32>,
    // Level, time, depth range, floor depth.
    level: vec4<f32>,
    // Linear shallow colour; w is the opacity.
    shallow: vec4<f32>,
    // Linear deep colour; w is the roughness.
    deep: vec4<f32>,
    // Strength, one over the wave length, speed, foam width.
    waves: vec4<f32>,
    // Terrain columns, rows, 1 when there is a terrain.
    grid: vec4<u32>,
    // Terrain origin; w is the cell size.
    origin: vec4<f32>,
}

@group(1) @binding(32) var<uniform> water: WaterParams;
@group(1) @binding(33) var<storage, read> water_heights: array<f32>;

struct WaterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
}

@vertex
fn vs_water(@builtin(vertex_index) i: u32) -> WaterOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[min(i, 5u)];
    let xz = mix(water.area.xy, water.area.zw, c);
    var o: WaterOut;
    o.world = vec3<f32>(xz.x, water.level.x, xz.y);
    o.clip = frame.view_proj * vec4<f32>(o.world, 1.0);
    return o;
}

fn water_sample(x: u32, z: u32) -> f32 {
    let cx = min(x, water.grid.x - 1u);
    let cz = min(z, water.grid.y - 1u);
    return water_heights[cz * water.grid.x + cx];
}

// The terrain height under a world point, blended from the four samples
// around it.
fn ground_height(p: vec2<f32>) -> f32 {
    let size = vec2<f32>(f32(water.grid.x - 1u), f32(water.grid.y - 1u));
    let g = clamp((p - water.origin.xz) / water.origin.w, vec2<f32>(0.0), size);
    let i = min(vec2<u32>(g), water.grid.xy - 2u);
    let f = g - vec2<f32>(i);
    let a = mix(water_sample(i.x, i.y), water_sample(i.x + 1u, i.y), f.x);
    let b = mix(water_sample(i.x, i.y + 1u), water_sample(i.x + 1u, i.y + 1u), f.x);
    return water.origin.y + mix(a, b, f.y);
}

// How much of a wave of angular frequency freq a pixel covering
// `footprint` world units shows: waves shorter than about two pixels
// fade out instead of aliasing into stripes.
fn wave_filter(freq: f32, footprint: f32) -> f32 {
    return clamp(1.5 - footprint * freq * 0.5, 0.0, 1.0);
}

// The slope of a sum of travelling waves at p: four long ones in
// different directions and four short ones for ripples, each faded out
// where the pixel's footprint is too coarse to show it.
fn wave_slope(p: vec2<f32>, t: f32, footprint: f32) -> vec2<f32> {
    let k = water.waves.y * 6.2831853;
    let speed = water.waves.z;
    var slope = vec2<f32>(0.0);
    var dirs = array<vec2<f32>, 4>(
        vec2<f32>(0.8, 0.6), vec2<f32>(-0.6, 0.8), vec2<f32>(0.2, -0.98), vec2<f32>(-0.9, -0.44),
    );
    for (var i = 0u; i < 4u; i++) {
        let d = dirs[i];
        let fi = f32(i);
        let freq = k * (1.0 + fi * 0.37);
        let amp = 1.0 / (1.0 + fi * 0.6);
        let phase = dot(d, p) * freq + t * speed * freq * (1.0 + 0.1 * fi);
        slope += d * cos(phase) * amp * wave_filter(freq, footprint);
        let rd = vec2<f32>(d.y, -d.x);
        let rfreq = freq * 4.3;
        let rphase = dot(rd, p) * rfreq + t * speed * rfreq * 0.7 + fi;
        slope += rd * cos(rphase) * amp * 0.35 * wave_filter(rfreq, footprint);
    }
    return slope * 0.25;
}

// The lit water colour (linear, straight) and its coverage.
fn water_color(in: WaterOut) -> vec4<f32> {
    let fw = fwidth(in.world.xz);
    let footprint = max(fw.x, fw.y);
    var depth = water.level.w;
    if water.grid.z != 0u {
        depth = water.level.x - ground_height(in.world.xz);
    }
    if depth <= 0.0 {
        discard;
    }
    let t = clamp(depth / water.level.z, 0.0, 1.0);
    let slope = wave_slope(in.world.xz, water.level.y, footprint) * water.waves.x * (0.3 + 0.7 * t);
    let n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    var body = mix(water.shallow.rgb, water.deep.rgb, sqrt(t));
    var alpha = mix(0.08, water.shallow.w, t);
    var roughness = water.deep.w;
    if water.waves.w > 0.0 {
        let edge = clamp(1.0 - depth / water.waves.w, 0.0, 1.0);
        let ripple = 0.6 + 0.4 * sin(depth * 25.0 - water.level.y * 2.5 + in.world.x * 0.7);
        let foam = edge * edge * ripple;
        body = mix(body, vec3<f32>(0.85, 0.9, 0.92), foam);
        alpha = max(alpha, foam);
        roughness = mix(roughness, 0.8, foam);
    }
    let s = plain_surface(body, n, roughness, in.world);
    var c = shade(s, in.clip, view_depth_of(in.world));
    let nv = max(dot(n, s.view), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - nv, 5.0);
    var a = clamp(alpha + fresnel * (1.0 - alpha), 0.0, 1.0);
    a *= smoothstep(0.0, 0.04, depth);
    c = apply_fog(c, in.world);
    return vec4<f32>(c, a);
}

@fragment
fn fs_water(in: WaterOut) -> @location(0) vec4<f32> {
    let c = water_color(in);
    return vec4<f32>(c.rgb * c.a, c.a);
}

@fragment
fn fs_water_oit(in: WaterOut) -> OitOut {
    let c = water_color(in);
    return oit_out(c.rgb, c.a, view_depth_of(in.world));
}
