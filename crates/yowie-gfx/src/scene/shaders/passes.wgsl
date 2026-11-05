// The entry points of the material passes beside the opaque, cut-out and
// sorted blended ones in `surface.wgsl`: order-independent transparency,
// transmission, fur shells, outlines and x-ray.

const IDENTITY: mat4x4<f32> = mat4x4<f32>(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
);

struct OitOut {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: f32,
}

// Weighted blended order-independent transparency (McGuire and Bavoil):
// each fragment adds its premultiplied colour times a weight that falls
// with distance, and multiplies the revealage by one minus its alpha. The
// weight is a tenth of the paper's equation 7, which keeps the 16-bit
// accumulation clear of overflow. `rgb` is linear colour, `a` coverage
// and `z` the view depth. Water and billboards (`large.wgsl`) write
// through this too.
fn oit_out(rgb: vec3<f32>, a: f32, z: f32) -> OitOut {
    let w = a * clamp(1.0 / (1e-5 + pow(z / 5.0, 2.0) + pow(z / 200.0, 6.0)), 1e-3, 300.0);
    var o: OitOut;
    o.accum = vec4<f32>(rgb * a, a) * w;
    o.reveal = a;
    return o;
}

@fragment
fn fs_oit(in: VertexOut, @builtin(front_facing) front: bool) -> OitOut {
    let c = lit(in, front);
    return oit_out(c.rgb, clamp(c.a, 0.0, 1.0), in.view_depth);
}

// A transmissive surface: its own reflection and diffuse light, plus what
// it passes through from the opaque scene copy.
@fragment
fn fs_transmit(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let s = material(surface(in, front));
    var c = shade(s, in.clip, in.view_depth);
    var reveal = 0.0;
    if !s.unlit {
        let t = transmission_light(s);
        c += t.rgb;
        reveal = t.w;
    }
    c = apply_fog(c, in.world);
    let a = clamp(s.alpha, 0.0, 1.0);
    return vec4<f32>(c * a, a * (1.0 - reveal));
}

// A fur shell's vertex: moved out along the normal to its height.
fn fur_out(v: VertexIn, p: Placed) -> VertexOut {
    let m = materials[v.params.x];
    let shells = max(m.ext[6].y, 1.0);
    let h = (f32(v.params.w >> 8u) + 1.0) / shells;
    var q = p;
    let n = p.normal;
    q.position += n * (inverseSqrt(max(dot(n, n), 1e-12)) * m.ext[6].z * h);
    var o = lit_out(v, q);
    o.shell = h;
    return o;
}

@vertex
fn vs_fur_static(v: VertexIn) -> VertexOut {
    return fur_out(v, place(v, IDENTITY));
}

@vertex
fn vs_fur_skinned(v: VertexIn, s: SkinIn) -> VertexOut {
    return fur_out(v, place(v, skin_matrix(v.params.y, s)));
}

// A fur shell keeps the strands that reach its height, darkened towards
// the root where the strands shade each other.
@fragment
fn fs_fur(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let m = materials[in.material];
    let mask = fur_mask(m, transform_uv(m, in.uv));
    var s = material(surface(in, front));
    let root = mix(0.35, 1.0, in.shell);
    s.occlusion *= root;
    let c = apply_fog(shade(s, in.clip, in.view_depth) * mix(0.6, 1.0, in.shell), in.world);
    if mask < in.shell {
        discard;
    }
    return vec4<f32>(c, 1.0);
}

struct OutlineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) material: u32,
}

// The outline hull: the mesh with every vertex pushed out across the
// screen along its normal by the outline's width in pixels.
fn outline_out(v: VertexIn, p: Placed) -> OutlineOut {
    let m = materials[v.params.x];
    var clip = frame.view_proj * vec4<f32>(p.position, 1.0);
    let n = p.normal * inverseSqrt(max(dot(p.normal, p.normal), 1e-12));
    let tip = frame.view_proj * vec4<f32>(p.position + n * 0.05, 1.0);
    if clip.w > 1e-5 && tip.w > 1e-5 {
        let d = (tip.xy / tip.w - clip.xy / clip.w) * frame.target_size.xy;
        let len = length(d);
        if len > 1e-6 {
            let offset = d / len * (m.ext[6].w * 2.0) * frame.target_size.zw;
            clip = vec4<f32>(clip.xy + offset * clip.w, clip.zw);
        }
    }
    var o: OutlineOut;
    o.clip = clip;
    o.material = v.params.x;
    return o;
}

@vertex
fn vs_outline_static(v: VertexIn) -> OutlineOut {
    return outline_out(v, place(v, IDENTITY));
}

@vertex
fn vs_outline_skinned(v: VertexIn, s: SkinIn) -> OutlineOut {
    return outline_out(v, place(v, skin_matrix(v.params.y, s)));
}

@fragment
fn fs_outline(in: OutlineOut) -> @location(0) vec4<f32> {
    let c = materials[in.material].ext[7];
    return vec4<f32>(c.rgb * c.a, c.a);
}

// The x-ray tint over the parts of a mesh behind other geometry, stronger
// towards the silhouette.
@fragment
fn fs_xray(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = materials[in.material].ext[8];
    let n = normalize(select(-in.normal, in.normal, front));
    let v = normalize(frame.camera.xyz - in.world);
    let rim = 1.0 - abs(dot(n, v));
    let k = c.a * (0.35 + 0.65 * rim * rim);
    return vec4<f32>(c.rgb * k, k);
}
