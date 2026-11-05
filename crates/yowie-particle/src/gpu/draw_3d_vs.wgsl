
// The simulation runs in a plane (origin, x axis, y axis) placed in the
// world; each particle is a quad facing the camera at its place on it.
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
    let right = vec3<f32>(frame.view[0][0], frame.view[1][0], frame.view[2][0]);
    let up = vec3<f32>(frame.view[0][1], frame.view[1][1], frame.view[2][1]);
    let centre = lut.plane_origin.xyz + lut.plane_x.xyz * q.center.x + lut.plane_y.xyz * q.center.y;
    // Quad v grows downward, camera up grows upward.
    let world = centre + right * q.corner.x - up * q.corner.y;
    out.clip = frame.view_proj * vec4<f32>(world, 1.0);
    out.uv = q.uv;
    out.color = q.color;
    return out;
}
