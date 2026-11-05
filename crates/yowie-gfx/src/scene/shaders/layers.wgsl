// The layered material features (clearcoat, sheen, subsurface,
// transmission, specular tint, iridescence, anisotropy, fur): reading them
// from the material. Mirrors `layers.rs`; `surface.wgsl` calls
// `finish_surface` from `surface`, and `lobes.wgsl` lights the features.

@group(1) @binding(10) var t_clearcoat: texture_2d<f32>;
@group(1) @binding(11) var t_clearcoat_normal: texture_2d<f32>;
@group(1) @binding(12) var t_sheen: texture_2d<f32>;
@group(1) @binding(13) var t_specular: texture_2d<f32>;
@group(1) @binding(14) var t_transmission: texture_2d<f32>;
@group(1) @binding(15) var t_thickness: texture_2d<f32>;
@group(1) @binding(16) var t_iridescence: texture_2d<f32>;
@group(1) @binding(17) var t_anisotropy: texture_2d<f32>;
@group(1) @binding(18) var t_fur: texture_2d<f32>;
@group(1) @binding(19) var s_layers: sampler;

// The opaque scene, copied with blurred mips before transmissive draws.
@group(0) @binding(23) var scene_copy: texture_2d<f32>;

// Which layered feature textures a material has, in `flags.w`.
const LT_CLEARCOAT: u32 = 1u;
const LT_CLEARCOAT_NORMAL: u32 = 2u;
const LT_SHEEN: u32 = 4u;
const LT_SPECULAR: u32 = 8u;
const LT_TRANSMISSION: u32 = 16u;
const LT_THICKNESS: u32 = 32u;
const LT_IRIDESCENCE: u32 = 64u;
const LT_ANISOTROPY: u32 = 128u;
const LT_FUR: u32 = 256u;

// The layered features of one fragment. A surface shader may change any
// of them; zero strengths switch a feature off.
struct Layers {
    clearcoat: f32,
    clearcoat_roughness: f32,
    // The coat's normal: the mesh normal or the coat normal map.
    clearcoat_normal: vec3<f32>,
    // Linear sheen colour.
    sheen: vec3<f32>,
    sheen_roughness: f32,
    subsurface: f32,
    // 0 thin to 1 thick; thick parts let no subsurface light through.
    thickness: f32,
    transmission: f32,
    ior: f32,
    // How far transmitted light travels inside, in world units.
    volume: f32,
    // Linear colour white light takes after `attenuation_distance`.
    attenuation: vec3<f32>,
    attenuation_distance: f32,
    specular: f32,
    specular_color: vec3<f32>,
    iridescence: f32,
    iridescence_ior: f32,
    // In nanometres.
    iridescence_thickness: f32,
    anisotropy: f32,
    // Unit direction the highlight stretches along, in world space.
    anisotropy_dir: vec3<f32>,
}

fn no_layers(n: vec3<f32>) -> Layers {
    var l: Layers;
    l.clearcoat_normal = n;
    l.ior = 1.5;
    l.attenuation = vec3<f32>(1.0);
    l.specular = 1.0;
    l.specular_color = vec3<f32>(1.0);
    l.iridescence_ior = 1.3;
    return l;
}

// The scene's time in seconds.
fn scene_time() -> f32 {
    return frame.camera.w;
}

fn sqr(x: f32) -> f32 {
    return x * x;
}

fn max3(v: vec3<f32>) -> f32 {
    return max(v.x, max(v.y, v.z));
}

// The material's texture transform applied to `uv`.
fn transform_uv(m: Material, uv: vec2<f32>) -> vec2<f32> {
    let p = vec3<f32>(uv, 1.0);
    return vec2<f32>(dot(m.uv_row0.xyz, p), dot(m.uv_row1.xyz, p));
}

fn layer_texel(t: texture_2d<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    return textureSampleGrad(t, s_layers, uv, dx, dy);
}

// Reads the material's layered features. `uv` is the transformed first
// coordinate set with its screen derivatives; `ng`, `t` and `b` are the
// mesh normal, tangent and bitangent.
fn read_layers(m: Material, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, ng: vec3<f32>, t: vec3<f32>, b: vec3<f32>) -> Layers {
    var l = no_layers(ng);
    let ext = m.flags.z;
    let tex = m.flags.w;
    if (ext & EXT_CLEARCOAT) != 0u {
        var k = vec2<f32>(1.0);
        if (tex & LT_CLEARCOAT) != 0u {
            k = layer_texel(t_clearcoat, uv, dx, dy).rg;
        }
        l.clearcoat = m.ext[0].x * k.x;
        l.clearcoat_roughness = clamp(m.ext[0].y * k.y, 0.03, 1.0);
        if (tex & LT_CLEARCOAT_NORMAL) != 0u {
            let c = layer_texel(t_clearcoat_normal, uv, dx, dy).xyz * 2.0 - 1.0;
            let s = m.ext[0].z;
            l.clearcoat_normal = normalize(t * (c.x * s) + b * (c.y * s) + ng * c.z);
        }
    }
    if (ext & EXT_SHEEN) != 0u {
        var c = vec4<f32>(1.0);
        if (tex & LT_SHEEN) != 0u {
            c = layer_texel(t_sheen, uv, dx, dy);
        }
        l.sheen = m.ext[1].xyz * c.rgb;
        l.sheen_roughness = clamp(m.ext[0].w * c.a, 0.07, 1.0);
    }
    var thick = 1.0;
    if (tex & LT_THICKNESS) != 0u {
        thick = layer_texel(t_thickness, uv, dx, dy).g;
    }
    if (ext & EXT_SUBSURFACE) != 0u {
        l.subsurface = m.ext[1].w;
        l.thickness = select(0.0, thick, (tex & LT_THICKNESS) != 0u);
    }
    if (ext & (EXT_SPECULAR | EXT_TRANSMISSION)) != 0u {
        l.ior = max(m.ext[2].y, 1.0);
    }
    if (ext & EXT_TRANSMISSION) != 0u {
        var k = 1.0;
        if (tex & LT_TRANSMISSION) != 0u {
            k = layer_texel(t_transmission, uv, dx, dy).r;
        }
        l.transmission = m.ext[2].x * k;
        l.volume = m.ext[2].z * thick;
        l.attenuation = m.ext[3].xyz;
        l.attenuation_distance = m.ext[2].w;
    }
    if (ext & EXT_SPECULAR) != 0u {
        var c = vec4<f32>(1.0);
        if (tex & LT_SPECULAR) != 0u {
            c = layer_texel(t_specular, uv, dx, dy);
        }
        l.specular = m.ext[3].w * c.a;
        l.specular_color = m.ext[4].xyz * c.rgb;
    }
    if (ext & EXT_IRIDESCENCE) != 0u {
        var c = vec2<f32>(1.0);
        if (tex & LT_IRIDESCENCE) != 0u {
            c = layer_texel(t_iridescence, uv, dx, dy).rg;
        }
        l.iridescence = m.ext[4].w * c.x;
        l.iridescence_ior = m.ext[5].x;
        l.iridescence_thickness = mix(m.ext[5].y, m.ext[5].z, c.y);
    }
    if (ext & EXT_ANISOTROPY) != 0u {
        var dir = vec2<f32>(1.0, 0.0);
        var k = 1.0;
        if (tex & LT_ANISOTROPY) != 0u {
            let a = layer_texel(t_anisotropy, uv, dx, dy);
            dir = a.rg * 2.0 - 1.0;
            k = a.b;
        }
        let cs = cos(m.ext[6].x);
        let sn = sin(m.ext[6].x);
        let d = vec2<f32>(dir.x * cs - dir.y * sn, dir.x * sn + dir.y * cs);
        let world = t * d.x + b * d.y;
        if dot(world, world) > 1e-8 {
            l.anisotropy = m.ext[5].w * k;
            l.anisotropy_dir = normalize(world);
        }
    }
    return l;
}

// Fills the fields `surface` leaves to the features: texture coordinates,
// vertex colour, the mesh normal and tangent, the layered features, and
// the decals over the surface.
fn finish_surface(s: ptr<function, Surface>, in: VertexOut, geometric: vec3<f32>, tangent: vec3<f32>) {
    let m = materials[in.material];
    let uv = transform_uv(m, in.uv);
    // Derivatives are taken here, in uniform control flow, for every
    // branch below.
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let dwx = dpdx(in.world);
    let dwy = dpdy(in.world);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    (*s).uv = in.uv;
    (*s).uv2 = in.uv2;
    (*s).color = in.color;
    (*s).geometric = geometric;
    var t = tangent - geometric * dot(geometric, tangent);
    if dot(t, t) < 1e-10 {
        // No tangents on the mesh: derive one from the texture coordinates.
        t = cross(dwy, geometric) * duv1.x + cross(geometric, dwx) * duv2.x;
        t -= geometric * dot(geometric, t);
        if dot(t, t) < 1e-12 {
            t = select(vec3<f32>(1.0, 0.0, 0.0), cross(vec3<f32>(0.0, 1.0, 0.0), geometric), abs(geometric.y) < 0.99);
        }
    }
    t = normalize(t);
    let b = cross(geometric, t) * select(1.0, -1.0, in.tangent.w < 0.0);
    (*s).tangent = t;
    (*s).layers = read_layers(m, uv, dx, dy, geometric, t, b);
    if (m.flags.x & MAT_DECALS) != 0u && frame.counts.z > 0u {
        apply_decals(s, dwx, dwy);
    }
}

// The strand mask of a fur shell: the fur texture's red, or a hash of a
// 64 by 64 grid without one.
fn fur_mask(m: Material, uv: vec2<f32>) -> f32 {
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    if (m.flags.w & LT_FUR) != 0u {
        return layer_texel(t_fur, uv, dx, dy).r;
    }
    let cell = floor(uv * 64.0);
    return fract(sin(dot(cell, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
