// One reflection probe cube, for adapters without cube arrays: a scene
// shows at most one probe.

@group(0) @binding(21) var probe_cubes: texture_cube<f32>;

fn probe_sample(layer: u32, d: vec3<f32>, lod: f32) -> vec3<f32> {
    return textureSampleLevel(probe_cubes, env_sampler, d, lod).rgb;
}
