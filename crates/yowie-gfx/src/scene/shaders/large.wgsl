// Helpers the large-scene programs share.

// The 4x4 ordered dither threshold of a pixel, in (0, 1). Two draws that
// show where the threshold is below and at or above the same fraction
// cover every pixel exactly once, which is how a model and its impostor
// cross-fade.
fn dither_threshold(frag: vec2<f32>) -> f32 {
    let x = u32(frag.x) & 3u;
    let y = u32(frag.y) & 3u;
    let a = x ^ y;
    let v = ((a & 1u) << 3u) | ((y & 1u) << 2u) | (((a >> 1u) & 1u) << 1u) | ((y >> 1u) & 1u);
    return (f32(v) + 0.5) / 16.0;
}

// A fresh Surface with every field zero but the given ones, so new
// Surface fields default to off.
fn plain_surface(base: vec3<f32>, normal: vec3<f32>, roughness: f32, world: vec3<f32>) -> Surface {
    var s: Surface;
    s.base = base;
    s.alpha = 1.0;
    s.metallic = 0.0;
    s.roughness = clamp(roughness, 0.03, 1.0);
    s.normal = normal;
    s.occlusion = 1.0;
    s.emissive = vec3<f32>(0.0);
    s.world = world;
    s.view = normalize(frame.camera.xyz - world);
    s.unlit = false;
    return s;
}

// The view depth of a world point, for the cluster and cascade lookups.
fn view_depth_of(world: vec3<f32>) -> f32 {
    return -(frame.view * vec4<f32>(world, 1.0)).z;
}
