// Lit 2D sprites: ambient light plus up to 16 point and spot lights, each
// optionally blocked by a polar shadow map built on the CPU, and an
// optional tangent-space normal map.
//
// Group 0 is the albedo texture, group 1 the view block, group 2 the
// lights and shadow strip, group 3 the normal map. The vertex program
// matches the sprite prelude.

struct YowieView {
    view_proj: mat4x4<f32>,
    view_size: vec2<f32>,
    pixel_scale: f32,
    time: f32,
};

// pos: xy position, z height above the sprite plane, w radius.
// color: linear rgb already multiplied by intensity, w falloff exponent.
// spot: xy unit direction, z cosine of the outer half angle, w cosine of
//       the inner half angle; z above 1 means a point light.
// shadow: x 1 when the light casts shadows, y its row in the strip,
//         z the soft edge width in view units.
struct Light {
    pos: vec4<f32>,
    color: vec4<f32>,
    spot: vec4<f32>,
    shadow: vec4<f32>,
};

struct Lights {
    ambient: vec4<f32>, // linear rgb, w light count
    lights: array<Light, 16>,
};

@group(0) @binding(0) var albedo_tex: texture_2d<f32>;
@group(0) @binding(1) var albedo_sampler: sampler;
@group(1) @binding(0) var<uniform> view: YowieView;
@group(2) @binding(0) var<uniform> lights: Lights;
@group(2) @binding(1) var shadow_strip: texture_2d<f32>;
@group(3) @binding(0) var normal_tex: texture_2d<f32>;
@group(3) @binding(1) var normal_sampler: sampler;

const TAU_INV: f32 = 0.15915494;
const STRIP_WIDTH: i32 = 512;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec2<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = view.view_proj * vec4<f32>(v.position, 0.0, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    out.world = v.position;
    return out;
}

// The fraction of the light's rays to this point that no occluder cuts.
// The strip row holds, for 512 directions around the light, the distance
// to the nearest occluder edge. Five taps across the soft edge average
// the comparison.
fn visible(row: f32, softness: f32, to_point: vec2<f32>, dist: f32) -> f32 {
    let u = atan2(to_point.y, to_point.x) * TAU_INV + 0.5;
    let half_width = max(softness * 0.5 / max(dist, 1.0) * TAU_INV, 0.5 / f32(STRIP_WIDTH));
    var lit = 0.0;
    for (var t = -2; t <= 2; t++) {
        let a = fract(u + f32(t) * half_width * 0.5);
        let x = min(i32(a * f32(STRIP_WIDTH)), STRIP_WIDTH - 1);
        let stored = textureLoad(shadow_strip, vec2<i32>(x, i32(row)), 0).r;
        lit += step(dist, stored);
    }
    return lit * 0.2;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    let albedo = textureSample(albedo_tex, albedo_sampler, in.uv) * in.color;
    var n = textureSample(normal_tex, normal_sampler, in.uv).xyz * 2.0 - vec3<f32>(1.0);
    // Normal maps are Y up; drawing coordinates are Y down.
    n.y = -n.y;
    n = normalize(n);
    var light = lights.ambient.rgb;
    let count = i32(lights.ambient.w);
    for (var i = 0; i < count; i++) {
        let l = lights.lights[i];
        let to_light = l.pos.xy - in.world;
        let d3 = vec3<f32>(to_light, l.pos.z);
        let dist3 = max(length(d3), 1e-3);
        let flat_dist = length(to_light);
        let radius = max(l.pos.w, 1e-3);
        let att = pow(clamp(1.0 - flat_dist / radius, 0.0, 1.0), l.color.w);
        var k = max(dot(n, d3 / dist3), 0.0) * att;
        if (l.spot.z <= 1.0) {
            let c = dot(-normalize(to_light + vec2<f32>(1e-5, 0.0)), l.spot.xy);
            k *= smoothstep(l.spot.z, max(l.spot.w, l.spot.z + 1e-4), c);
        }
        if (l.shadow.x > 0.5 && k > 0.0) {
            k *= visible(l.shadow.y, l.shadow.z, -to_light, flat_dist);
        }
        light += l.color.rgb * k;
    }
    return vec4<f32>(albedo.rgb * light, albedo.a);
}
