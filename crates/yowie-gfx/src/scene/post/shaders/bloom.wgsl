// Bloom as a chain of downsamples and upsamples (Jimenez, "Next
// Generation Post Processing in Call of Duty: Advanced Warfare"). The
// prefilter reads the scene with a 13-tap filter, weights its groups by
// their brightness (the Karis average, which keeps single bright pixels
// from flickering) and keeps the light above the threshold with a soft
// knee. Each downsample halves the image with the same 13 taps; each
// upsample spreads the smaller mip with a 3x3 tent and is added to the
// larger one.

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var<storage, read> exposure_state: array<f32>;

fn tap(uv: vec2<f32>, o: vec2<f32>, texel: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, linear_clamp, uv + o * texel, 0.0).rgb;
}

fn karis(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>) -> vec4<f32> {
    let avg = (a + b + c + d) * 0.25;
    let w = 1.0 / (1.0 + luma(avg));
    return vec4<f32>(avg * w, w);
}

@fragment
fn fs_prefilter(in: FullOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(src, 0));
    let a = tap(in.uv, vec2<f32>(-2.0, -2.0), texel);
    let b = tap(in.uv, vec2<f32>(0.0, -2.0), texel);
    let c = tap(in.uv, vec2<f32>(2.0, -2.0), texel);
    let d = tap(in.uv, vec2<f32>(-2.0, 0.0), texel);
    let e = tap(in.uv, vec2<f32>(0.0, 0.0), texel);
    let f = tap(in.uv, vec2<f32>(2.0, 0.0), texel);
    let g = tap(in.uv, vec2<f32>(-2.0, 2.0), texel);
    let h = tap(in.uv, vec2<f32>(0.0, 2.0), texel);
    let i = tap(in.uv, vec2<f32>(2.0, 2.0), texel);
    let j = tap(in.uv, vec2<f32>(-1.0, -1.0), texel);
    let k = tap(in.uv, vec2<f32>(1.0, -1.0), texel);
    let l = tap(in.uv, vec2<f32>(-1.0, 1.0), texel);
    let m = tap(in.uv, vec2<f32>(1.0, 1.0), texel);
    let g0 = karis(j, k, l, m) * 0.5;
    let g1 = karis(a, b, d, e) * 0.125;
    let g2 = karis(b, c, e, f) * 0.125;
    let g3 = karis(d, e, g, h) * 0.125;
    let g4 = karis(e, f, h, i) * 0.125;
    let sum = g0 + g1 + g2 + g3 + g4;
    var color = sum.rgb / max(sum.w, 1e-6);
    let flags = u32(post.tone.z);
    if (flags & FLAG_FLAT) == 0u {
        var exposure = post.tone.x;
        if (flags & FLAG_AUTO_EXPOSURE) != 0u {
            exposure *= exposure_state[0];
        }
        color *= exposure;
    }
    // The soft threshold: a quadratic knee below the threshold, linear
    // above it.
    let threshold = post.bloom.y;
    let knee = max(post.bloom.z, 1e-4);
    let bright = max(color.r, max(color.g, color.b));
    var soft = clamp(bright - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee);
    let keep = max(soft, bright - threshold) / max(bright, 1e-4);
    return vec4<f32>(max(color * keep, vec3<f32>(0.0)), 1.0);
}

@fragment
fn fs_down(in: FullOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(src, 0));
    let a = tap(in.uv, vec2<f32>(-2.0, -2.0), texel);
    let b = tap(in.uv, vec2<f32>(0.0, -2.0), texel);
    let c = tap(in.uv, vec2<f32>(2.0, -2.0), texel);
    let d = tap(in.uv, vec2<f32>(-2.0, 0.0), texel);
    let e = tap(in.uv, vec2<f32>(0.0, 0.0), texel);
    let f = tap(in.uv, vec2<f32>(2.0, 0.0), texel);
    let g = tap(in.uv, vec2<f32>(-2.0, 2.0), texel);
    let h = tap(in.uv, vec2<f32>(0.0, 2.0), texel);
    let i = tap(in.uv, vec2<f32>(2.0, 2.0), texel);
    let j = tap(in.uv, vec2<f32>(-1.0, -1.0), texel);
    let k = tap(in.uv, vec2<f32>(1.0, -1.0), texel);
    let l = tap(in.uv, vec2<f32>(-1.0, 1.0), texel);
    let m = tap(in.uv, vec2<f32>(1.0, 1.0), texel);
    let color = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_up(in: FullOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(src, 0));
    let a = tap(in.uv, vec2<f32>(-1.0, -1.0), texel);
    let b = tap(in.uv, vec2<f32>(0.0, -1.0), texel);
    let c = tap(in.uv, vec2<f32>(1.0, -1.0), texel);
    let d = tap(in.uv, vec2<f32>(-1.0, 0.0), texel);
    let e = tap(in.uv, vec2<f32>(0.0, 0.0), texel);
    let f = tap(in.uv, vec2<f32>(1.0, 0.0), texel);
    let g = tap(in.uv, vec2<f32>(-1.0, 1.0), texel);
    let h = tap(in.uv, vec2<f32>(0.0, 1.0), texel);
    let i = tap(in.uv, vec2<f32>(1.0, 1.0), texel);
    let color = (e * 4.0 + (b + d + f + h) * 2.0 + (a + c + g + i)) / 16.0;
    return vec4<f32>(color, 1.0);
}
