// Copies the scene depth into the chain's R32Float depth image, which
// every pass that reads depth loads from.

@group(1) @binding(0) var depth_src: texture_depth_2d;

@fragment
fn fs_depth(in: FullOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(depth_src, vec2<i32>(in.clip.xy), 0), 0.0, 0.0, 1.0);
}
