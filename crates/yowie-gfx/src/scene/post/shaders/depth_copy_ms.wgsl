// Resolves multisampled scene depth into the chain's R32Float depth
// image: the nearest sample of each pixel (the largest, depth being
// reversed).

@group(1) @binding(0) var depth_src: texture_depth_multisampled_2d;

@fragment
fn fs_depth_ms(in: FullOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.clip.xy);
    var d = 0.0;
    for (var i = 0u; i < textureNumSamples(depth_src); i++) {
        d = max(d, textureLoad(depth_src, p, i32(i)));
    }
    return vec4<f32>(d, 0.0, 0.0, 1.0);
}
