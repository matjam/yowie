// Temporal anti-aliasing: the scene renders with a sub-pixel jitter that
// changes every frame, and each pixel blends its new sample with the
// resolved picture of the frames before, read where the pixel was last
// frame. That place follows the moved object's own motion where the
// velocity image has it, otherwise the camera's motion through the
// nearest depth of the 3x3 neighbourhood. The history is clipped to the
// neighbourhood's colour spread (variance clipping in YCoCg) so stale
// colours from disoccluded surfaces do not ghost, and both samples are
// weighted by inverse luminance so bright pixels do not flicker.

@group(1) @binding(0) var current: texture_2d<f32>;
@group(1) @binding(1) var history: texture_2d<f32>;
@group(1) @binding(2) var depth_tex: texture_2d<f32>;
@group(1) @binding(3) var velocity_tex: texture_2d<f32>;

fn to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b,
    );
}

fn from_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

// Moves `h` towards `mean` until it lies inside the box `mean ± extent`.
fn clip_box(h: vec3<f32>, mean: vec3<f32>, extent: vec3<f32>) -> vec3<f32> {
    let off = h - mean;
    let units = abs(off / max(extent, vec3<f32>(1e-5)));
    let m = max(units.x, max(units.y, units.z));
    if m > 1.0 {
        return mean + off / m;
    }
    return h;
}

// Where the pixel at `px` (texture coordinates `uv`) was last frame.
fn previous_uv(px: vec2<i32>, uv: vec2<f32>) -> vec2<f32> {
    let size = vec2<i32>(post.size.xy);
    var best = -1.0;
    var best_p = px;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let q = clamp(px + vec2<i32>(x, y), vec2<i32>(0), size - 1);
            let d = textureLoad(depth_tex, q, 0).r;
            if d > best {
                best = d;
                best_p = q;
            }
        }
    }
    let vel = textureLoad(velocity_tex, best_p, 0);
    if vel.a > 0.5 {
        return uv - vel.xy;
    }
    return reproject(uv, best);
}

@fragment
fn fs_taa(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    let size = vec2<i32>(post.size.xy);
    let c = textureLoad(current, px, 0);
    if post.temporal.w < 0.5 {
        return c;
    }
    let uv = in.clip.xy * post.size.zw;
    let prev = previous_uv(px, uv);
    if any(prev < vec2<f32>(0.0)) || any(prev > vec2<f32>(1.0)) {
        return c;
    }
    var m1 = vec4<f32>(0.0);
    var m2 = vec4<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let s = textureLoad(current, clamp(px + vec2<i32>(x, y), vec2<i32>(0), size - 1), 0);
            let v = vec4<f32>(to_ycocg(s.rgb), s.a);
            m1 += v;
            m2 += v * v;
        }
    }
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec4<f32>(0.0))) * 1.25;
    let h = textureSampleLevel(history, linear_clamp, prev, 0.0);
    let hc = from_ycocg(clip_box(to_ycocg(h.rgb), mean.xyz, sigma.xyz));
    let ha = clamp(h.a, mean.w - sigma.w, mean.w + sigma.w);
    let blend = post.temporal.z;
    let wc = blend / (1.0 + luma(c.rgb));
    let wh = (1.0 - blend) / (1.0 + luma(max(hc, vec3<f32>(0.0))));
    let rgb = (c.rgb * wc + max(hc, vec3<f32>(0.0)) * wh) / (wc + wh);
    return vec4<f32>(rgb, mix(ha, c.a, blend));
}
