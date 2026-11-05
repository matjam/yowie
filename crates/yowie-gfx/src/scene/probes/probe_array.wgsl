// Reflection probe cubes as one cube array, a cube per probe slot.

@group(0) @binding(21) var probe_cubes: texture_cube_array<f32>;

fn probe_sample(layer: u32, d: vec3<f32>, lod: f32) -> vec3<f32> {
    return textureSampleLevel(probe_cubes, env_sampler, d, layer, lod).rgb;
}
