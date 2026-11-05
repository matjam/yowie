// The layered material features (clearcoat, sheen, subsurface,
// transmission, specular tint, iridescence, anisotropy): reading them from
// the material and their light. Mirrors `layers.rs`; `surface.wgsl` calls
// in here from `surface`, `shade_light` and `shade_ambient`.

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
@group(0) @binding(20) var scene_copy: texture_2d<f32>;

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
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let dwx = dpdx(in.world);
    let dwy = dpdy(in.world);
    (*s).uv = in.uv;
    (*s).uv2 = in.uv2;
    (*s).color = in.color;
    (*s).geometric = geometric;
    var t = tangent - geometric * dot(geometric, tangent);
    if dot(t, t) < 1e-10 {
        // No tangents on the mesh: derive one from the texture coordinates.
        let duv1 = dpdx(in.uv);
        let duv2 = dpdy(in.uv);
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

// A dielectric's reflectance at normal incidence.
fn ior_f0(ior: f32) -> f32 {
    if ior == 1.5 {
        return 0.04;
    }
    return sqr((ior - 1.0) / (ior + 1.0));
}

// Reflectance at normal incidence: the dielectric's, scaled and tinted by
// the specular layer, or the base colour for a metal.
fn base_f0(s: Surface) -> vec3<f32> {
    let d = min(vec3<f32>(ior_f0(s.layers.ior)) * s.layers.specular_color, vec3<f32>(1.0)) * s.layers.specular;
    return mix(d, s.base, s.metallic);
}

// Reflectance at grazing incidence.
fn base_f90(s: Surface) -> f32 {
    return mix(s.layers.specular, 1.0, s.metallic);
}

// The reflectance of a thin film of index `film_ior`, `thickness`
// nanometres thick, over a base of reflectance `f0`, at the angle whose
// cosine is `cos1`: the two reflections interfere, evaluated at three
// wavelengths for red, green and blue.
fn thin_film(cos1: f32, thickness: f32, film_ior: f32, f0: vec3<f32>) -> vec3<f32> {
    let n2 = max(film_ior, 1.0001);
    let sin2 = (1.0 - cos1 * cos1) / (n2 * n2);
    let cos2 = sqrt(max(1.0 - sin2, 0.0));
    let r12 = sqr((n2 - 1.0) / (n2 + 1.0));
    let rf12 = r12 + (1.0 - r12) * pow(1.0 - cos1, 5.0);
    let sf0 = sqrt(clamp(f0, vec3<f32>(0.0), vec3<f32>(0.9999)));
    let n3 = (1.0 + sf0) / (1.0 - sf0);
    let q = (n3 - n2) / (n3 + n2);
    let r23 = q * q;
    let rf23 = r23 + (1.0 - r23) * pow(1.0 - cos2, 5.0);
    let phase = 4.0 * PI * n2 * thickness * cos2 / vec3<f32>(650.0, 532.0, 450.0);
    // Both reflections turn the phase when the base is denser than the
    // film, so their cross term keeps its sign; otherwise it flips.
    let sign = select(vec3<f32>(-1.0), vec3<f32>(1.0), n3 > vec3<f32>(n2));
    let cross_term = 2.0 * sqrt(rf12 * rf23) * cos(phase) * sign;
    return clamp((rf12 + rf23 + cross_term) / (1.0 + rf12 * rf23 + cross_term), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Fresnel reflectance at the angle whose cosine is `cos_theta`, with the
// specular layer's grazing reflectance and the iridescent film.
fn layer_fresnel(s: Surface, f0: vec3<f32>, cos_theta: f32) -> vec3<f32> {
    let f = f0 + (vec3<f32>(base_f90(s)) - f0) * pow(1.0 - cos_theta, 5.0);
    if s.layers.iridescence <= 0.0 {
        return f;
    }
    let film = thin_film(cos_theta, s.layers.iridescence_thickness, s.layers.iridescence_ior, f0);
    return mix(f, film, s.layers.iridescence);
}

fn d_ggx_aniso(nh: f32, th: f32, bh: f32, at: f32, ab: f32) -> f32 {
    let a2 = at * ab;
    let d = vec3<f32>(ab * th, at * bh, a2 * nh);
    let d2 = max(dot(d, d), 1e-8);
    let b2 = a2 / d2;
    return a2 * b2 * b2 / PI;
}

fn v_smith_aniso(at: f32, ab: f32, tv: f32, bv: f32, tl: f32, bl: f32, nv: f32, nl: f32) -> f32 {
    let lv = nl * length(vec3<f32>(at * tv, ab * bv, nv));
    let ll = nv * length(vec3<f32>(at * tl, ab * bl, nl));
    return 0.5 / max(lv + ll, 1e-5);
}

// The specular lobe's distribution times visibility, stretched along the
// anisotropy direction when there is one (KHR_materials_anisotropy).
fn specular_dv(s: Surface, l: vec3<f32>, h: vec3<f32>, nv: f32, nl: f32, nh: f32, a: f32) -> f32 {
    let k = s.layers.anisotropy;
    if k == 0.0 {
        return d_ggx(nh, a) * v_smith(nv, nl, a);
    }
    let n = s.normal;
    var t = normalize(s.layers.anisotropy_dir - n * dot(n, s.layers.anisotropy_dir));
    var b = cross(n, t);
    if k < 0.0 {
        let along = t;
        t = b;
        b = -along;
    }
    let at = max(mix(a, 1.0, k * k), 1e-3);
    let ab = max(a, 1e-3);
    let v = s.view;
    return d_ggx_aniso(nh, dot(t, h), dot(b, h), at, ab)
        * v_smith_aniso(at, ab, dot(t, v), dot(b, v), dot(t, l), dot(b, l), nv, nl);
}

// The Charlie sheen distribution for roughness `r`.
fn d_charlie(nh: f32, r: f32) -> f32 {
    let a = max(r * r, 0.0049);
    let inv = 1.0 / a;
    let sin2 = max(1.0 - nh * nh, 0.0);
    return (2.0 + inv) * pow(sin2, inv * 0.5) / (2.0 * PI);
}

fn sheen_lambda_fit(x: f32, a: f32) -> f32 {
    let t = sqr(1.0 - a);
    let ca = mix(21.5473, 25.3245, t);
    let cb = mix(3.82987, 3.32435, t);
    let cc = mix(0.19823, 0.16801, t);
    let cd = mix(-1.97760, -1.27393, t);
    let ce = mix(-4.32054, -4.85967, t);
    return ca / (1.0 + cb * pow(max(x, 1e-4), cc)) + cd * x + ce;
}

fn sheen_lambda(c: f32, a: f32) -> f32 {
    if c < 0.5 {
        return exp(sheen_lambda_fit(c, a));
    }
    return exp(2.0 * sheen_lambda_fit(0.5, a) - sheen_lambda_fit(1.0 - c, a));
}

// The sheen visibility of Estevez and Kulla, which keeps the lobe's
// albedo below one at grazing angles.
fn v_sheen(nv: f32, nl: f32, r: f32) -> f32 {
    let a = max(r * r, 0.0049);
    let v = 1.0 / ((1.0 + sheen_lambda(nv, a) + sheen_lambda(nl, a)) * 4.0 * nv * nl);
    return clamp(v, 0.0, 1.0);
}

// The sheen lobe's directional albedo, tabulated for roughness 0.07, 0.25,
// 0.5, 0.75 and 1 (rows) at cosines 0 to 1 in eighths (columns). The base
// is scaled down by it so the sheen adds no energy.
const SHEEN_ALBEDO: array<f32, 45> = array<f32, 45>(
    0.950, 0.256, 0.018, 0.001, 0.000, 0.000, 0.000, 0.000, 0.000,
    0.818, 0.475, 0.244, 0.125, 0.060, 0.026, 0.010, 0.002, 0.000,
    0.686, 0.516, 0.367, 0.264, 0.188, 0.132, 0.089, 0.055, 0.027,
    0.741, 0.609, 0.480, 0.378, 0.298, 0.235, 0.184, 0.140, 0.101,
    0.760, 0.643, 0.530, 0.435, 0.356, 0.295, 0.245, 0.202, 0.164,
);

fn sheen_albedo(nv: f32, r: f32) -> f32 {
    var table = SHEEN_ALBEDO;
    let rf = select(1.0 + (r - 0.25) / 0.25, (r - 0.07) / 0.18, r < 0.25);
    let ri = clamp(rf, 0.0, 3.999);
    let ci = clamp(nv * 8.0, 0.0, 7.999);
    let r0 = u32(ri);
    let c0 = u32(ci);
    let fr = ri - f32(r0);
    let fc = ci - f32(c0);
    let i = r0 * 9u + c0;
    let top = mix(table[i], table[i + 1u], fc);
    let bottom = mix(table[i + 9u], table[i + 10u], fc);
    return mix(top, bottom, fr);
}

// The subsurface approximation: light through thin parts, strongest when
// the light is behind the surface, plus a little wrap on the lit side.
fn subsurface_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    let k = s.layers.subsurface * (1.0 - s.layers.thickness);
    if k <= 0.0 {
        return vec3<f32>(0.0);
    }
    let through = normalize(l + s.normal * 0.3);
    let back = pow(max(dot(s.view, -through), 0.0), 3.0);
    return s.base * radiance * (back + 0.15) * k / PI;
}

// The layers' share of one light, given the base lobe's `base`: the
// sheen and clearcoat over it and the subsurface light through it.
fn layer_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>, base: vec3<f32>) -> vec3<f32> {
    let ly = s.layers;
    var color = base;
    if ly.clearcoat <= 0.0 && max3(ly.sheen) <= 0.0 && ly.subsurface <= 0.0 {
        return color;
    }
    let h = normalize(l + s.view);
    let vh = max(dot(s.view, h), 0.0);
    if max3(ly.sheen) > 0.0 {
        let n = s.normal;
        let nl = max(dot(n, l), 1e-4);
        let nv = max(dot(n, s.view), 1e-4);
        let nh = max(dot(n, h), 0.0);
        let r = ly.sheen_roughness;
        let lobe = ly.sheen * (d_charlie(nh, r) * v_sheen(nv, nl, r));
        let scale = 1.0 - max3(ly.sheen) * max(sheen_albedo(nv, r), sheen_albedo(nl, r));
        color = color * scale + lobe * radiance * nl;
    }
    if ly.clearcoat > 0.0 {
        let cn = ly.clearcoat_normal;
        let cnl = max(dot(cn, l), 0.0);
        let cnv = max(dot(cn, s.view), 1e-4);
        let cnh = max(dot(cn, h), 0.0);
        let ca = ly.clearcoat_roughness * ly.clearcoat_roughness;
        let fc = (0.04 + 0.96 * pow(1.0 - vh, 5.0)) * ly.clearcoat;
        let coat = d_ggx(cnh, ca) * v_smith(cnv, max(cnl, 1e-4), ca) * fc;
        color = color * (1.0 - fc) + coat * radiance * cnl;
    }
    return color + subsurface_light(s, l, radiance);
}

// What a light behind the surface still contributes.
fn layer_back_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    return subsurface_light(s, l, radiance);
}

fn env_ab(roughness: f32, nv: f32) -> vec2<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    return vec2<f32>(-1.04, 1.04) * a004 + r.zw;
}

// How much ambient light the specular lobe reflects.
fn env_weight(s: Surface, f0: vec3<f32>, nv: f32) -> vec3<f32> {
    var f = f0;
    if s.layers.iridescence > 0.0 {
        let film = thin_film(nv, s.layers.iridescence_thickness, s.layers.iridescence_ior, f0);
        f = mix(f0, film, s.layers.iridescence);
    }
    let ab = env_ab(s.roughness, nv);
    return f * ab.x + base_f90(s) * ab.y;
}

// The direction ambient reflections are looked up in: the mirror
// direction, bent across the highlight for an anisotropic surface.
fn layer_reflect(s: Surface, n: vec3<f32>) -> vec3<f32> {
    let k = s.layers.anisotropy;
    if k == 0.0 {
        return reflect(-s.view, n);
    }
    let t = normalize(s.layers.anisotropy_dir - n * dot(n, s.layers.anisotropy_dir));
    let across = select(t, cross(n, t), k >= 0.0);
    let at = cross(across, s.view);
    let an = cross(at, across);
    let bent = normalize(mix(n, an, abs(k)));
    return reflect(-s.view, bent);
}

// Ambient specular light along `dir` at `roughness`.
fn ambient_specular(dir: vec3<f32>, roughness: f32) -> vec3<f32> {
    if frame.env.z > 0.5 {
        let lod = roughness * (frame.env.y - 1.0);
        return textureSampleLevel(env_specular, env_sampler, dir, lod).rgb * frame.env.x;
    }
    return frame.ambient.rgb;
}

// Ambient irradiance from around `dir`.
fn ambient_irradiance(dir: vec3<f32>) -> vec3<f32> {
    if frame.env.z > 0.5 {
        return textureSampleLevel(env_irradiance, env_sampler, dir, 0.0).rgb * frame.env.x;
    }
    return frame.ambient.rgb;
}

// The layers' share of the ambient light, given the base lobes' `base`
// and the irradiance `irr` around the normal.
fn layer_ambient(s: Surface, base: vec3<f32>, irr: vec3<f32>) -> vec3<f32> {
    let ly = s.layers;
    var color = base;
    if ly.clearcoat <= 0.0 && max3(ly.sheen) <= 0.0 && ly.subsurface <= 0.0 {
        return color;
    }
    let nv = max(dot(s.normal, s.view), 1e-4);
    if max3(ly.sheen) > 0.0 {
        let e = sheen_albedo(nv, ly.sheen_roughness);
        color = color * (1.0 - max3(ly.sheen) * e) + ly.sheen * irr * e * s.occlusion;
    }
    if ly.clearcoat > 0.0 {
        let cn = ly.clearcoat_normal;
        let cnv = max(dot(cn, s.view), 1e-4);
        let fc = (0.04 + 0.96 * pow(1.0 - cnv, 5.0)) * ly.clearcoat;
        let r = reflect(-s.view, cn);
        color = color * (1.0 - fc) + ambient_specular(r, ly.clearcoat_roughness) * fc * s.occlusion;
    }
    if ly.subsurface > 0.0 {
        let k = ly.subsurface * (1.0 - ly.thickness) * 0.5;
        color += s.base * ambient_irradiance(-s.normal) * k * s.occlusion;
    }
    return color;
}

// The light a transmissive surface passes from behind: the opaque scene
// copy along the refracted ray, blurred by roughness, absorbed by the
// volume and tinted by the base colour. `w` is how much of whatever lies
// outside the copy's coverage (the clear colour) shows through.
fn transmission_light(s: Surface) -> vec4<f32> {
    let ly = s.layers;
    if ly.transmission <= 0.0 {
        return vec4<f32>(0.0);
    }
    let n = s.normal;
    let nv = max(dot(n, s.view), 1e-4);
    let weight = (1.0 - env_weight(s, base_f0(s), nv)) * (1.0 - s.metallic) * ly.transmission;
    var dir = refract(-s.view, n, 1.0 / ly.ior);
    if dot(dir, dir) < 1e-6 {
        dir = -s.view;
    }
    let exit = s.world + dir * ly.volume;
    let clip = frame.view_proj * vec4<f32>(exit, 1.0);
    let ndc = clip.xy / max(clip.w, 1e-4);
    let uv = clamp(vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let levels = f32(textureNumLevels(scene_copy));
    let rough = s.roughness * clamp(ly.ior * 2.0 - 2.0, 0.0, 1.0);
    let behind = textureSampleLevel(scene_copy, env_sampler, uv, rough * (levels - 1.0));
    var absorb = vec3<f32>(1.0);
    if ly.attenuation_distance > 0.0 {
        let sigma = -log(max(ly.attenuation, vec3<f32>(1e-4))) / ly.attenuation_distance;
        absorb = exp(-sigma * ly.volume);
    }
    let through = weight * s.base * absorb;
    let reveal = (1.0 - clamp(behind.a, 0.0, 1.0)) * dot(through, vec3<f32>(1.0 / 3.0));
    return vec4<f32>(behind.rgb * through, reveal);
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
