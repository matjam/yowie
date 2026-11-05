// FXAA (Lottes), the console variant: where the local contrast of luma
// is high, blends along the edge direction. It reads the composited
// picture from an sRGB image, so the sampled values are linear and the
// luma is taken of their square root, close to the display value.

@group(1) @binding(0) var src: texture_2d<f32>;

fn fxaa_luma(c: vec3<f32>) -> f32 {
    return sqrt(dot(c, vec3<f32>(0.299, 0.587, 0.114)));
}

@fragment
fn fs_fxaa(in: FullOut) -> @location(0) vec4<f32> {
    let px = 1.0 / vec2<f32>(textureDimensions(src, 0));
    let m = textureSampleLevel(src, linear_clamp, in.uv, 0.0);
    let l_m = fxaa_luma(m.rgb);
    let l_nw = fxaa_luma(textureSampleLevel(src, linear_clamp, in.uv + vec2<f32>(-1.0, -1.0) * px, 0.0).rgb);
    let l_ne = fxaa_luma(textureSampleLevel(src, linear_clamp, in.uv + vec2<f32>(1.0, -1.0) * px, 0.0).rgb);
    let l_sw = fxaa_luma(textureSampleLevel(src, linear_clamp, in.uv + vec2<f32>(-1.0, 1.0) * px, 0.0).rgb);
    let l_se = fxaa_luma(textureSampleLevel(src, linear_clamp, in.uv + vec2<f32>(1.0, 1.0) * px, 0.0).rgb);
    let l_min = min(l_m, min(min(l_nw, l_ne), min(l_sw, l_se)));
    let l_max = max(l_m, max(max(l_nw, l_ne), max(l_sw, l_se)));
    if l_max - l_min < max(0.0312, l_max * 0.125) {
        return m;
    }
    var dir = vec2<f32>(-((l_nw + l_ne) - (l_sw + l_se)), (l_nw + l_sw) - (l_ne + l_se));
    let reduce = max((l_nw + l_ne + l_sw + l_se) * 0.03125, 0.0078125);
    let rcp = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * rcp, vec2<f32>(-8.0), vec2<f32>(8.0)) * px;
    let a = 0.5 * (textureSampleLevel(src, linear_clamp, in.uv + dir * (1.0 / 3.0 - 0.5), 0.0)
        + textureSampleLevel(src, linear_clamp, in.uv + dir * (2.0 / 3.0 - 0.5), 0.0));
    let b = a * 0.5 + 0.25 * (textureSampleLevel(src, linear_clamp, in.uv + dir * -0.5, 0.0)
        + textureSampleLevel(src, linear_clamp, in.uv + dir * 0.5, 0.0));
    let l_b = fxaa_luma(b.rgb);
    return select(b, a, l_b < l_min || l_b > l_max);
}
