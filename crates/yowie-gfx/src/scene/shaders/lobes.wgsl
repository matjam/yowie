// The light of the layered material features: Fresnel with the specular
// layer and the iridescent film, the anisotropic lobe, sheen, clearcoat,
// subsurface, ambient light and transmission. `surface.wgsl` calls in here
// from `shade_light` and `shade_ambient`; `layers.wgsl` reads the features.

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
