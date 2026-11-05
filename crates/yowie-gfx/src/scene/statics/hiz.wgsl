// The depth pyramid static groups are culled against: each texel of a
// level holds the farthest (smallest, depth being reversed) of the 2 by 2
// texels below it, level 0 reducing the scene's depth (its size rounded
// up). Higher levels are half the size rounded down, so the last texel of
// a row or column made from an odd size also takes the texels left over.

@group(0) @binding(0) var src_depth: texture_depth_2d;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var src_depth_ms: texture_depth_multisampled_2d;

// The farthest sample of a multisampled depth texel.
fn farthest_ms(p: vec2<u32>) -> f32 {
    var far = 1.0;
    for (var i = 0u; i < textureNumSamples(src_depth_ms); i++) {
        far = min(far, textureLoad(src_depth_ms, p, i32(i)));
    }
    return far;
}

// Level 0 from multisampled depth: the farthest sample of the 2 by 2
// texels.
@compute @workgroup_size(8, 8)
fn cs_first_ms(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let last = textureDimensions(src_depth_ms) - vec2<u32>(1u);
    let p = id.xy * 2u;
    let a = farthest_ms(min(p, last));
    let b = farthest_ms(min(p + vec2<u32>(1u, 0u), last));
    let c = farthest_ms(min(p + vec2<u32>(0u, 1u), last));
    let d = farthest_ms(min(p + vec2<u32>(1u, 1u), last));
    textureStore(dst, id.xy, vec4<f32>(min(min(a, b), min(c, d)), 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_first(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let last = textureDimensions(src_depth) - vec2<u32>(1u);
    let p = id.xy * 2u;
    let a = textureLoad(src_depth, min(p, last), 0);
    let b = textureLoad(src_depth, min(p + vec2<u32>(1u, 0u), last), 0);
    let c = textureLoad(src_depth, min(p + vec2<u32>(0u, 1u), last), 0);
    let d = textureLoad(src_depth, min(p + vec2<u32>(1u, 1u), last), 0);
    textureStore(dst, id.xy, vec4<f32>(min(min(a, b), min(c, d)), 0.0, 0.0, 0.0));
}

@compute @workgroup_size(8, 8)
fn cs_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let src_size = textureDimensions(src);
    let last = src_size - vec2<u32>(1u);
    let p = id.xy * 2u;
    let odd = (src_size & vec2<u32>(1u)) == vec2<u32>(1u);
    let at_end = id.xy == size - vec2<u32>(1u);
    let reach = select(vec2<u32>(1u), vec2<u32>(2u), odd & at_end);
    var far = 1.0;
    for (var y = 0u; y <= reach.y; y++) {
        for (var x = 0u; x <= reach.x; x++) {
            far = min(far, textureLoad(src, min(p + vec2<u32>(x, y), last), 0).r);
        }
    }
    textureStore(dst, id.xy, vec4<f32>(far, 0.0, 0.0, 0.0));
}
