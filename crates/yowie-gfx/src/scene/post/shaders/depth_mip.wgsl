// One mip of the nearest-depth pyramid: each texel holds the nearest
// (largest) depth of the texels of the finer mip it covers. Where the
// finer mip has an odd size, the last texel also covers the extra row or
// column, so no finer texel is left out.

@group(1) @binding(0) var finer: texture_2d<f32>;

@fragment
fn fs_depth_mip(in: FullOut) -> @location(0) vec4<f32> {
    let dst = vec2<i32>(in.clip.xy);
    let src = vec2<i32>(textureDimensions(finer));
    let last = max(src / 2, vec2<i32>(1)) - 1;
    let odd = (src & vec2<i32>(1)) == vec2<i32>(1);
    let span = vec2<i32>(2) + select(vec2<i32>(0), vec2<i32>(1), odd & (dst == last));
    var d = 0.0;
    for (var y = 0; y < span.y; y++) {
        for (var x = 0; x < span.x; x++) {
            let p = min(dst * 2 + vec2<i32>(x, y), src - 1);
            d = max(d, textureLoad(finer, p, 0).r);
        }
    }
    return vec4<f32>(d, 0.0, 0.0, 1.0);
}
