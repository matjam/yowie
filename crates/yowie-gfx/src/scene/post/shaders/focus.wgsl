// Depth of field, the gather: at half size, each pixel's circle of
// confusion grows with its distance from the focus plane, and it gathers
// a disc of samples that wide (a golden-angle spiral, the bokeh). A
// sample nearer the eye than the pixel counts only as much as its own
// blur, so a sharp foreground does not smear over the background. Alpha
// carries the pixel's circle of confusion, 0 sharp to 1 fully blurred.

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var depth_tex: texture_2d<f32>;

// Distance in front of the eye at full-size texture coordinates `uv`.
fn distance_at(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(post.size.xy);
    let p = clamp(vec2<i32>(uv * post.size.xy), vec2<i32>(0), size - 1);
    return view_distance(uv, textureLoad(depth_tex, p, 0).r);
}

fn coc(dist: f32) -> f32 {
    let range = max(post.dof.y, 1e-4);
    return clamp((abs(dist - post.dof.x) - range) / range, 0.0, 1.0);
}

@fragment
fn fs_focus(in: FullOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let sharp = textureSampleLevel(src, linear_clamp, uv, 0.0);
    let dist = distance_at(uv);
    let here = coc(dist);
    if here <= 0.0 {
        return vec4<f32>(sharp.rgb, 0.0);
    }
    let half_texel = 2.0 * post.size.zw;
    let radius = here * post.dof.z;
    let taps = max(i32(post.dof.w), 1);
    var sum = sharp.rgb;
    var weight = 1.0;
    for (var i = 1; i <= taps; i++) {
        let angle = f32(i) * 2.39996323;
        let r = sqrt(f32(i) / f32(taps)) * radius;
        let q = uv + vec2<f32>(cos(angle), sin(angle)) * r * half_texel;
        let d = distance_at(q);
        let w = select(coc(d), 1.0, d >= dist);
        sum += textureSampleLevel(src, linear_clamp, q, 0.0).rgb * w;
        weight += w;
    }
    return vec4<f32>(sum / weight, here);
}
