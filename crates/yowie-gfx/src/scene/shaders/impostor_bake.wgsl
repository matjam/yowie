// The impostor bake: the mesh programs with a fragment program that
// writes the surface's base colour and its normal (in the bake camera's
// frame, which is the world's) instead of lighting it. Cut-out and
// blended materials drop pixels below their cutoff (or half alpha).

struct BakeOut {
    @location(0) albedo: vec4<f32>,
    @location(1) normal: vec4<f32>,
}

@fragment
fn fs_impostor_bake(in: VertexOut, @builtin(front_facing) front: bool) -> BakeOut {
    let s = surface(in, front);
    let m = materials[in.material];
    var cutoff = m.params.w;
    if (m.flags.x & MAT_BLEND) != 0u {
        cutoff = 0.5;
    }
    if cutoff >= 0.0 && s.alpha < cutoff {
        discard;
    }
    var o: BakeOut;
    o.albedo = vec4<f32>(s.base + s.emissive, 1.0);
    o.normal = vec4<f32>(normalize(s.normal) * 0.5 + 0.5, 1.0);
    return o;
}
