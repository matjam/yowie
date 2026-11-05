// Depth of field, the combine: each full-size pixel mixes the sharp
// picture with the half-size gather by its own circle of confusion. The
// four gather texels around a pixel blend bilinearly, but one whose depth
// is far from the pixel's counts for little, so a sharp edge in front of
// a blurred background keeps no halo.

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var blurred: texture_2d<f32>;
@group(1) @binding(2) var depth_tex: texture_2d<f32>;

fn distance_of(p: vec2<i32>) -> f32 {
    let size = vec2<i32>(post.size.xy);
    let q = clamp(p, vec2<i32>(0), size - 1);
    return view_distance((vec2<f32>(q) + 0.5) * post.size.zw, textureLoad(depth_tex, q, 0).r);
}

@fragment
fn fs_focus_combine(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    let sharp = textureLoad(src, px, 0);
    let dist = distance_of(px);
    let range = max(post.dof.y, 1e-4);
    let here = clamp((abs(dist - post.dof.x) - range) / range, 0.0, 1.0);
    if here <= 0.0 {
        return sharp;
    }
    let hsize = vec2<i32>(textureDimensions(blurred));
    let hp = in.clip.xy * 0.5 - 0.5;
    let base = vec2<i32>(floor(hp));
    let f = hp - floor(hp);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var j = 0; j < 4; j++) {
        let o = vec2<i32>(j & 1, j >> 1u);
        let t = clamp(base + o, vec2<i32>(0), hsize - 1);
        let bx = select(1.0 - f.x, f.x, o.x == 1);
        let by = select(1.0 - f.y, f.y, o.y == 1);
        let dz = abs(distance_of(t * 2) - dist) / max(dist, 1e-3);
        let w = bx * by / (1.0 + 2500.0 * dz * dz) + 1e-5;
        sum += textureLoad(blurred, t, 0).rgb * w;
        total += w;
    }
    return vec4<f32>(mix(sharp.rgb, sum / total, here), sharp.a);
}
