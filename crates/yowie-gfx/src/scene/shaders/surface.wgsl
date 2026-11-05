// The surface program: reads the material into a Surface, then lights it
// with the sun (cascaded shadows), the clustered point and spot lights
// (atlas shadows), and the ambient or image-based light, then applies fog
// and aerial perspective.

@group(1) @binding(0) var t_base: texture_2d<f32>;
@group(1) @binding(1) var t_metal_rough: texture_2d<f32>;
@group(1) @binding(2) var t_normal: texture_2d<f32>;
@group(1) @binding(3) var t_occlusion: texture_2d<f32>;
@group(1) @binding(4) var t_emissive: texture_2d<f32>;
@group(1) @binding(5) var s_base: sampler;
@group(1) @binding(6) var s_metal_rough: sampler;
@group(1) @binding(7) var s_normal: sampler;
@group(1) @binding(8) var s_occlusion: sampler;
@group(1) @binding(9) var s_emissive: sampler;

struct Surface {
    base: vec3<f32>,
    alpha: f32,
    metallic: f32,
    roughness: f32,
    normal: vec3<f32>,
    occlusion: f32,
    emissive: vec3<f32>,
    world: vec3<f32>,
    view: vec3<f32>,
    unlit: bool,
    // The mesh's texture coordinates and vertex colour.
    uv: vec2<f32>,
    uv2: vec2<f32>,
    color: vec4<f32>,
    // The mesh normal before normal mapping, and a unit tangent.
    geometric: vec3<f32>,
    tangent: vec3<f32>,
    // Clearcoat, sheen, transmission and the other layered features.
    layers: Layers,
}

fn uv_of(m: Material, slot: u32, uv: vec2<f32>, uv2: vec2<f32>) -> vec2<f32> {
    let src = select(uv, uv2, ((m.flags.y >> slot) & 1u) != 0u);
    let p = vec3<f32>(src, 1.0);
    return vec2<f32>(dot(m.uv_row0.xyz, p), dot(m.uv_row1.xyz, p));
}

// The material's base colour and alpha. Textures are stored
// premultiplied, so the texel is divided by its alpha first.
fn base_color(in: VertexOut) -> vec4<f32> {
    let m = materials[in.material];
    let texel = textureSample(t_base, s_base, uv_of(m, 0u, in.uv, in.uv2));
    var c = vec4<f32>(texel.rgb / max(texel.a, 1e-4), texel.a) * m.base_color;
    if (m.flags.x & MAT_VERTEX_COLORS) != 0u {
        c *= in.color;
    }
    return c;
}

fn surface(in: VertexOut, front: bool) -> Surface {
    let m = materials[in.material];
    var s: Surface;
    let bc = base_color(in);
    s.base = bc.rgb;
    s.alpha = bc.a;
    let mr = textureSample(t_metal_rough, s_metal_rough, uv_of(m, 1u, in.uv, in.uv2));
    s.metallic = clamp(m.params.x * mr.b, 0.0, 1.0);
    s.roughness = clamp(m.params.y * mr.g, 0.03, 1.0);
    let occ = textureSample(t_occlusion, s_occlusion, uv_of(m, 3u, in.uv, in.uv2)).r;
    s.occlusion = mix(1.0, occ, m.params.z);
    let em = textureSample(t_emissive, s_emissive, uv_of(m, 4u, in.uv, in.uv2));
    s.emissive = m.emissive.rgb * em.rgb;
    let nt = textureSample(t_normal, s_normal, uv_of(m, 2u, in.uv, in.uv2)).xyz * 2.0 - 1.0;
    var n = normalize(in.normal);
    var t = in.tangent.xyz;
    if !front {
        n = -n;
        t = -t;
    }
    let geometric = n;
    let tangent = t;
    if (m.flags.x & MAT_NORMAL_MAP) != 0u && dot(t, t) > 1e-8 {
        t = normalize(t - n * dot(n, t));
        let b = cross(n, t) * in.tangent.w;
        let local = vec3<f32>(nt.xy * m.emissive.w, nt.z);
        n = normalize(t * local.x + b * local.y + n * local.z);
    }
    s.normal = n;
    s.world = in.world;
    s.view = normalize(frame.camera.xyz - in.world);
    s.unlit = (m.flags.x & MAT_UNLIT) != 0u;
    finish_surface(&s, in, geometric, tangent);
    return s;
}

// One shadow map lookup with a 3x3 filter of bilinear comparisons.
// `dist` scales the normal offset of perspective maps.
fn shadow_lookup(index: u32, world: vec3<f32>, n: vec3<f32>, dist: f32) -> f32 {
    let v = frame.shadow_views[index];
    let p = world + n * (v.params.x + v.params.y * dist);
    let clip = v.view_proj * vec4<f32>(p, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z > 1.0 {
        return 1.0;
    }
    let texel = frame.shadow.xy;
    let lo = v.rect.xy + texel * 1.5;
    let hi = v.rect.xy + v.rect.zw - texel * 1.5;
    let uv = v.rect.xy + vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * v.rect.zw;
    let depth = max(ndc.z, 0.0);
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = clamp(uv + vec2<f32>(f32(x), f32(y)) * texel, lo, hi);
            sum += textureSampleCompareLevel(shadow_atlas, shadow_sampler, o, depth);
        }
    }
    return sum / 9.0;
}

fn sun_shadow(s: Surface, view_depth: f32) -> f32 {
    let count = u32(frame.shadow.z);
    for (var i = 0u; i < count; i++) {
        if view_depth < frame.cascade_far[i] {
            let lit = shadow_lookup(i, s.world, s.normal, 0.0);
            return mix(1.0, lit, frame.sun_color.w);
        }
    }
    return 1.0;
}

fn cube_face(d: vec3<f32>) -> u32 {
    let a = abs(d);
    if a.x >= a.y && a.x >= a.z {
        return select(1u, 0u, d.x > 0.0);
    }
    if a.y >= a.z {
        return select(3u, 2u, d.y > 0.0);
    }
    return select(5u, 4u, d.z > 0.0);
}

fn d_ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn v_smith(nv: f32, nl: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = nl * sqrt(nv * nv * (1.0 - a2) + a2);
    let gl = nv * sqrt(nl * nl * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn f_schlick(f0: vec3<f32>, vh: f32) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - vh, 5.0);
}

// The light leaving the surface towards the eye from one light arriving
// along l with radiance `radiance`.
fn shade_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    let n = s.normal;
    let nl = dot(n, l);
    if nl <= 0.0 {
        return layer_back_light(s, l, radiance);
    }
    let h = normalize(l + s.view);
    let nv = max(dot(n, s.view), 1e-4);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(s.view, h), 0.0);
    let a = s.roughness * s.roughness;
    let f0 = base_f0(s);
    let f = layer_fresnel(s, f0, vh);
    let spec = f * specular_dv(s, l, h, nv, nl, nh, a);
    let diffuse = (1.0 - f) * (1.0 - s.metallic) * (1.0 - s.layers.transmission) * s.base / PI;
    return layer_light(s, l, radiance, (diffuse + spec) * radiance * nl);
}

// Ambient light: reflection probes and the light probe grid where they
// reach, then the environment or sky maps when bound, otherwise the flat
// ambient colour (`probes.wgsl`).
fn shade_ambient(s: Surface) -> vec3<f32> {
    let n = s.normal;
    let nv = max(dot(n, s.view), 1e-4);
    let f0 = base_f0(s);
    let spec_weight = env_weight(s, f0, nv);
    let diffuse_weight = (1.0 - spec_weight) * (1.0 - s.metallic) * (1.0 - s.layers.transmission) * s.base;
    let spec = ambient_specular(s.world, layer_reflect(s, n), s.roughness);
    let irr = ambient_diffuse(s.world, n);
    let c = (irr * diffuse_weight + spec * spec_weight) * s.occlusion;
    return layer_ambient(s, c, irr);
}

fn cluster_index(frag: vec4<f32>, view_depth: f32) -> u32 {
    let tile = vec2<u32>(frag.xy / frame.cluster.w);
    let tx = min(tile.x, u32(frame.cluster.x) - 1u);
    let ty = min(tile.y, u32(frame.cluster.y) - 1u);
    let slices = u32(frame.cluster.z);
    let sf = log(max(view_depth, frame.cluster_depth.x)) * frame.cluster_depth.z + frame.cluster_depth.w;
    let slice = min(u32(max(sf, 0.0)), slices - 1u);
    return (slice * u32(frame.cluster.y) + ty) * u32(frame.cluster.x) + tx;
}

fn shade(s: Surface, frag: vec4<f32>, view_depth: f32) -> vec3<f32> {
    if s.unlit {
        return s.base + s.emissive;
    }
    var color = vec3<f32>(0.0);
    if frame.sun_dir.w > 0.5 {
        color += shade_light(s, frame.sun_dir.xyz, frame.sun_color.rgb) * sun_shadow(s, view_depth);
    }
    if frame.counts.x > 0u {
        let cell = clusters[cluster_index(frag, view_depth)];
        let first = cell >> 8u;
        let count = cell & 0xffu;
        for (var i = 0u; i < count; i++) {
            let light = lights[light_indices[first + i]];
            let d = light.pos_range.xyz - s.world;
            let dist = length(d);
            let range = max(light.pos_range.w, 1e-3);
            if dist >= range {
                continue;
            }
            let l = d / max(dist, 1e-5);
            let x = dist / range;
            var att = clamp(1.0 - x * x, 0.0, 1.0);
            att *= att;
            let shadow_view = light.params.y;
            if light.color_kind.w > 0.5 {
                let cd = dot(-l, light.dir_cos_outer.xyz);
                att *= smoothstep(light.dir_cos_outer.w, max(light.params.x, light.dir_cos_outer.w + 1e-3), cd);
                if shadow_view >= 0.0 && att > 0.0 {
                    att *= shadow_lookup(u32(shadow_view), s.world, s.normal, dist);
                }
            } else if shadow_view >= 0.0 && att > 0.0 {
                att *= shadow_lookup(u32(shadow_view) + cube_face(-d), s.world, s.normal, dist);
            }
            if att > 0.0 {
                color += shade_light(s, l, light.color_kind.rgb * att);
            }
        }
    }
    return color + shade_ambient(s) + s.emissive;
}

// The air a ray of length dist from height y0 to height y1 passes
// through, in units of the fog's full density: full at and below height
// h, thinning by e every 1 / k above it.
fn height_fog_depth(y0: f32, y1: f32, dist: f32, h: f32, k: f32) -> f32 {
    let a = min(y0, y1);
    let b = max(y0, y1);
    if b - a < 1e-3 {
        return dist * min(exp(-(a - h) * k), 1.0);
    }
    var sum = clamp(h, a, b) - a;
    if b > h {
        sum += (exp(-(max(a, h) - h) * k) - exp(-(b - h) * k)) / k;
    }
    return dist * sum / (b - a);
}

// Distance and height fog, then the atmosphere between the eye and the
// surface.
fn apply_fog(color: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let to = world - frame.camera.xyz;
    let dist = length(to);
    var c = color;
    let fp = frame.fog_params;
    var f = 0.0;
    if fp.y > fp.x {
        f = clamp((dist - fp.x) / (fp.y - fp.x), 0.0, 1.0);
        if fp.w > 0.0 {
            f *= clamp(exp(-(world.y - fp.z) * fp.w), 0.0, 1.0);
        }
    }
    if frame.fog_color.w > 0.0 {
        var depth = dist;
        if fp.w > 0.0 {
            depth = height_fog_depth(frame.camera.y, world.y, dist, fp.z, fp.w);
        }
        f = max(f, 1.0 - exp(-frame.fog_color.w * depth));
    }
    c = mix(c, frame.fog_color.rgb, f);
    if frame.beta_m.w > 0.5 && dist > 0.0 {
        let sc = atmosphere_scatter(to / dist, dist * frame.sky_ground.w, 4, 2);
        c = c * sc.transmittance + sc.radiance * frame.sky_horizon.w;
    }
    return c;
}

fn lit(in: VertexOut, front: bool) -> vec4<f32> {
    let s = material(surface(in, front));
    let c = apply_fog(shade(s, in.clip, in.view_depth), in.world);
    return vec4<f32>(c, s.alpha);
}

@fragment
fn fs_opaque(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return vec4<f32>(lit(in, front).rgb, 1.0);
}

@fragment
fn fs_mask(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = lit(in, front);
    if c.a < materials[in.material].params.w {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}

// The outputs of opaque surfaces when post effects read the surface:
// colour; the world normal (octahedral), roughness and the share of the
// colour that is ambient light (ambient occlusion scales only that share);
// the environment reflection weight (screen-space reflections replace the
// environment's reflection by that weight).
struct PostOut {
    @location(0) color: vec4<f32>,
    @location(1) surface: vec4<f32>,
    @location(2) specular: vec4<f32>,
}

fn oct_encode(n: vec3<f32>) -> vec2<f32> {
    let p = n.xy / (abs(n.x) + abs(n.y) + abs(n.z));
    if n.z >= 0.0 {
        return p;
    }
    return (1.0 - abs(p.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
}

// The outputs of surface `s`, shaded to `total` before fog, at `world`.
// The specular image holds how much environment reflection the surface
// shows, as `shade_ambient` weighs it: the base lobe under the clearcoat,
// plus the coat. The roughness written for reflections moves towards the
// coat's roughness by the coat's share of that reflection.
fn post_out(s: Surface, total: vec3<f32>, world: vec3<f32>) -> PostOut {
    var out: PostOut;
    out.color = vec4<f32>(apply_fog(total, world), s.alpha);
    let weights = vec3<f32>(0.2126, 0.7152, 0.0722);
    var ambient = 0.0;
    var spec = vec3<f32>(0.0);
    var rough = s.roughness;
    if !s.unlit {
        ambient = clamp(dot(shade_ambient(s), weights) / max(dot(total, weights), 1e-5), 0.0, 1.0);
        let nv = max(dot(s.normal, s.view), 1e-4);
        spec = env_weight(s, base_f0(s), nv) * s.occlusion;
        let ly = s.layers;
        if ly.clearcoat > 0.0 {
            let cnv = max(dot(ly.clearcoat_normal, s.view), 1e-4);
            let fc = (0.04 + 0.96 * pow(1.0 - cnv, 5.0)) * ly.clearcoat;
            spec = spec * (1.0 - fc) + vec3<f32>(fc * s.occlusion);
            let share = clamp(fc * s.occlusion / max(dot(spec, weights), 1e-4), 0.0, 1.0);
            rough = mix(s.roughness, min(s.roughness, ly.clearcoat_roughness), share);
        }
    }
    out.surface = vec4<f32>(oct_encode(s.normal), rough, ambient);
    out.specular = vec4<f32>(spec, 1.0);
    return out;
}

fn lit_post(in: VertexOut, front: bool) -> PostOut {
    let s = material(surface(in, front));
    return post_out(s, shade(s, in.clip, in.view_depth), in.world);
}

@fragment
fn fs_opaque_post(in: VertexOut, @builtin(front_facing) front: bool) -> PostOut {
    var out = lit_post(in, front);
    out.color.a = 1.0;
    return out;
}

@fragment
fn fs_mask_post(in: VertexOut, @builtin(front_facing) front: bool) -> PostOut {
    var out = lit_post(in, front);
    if out.color.a < materials[in.material].params.w {
        discard;
    }
    out.color.a = 1.0;
    return out;
}

@fragment
fn fs_blend(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = lit(in, front);
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(c.rgb * a, a);
}

@fragment
fn fs_shadow_mask(in: ShadowOut) {
    let m = materials[in.material];
    let texel = textureSample(t_base, s_base, uv_of(m, 0u, in.uv, in.uv2));
    var a = texel.a * m.base_color.a;
    if (m.flags.x & MAT_VERTEX_COLORS) != 0u {
        a *= in.color.a;
    }
    if a < m.params.w {
        discard;
    }
}
