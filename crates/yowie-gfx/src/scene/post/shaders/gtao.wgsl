// Ground-truth ambient occlusion (Jimenez et al. 2016, after XeGTAO):
// for a few slices through the view direction, the horizon on either
// side is found by stepping through depth within the occlusion radius,
// and the visible arc between the horizons is integrated against the
// surface normal (from the surface image) with a cosine weight. The slice
// angles turn per pixel with interleaved gradient noise, which the blur
// that follows removes. The output is visibility: 1 open, 0 occluded.

@group(1) @binding(0) var depth_tex: texture_2d<f32>;
@group(1) @binding(1) var surface_tex: texture_2d<f32>;

const SLICES: i32 = 3;
const STEPS: i32 = 4;

fn depth_at(p: vec2<f32>) -> f32 {
    let size = vec2<i32>(post.size.xy);
    return textureLoad(depth_tex, clamp(vec2<i32>(p), vec2<i32>(0), size - 1), 0).r;
}

fn pos_at(p: vec2<f32>) -> vec3<f32> {
    let c = floor(p) + 0.5;
    return view_pos(c * post.size.zw, depth_at(c));
}

@fragment
fn fs_gtao(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    let d = textureLoad(depth_tex, px, 0).r;
    if d <= 0.0 {
        return vec4<f32>(1.0);
    }
    let p = view_pos(in.clip.xy * post.size.zw, d);
    let n = normalize(to_view(oct_decode(textureLoad(surface_tex, px, 0).xy)));
    let v = to_eye(p);
    let radius = post.occlusion.y;
    // Pixels per world unit at this distance.
    var scale = post.proj[1][1] * 0.5 * post.size.y;
    if !ortho() {
        scale /= max(-p.z, 1e-3);
    }
    let r_px = radius * scale;
    if r_px < 1.0 {
        return vec4<f32>(1.0);
    }
    let falloff_range = 0.615 * radius;
    let falloff_mul = -1.0 / falloff_range;
    let falloff_add = (radius - falloff_range) / falloff_range + 1.0;
    let noise = ign(in.clip.xy);
    let noise2 = fract(noise * 7.31 + 0.5);
    let min_s = min(1.3 / r_px, 1.0);
    var visibility = 0.0;
    for (var slice = 0; slice < SLICES; slice++) {
        let phi = (f32(slice) + noise) * PI / f32(SLICES);
        let dir = vec3<f32>(cos(phi), sin(phi), 0.0);
        // View y is up, pixel y is down.
        let omega = vec2<f32>(dir.x, -dir.y) * r_px;
        let ortho_dir = dir - dot(dir, v) * v;
        let axis = normalize(cross(ortho_dir, v));
        let pn = n - axis * dot(n, axis);
        let pn_len = length(pn);
        let sign_n = sign(dot(ortho_dir, pn));
        let cos_n = clamp(dot(pn, v) / max(pn_len, 1e-5), 0.0, 1.0);
        let n_angle = sign_n * acos(cos_n);
        let low0 = cos(n_angle + PI * 0.5);
        let low1 = cos(n_angle - PI * 0.5);
        var h0 = low0;
        var h1 = low1;
        for (var step = 0; step < STEPS; step++) {
            var s = (f32(step) + noise2) / f32(STEPS);
            s = max(s * s, min_s);
            let off = omega * s;
            let d0 = pos_at(in.clip.xy + off) - p;
            let d1 = pos_at(in.clip.xy - off) - p;
            let l0 = length(d0);
            let l1 = length(d1);
            let w0 = clamp(l0 * falloff_mul + falloff_add, 0.0, 1.0);
            let w1 = clamp(l1 * falloff_mul + falloff_add, 0.0, 1.0);
            h0 = max(h0, mix(low0, dot(d0 / max(l0, 1e-5), v), w0));
            h1 = max(h1, mix(low1, dot(d1 / max(l1, 1e-5), v), w1));
        }
        var a0 = -acos(clamp(h1, -1.0, 1.0));
        var a1 = acos(clamp(h0, -1.0, 1.0));
        a0 = n_angle + clamp(a0 - n_angle, -PI * 0.5, PI * 0.5);
        a1 = n_angle + clamp(a1 - n_angle, -PI * 0.5, PI * 0.5);
        let arc0 = (cos_n + 2.0 * a0 * sin(n_angle) - cos(2.0 * a0 - n_angle)) * 0.25;
        let arc1 = (cos_n + 2.0 * a1 * sin(n_angle) - cos(2.0 * a1 - n_angle)) * 0.25;
        visibility += pn_len * (arc0 + arc1);
    }
    return vec4<f32>(clamp(visibility / f32(SLICES), 0.0, 1.0));
}
