// The composite: lens distortion and chromatic aberration on the way in,
// exposure (manual times auto), bloom, light shafts and lens ghosts, the
// white balance, the tone curve, the grade (saturation, contrast, lift,
// gamma, gain, lookup table), grain and vignette, then composition over
// the background colour. Each stage runs only when its flag is set, so
// the default settings run exposure and the tone curve alone. The output
// is sRGB or linear float, so the program writes linear light. A 2D frame
// (FLAG_FLAT) skips exposure and the tone curve.

@group(1) @binding(0) var scene: texture_2d<f32>;
@group(1) @binding(1) var bloom_tex: texture_2d<f32>;
@group(1) @binding(2) var ao_tex: texture_2d<f32>;
@group(1) @binding(3) var rays_tex: texture_2d<f32>;
@group(1) @binding(4) var lut_tex: texture_3d<f32>;
@group(1) @binding(5) var<storage, read> exposure_state: array<f32>;

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0031308)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.04045)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// A per-pixel hash moved each frame by `seed`.
fn grain_noise(p: vec2<f32>, seed: f32) -> f32 {
    return fract(sin(dot(p + fract(seed * 0.618034) * 113.0, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn sample_scene(uv: vec2<f32>, off: vec2<f32>, flags: u32) -> vec4<f32> {
    if (flags & FLAG_LENS) != 0u && post.lens.y > 0.0 {
        // Red and blue sampled either side of green, further apart towards
        // the edges; 1 is about three pixels at the edge of a 1080-wide
        // picture.
        let shift = off * post.lens.y * 0.0055;
        let g = textureSampleLevel(scene, linear_clamp, uv, 0.0);
        let r = textureSampleLevel(scene, linear_clamp, uv + shift, 0.0).r;
        let b = textureSampleLevel(scene, linear_clamp, uv - shift, 0.0).b;
        return vec4<f32>(r, g.g, b, g.a);
    }
    return textureSampleLevel(scene, linear_clamp, uv, 0.0);
}

// The lens ghosts: the bloom mirrored through the centre a few times
// over, each copy dimmer than the last.
fn ghosts(uv: vec2<f32>) -> vec3<f32> {
    let to_centre = vec2<f32>(0.5) - uv;
    var sum = vec3<f32>(0.0);
    for (var k = 1; k <= 3; k++) {
        let p = uv + to_centre * (f32(k) * 0.55);
        let edge = smoothstep(0.0, 0.1, p.x) * smoothstep(0.0, 0.1, p.y)
            * smoothstep(0.0, 0.1, 1.0 - p.x) * smoothstep(0.0, 0.1, 1.0 - p.y);
        sum += textureSampleLevel(bloom_tex, linear_clamp, p, 0.0).rgb * edge / f32(k);
    }
    return sum;
}

fn grade(c_in: vec3<f32>, flags: u32) -> vec3<f32> {
    var c = clamp(c_in, vec3<f32>(0.0), vec3<f32>(1.0));
    let l = luma(c);
    c = max(mix(vec3<f32>(l), c, post.grade.y), vec3<f32>(0.0));
    // Contrast, lift, gamma and gain work on display values.
    var d = srgb_encode(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    d = (d - 0.5) * post.grade.x + 0.5;
    d = post.gain.rgb * (d + post.lift.rgb * (1.0 - d));
    d = pow(max(d, vec3<f32>(0.0)), 1.0 / max(post.gamma.rgb, vec3<f32>(1e-3)));
    d = clamp(d, vec3<f32>(0.0), vec3<f32>(1.0));
    if (flags & FLAG_LUT) != 0u {
        let n = post.grade.w;
        let coord = d * ((n - 1.0) / n) + 0.5 / n;
        let graded = textureSampleLevel(lut_tex, linear_clamp, coord, 0.0).rgb;
        d = mix(d, graded, post.grade.z);
    }
    return srgb_decode(d);
}

@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4<f32> {
    let flags = u32(post.tone.z);
    let off = in.uv - 0.5;
    var uv = in.uv;
    if (flags & FLAG_LENS) != 0u && post.lens.z != 0.0 {
        uv = 0.5 + off * (1.0 + post.lens.z * dot(off, off));
    }
    if (flags & FLAG_SHOW_AO) != 0u {
        let o = textureSampleLevel(ao_tex, linear_clamp, uv, 0.0).r;
        return vec4<f32>(vec3<f32>(o), 1.0);
    }
    let s = sample_scene(uv, off, flags);
    let a = clamp(s.a, 0.0, 1.0);
    var c = s.rgb / max(a, 1e-4);
    let flat = (flags & FLAG_FLAT) != 0u;
    if !flat {
        var e = post.tone.x;
        if (flags & FLAG_AUTO_EXPOSURE) != 0u {
            e *= exposure_state[0];
        }
        c = c * e;
    }
    // Bloom, ghosts and shafts are light in front of the picture: they add
    // to what the picture covers and, tone-mapped on their own, to the
    // background where it does not.
    var glow = vec3<f32>(0.0);
    if (flags & FLAG_BLOOM) != 0u {
        glow = textureSampleLevel(bloom_tex, linear_clamp, uv, 0.0).rgb * post.bloom.x;
        if post.bloom.w > 0.0 {
            glow += ghosts(uv) * post.bloom.w;
        }
    }
    if (flags & FLAG_RAYS) != 0u {
        glow += textureSampleLevel(rays_tex, linear_clamp, uv, 0.0).rgb;
    }
    c += glow;
    var uncovered = vec3<f32>(0.0);
    if a < 1.0 && any(glow > vec3<f32>(0.0)) {
        if flat {
            uncovered = clamp(glow, vec3<f32>(0.0), vec3<f32>(1.0));
        } else {
            uncovered = tonemap(glow, u32(post.tone.y));
        }
    }
    if (flags & FLAG_GRADE) != 0u {
        c = max(mat3x3<f32>(post.balance0.xyz, post.balance1.xyz, post.balance2.xyz) * c, vec3<f32>(0.0));
    }
    if flat {
        c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    } else {
        c = tonemap(c, u32(post.tone.y));
    }
    if (flags & FLAG_GRADE) != 0u {
        c = grade(c, flags);
    }
    if (flags & FLAG_LENS) != 0u {
        if post.lens.w > 0.0 {
            let n = grain_noise(floor(in.clip.xy), post.tone.w) - 0.5;
            c = clamp(c + n * post.lens.w, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        c *= clamp(1.0 - post.lens.x * dot(off, off) * 2.0, 0.0, 1.0);
    }
    let rgb = c * a;
    let bg = post.background;
    return vec4<f32>(rgb + (bg.rgb + uncovered) * (1.0 - a), a + bg.a * (1.0 - a));
}
