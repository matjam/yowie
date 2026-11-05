// Light shafts: the sky around the sun is the light source and any depth
// in front of it blocks it. Each half-size pixel walks towards the sun's
// place on screen, adding the unblocked steps with a weight that decays
// along the walk and falls off with the step's distance from the sun (so
// the whole sky does not glow), and the sum takes the sun's hue. The
// composite adds the result.

@group(1) @binding(0) var depth_tex: texture_2d<f32>;

@fragment
fn fs_rays(in: FullOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(post.size.xy);
    let taps = max(i32(post.rays1.x), 2);
    let delta = (in.uv - post.rays0.xy) * (post.rays1.y / f32(taps));
    let aspect = vec2<f32>(post.size.x * post.size.w, 1.0);
    var uv = in.uv;
    var fade = 1.0;
    var sum = 0.0;
    for (var i = 0; i < taps; i++) {
        uv -= delta;
        let p = clamp(vec2<i32>(uv * post.size.xy), vec2<i32>(0), size - 1);
        let sky = select(0.0, 1.0, textureLoad(depth_tex, p, 0).r <= 0.0);
        let near = clamp(1.0 - length((uv - post.rays0.xy) * aspect) * 1.5, 0.0, 1.0);
        sum += sky * near * near * fade * post.rays1.z;
        fade *= post.rays0.w;
    }
    return vec4<f32>(post.rays2.rgb * (sum * post.rays0.z), 1.0);
}
