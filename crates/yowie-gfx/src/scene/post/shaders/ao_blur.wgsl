// The occlusion blur, one axis per pass: a Gaussian whose taps count less
// the further their depth is from the pixel's own, so occlusion does not
// bleed across silhouettes.

@group(1) @binding(0) var ao_tex: texture_2d<f32>;
@group(1) @binding(1) var depth_tex: texture_2d<f32>;

const RADIUS: i32 = 4;

fn blur(clip: vec2<f32>, axis: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(post.size.xy);
    let px = vec2<i32>(clip);
    let d = textureLoad(depth_tex, px, 0).r;
    if d <= 0.0 {
        return vec4<f32>(1.0);
    }
    let z = view_distance(clip * post.size.zw, d);
    var sum = 0.0;
    var total = 0.0;
    for (var i = -RADIUS; i <= RADIUS; i++) {
        let q = clamp(px + axis * i, vec2<i32>(0), size - 1);
        let qd = textureLoad(depth_tex, q, 0).r;
        let qz = view_distance((vec2<f32>(q) + 0.5) * post.size.zw, qd);
        let dz = abs(qz - z) / max(z, 1e-3);
        let g = exp(-f32(i * i) / 8.0);
        let w = g / (1.0 + 400.0 * dz * dz) * select(1.0, 0.0, qd <= 0.0);
        sum += textureLoad(ao_tex, q, 0).r * w;
        total += w;
    }
    return vec4<f32>(sum / max(total, 1e-5));
}

@fragment
fn fs_blur_x(in: FullOut) -> @location(0) vec4<f32> {
    return blur(in.clip.xy, vec2<i32>(1, 0));
}

@fragment
fn fs_blur_y(in: FullOut) -> @location(0) vec4<f32> {
    return blur(in.clip.xy, vec2<i32>(0, 1));
}
