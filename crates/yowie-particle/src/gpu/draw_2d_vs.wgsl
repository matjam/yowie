
@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VertexOut {
    var out: VertexOut;
    let q = fetch(vi, ii);
    if (!q.alive) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.uv = vec2<f32>(0.0);
        out.color = vec4<f32>(0.0);
        return out;
    }
    out.clip = view.view_proj * vec4<f32>(q.center + q.corner, 0.0, 1.0);
    out.uv = q.uv;
    out.color = q.color;
    return out;
}
