// Motion blur: each pixel averages the picture along its motion since
// last frame (the moved object's own where the velocity image has it,
// otherwise the camera's), scaled by the strength and centred on the
// pixel. The motion is capped at a tenth of the picture's width.

@group(1) @binding(0) var src: texture_2d<f32>;
@group(1) @binding(1) var depth_tex: texture_2d<f32>;
@group(1) @binding(2) var velocity_tex: texture_2d<f32>;

@fragment
fn fs_motion(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    let uv = in.clip.xy * post.size.zw;
    let vel = textureLoad(velocity_tex, px, 0);
    var motion = vel.xy;
    if vel.a < 0.5 {
        motion = uv - reproject(uv, textureLoad(depth_tex, px, 0).r);
    }
    motion *= post.temporal.x;
    let len = length(motion);
    if len > 0.1 {
        motion *= 0.1 / len;
    }
    if len * post.size.x < 0.5 {
        return textureLoad(src, px, 0);
    }
    let taps = max(i32(post.temporal.y), 2);
    let jitter = ign(in.clip.xy) - 0.5;
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < taps; i++) {
        let t = (f32(i) + 0.5 + jitter) / f32(taps) - 0.5;
        sum += textureSampleLevel(src, linear_clamp, uv - motion * t, 0.0);
    }
    return sum / f32(taps);
}
