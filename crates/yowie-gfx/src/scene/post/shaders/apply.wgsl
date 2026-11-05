// Applies ambient occlusion and reflections to the scene: occlusion
// darkens the share of each pixel that is ambient or image-based light
// (the surface image's alpha), and a reflection replaces as much of the
// environment's reflection as the surface reflects (the specular image)
// and the trace's fade allows.

@group(1) @binding(0) var scene_tex: texture_2d<f32>;
@group(1) @binding(1) var ao_tex: texture_2d<f32>;
@group(1) @binding(2) var surface_tex: texture_2d<f32>;
@group(1) @binding(3) var specular_tex: texture_2d<f32>;
@group(1) @binding(4) var reflection_tex: texture_2d<f32>;

@fragment
fn fs_apply(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    var c = textureLoad(scene_tex, px, 0);
    if (u32(post.tone.z) & FLAG_AO) != 0u {
        let ambient = textureLoad(surface_tex, px, 0).w;
        let o = mix(1.0, textureLoad(ao_tex, px, 0).r, post.occlusion.x);
        c = vec4<f32>(c.rgb * (1.0 - ambient * (1.0 - o)), c.a);
    }
    let r = textureLoad(reflection_tex, px, 0);
    if r.a > 0.0 {
        let w = textureLoad(specular_tex, px, 0).rgb * post.ssr.x;
        c = vec4<f32>(c.rgb * (1.0 - w * r.a) + r.rgb * w * c.a, c.a);
    }
    return c;
}
