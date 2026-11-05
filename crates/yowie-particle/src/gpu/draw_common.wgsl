// The part of the GPU particle draw shared by the 2D and 3D passes. The
// camera block at group 0 and the vertex entry point come from draw_2d.wgsl
// or draw_3d.wgsl, which are placed before and after this text.
//
// One instanced draw covers every live particle: instance `i` is the
// i-th entry of the alive list, expanded to a quad from six vertices.

struct Particle {
    pos: vec2<f32>,
    vel: vec2<f32>,
    origin: vec2<f32>,
    age: f32,
    life: f32,
    size: f32,
    rot: f32,
    spin: f32,
    tint: u32,
};

struct Lut {
    offset: vec2<f32>,
    aspect: f32,
    frame_count: u32,
    capacity: u32,
    premult: u32,
    pad0: u32,
    pad1: u32,
    plane_origin: vec4<f32>,
    plane_x: vec4<f32>,
    plane_y: vec4<f32>,
    color: array<vec4<f32>, 128>,
    curve: array<vec4<f32>, 128>,
    frames: array<vec4<f32>, 64>,
};

@group(1) @binding(0) var<uniform> lut: Lut;
@group(1) @binding(1) var<storage, read> particles: array<Particle>;
@group(1) @binding(2) var<storage, read> slots: array<u32>;
@group(1) @binding(3) var<storage, read> header: array<u32>;
@group(1) @binding(4) var tex: texture_2d<f32>;
@group(1) @binding(5) var samp: sampler;

struct Quad {
    alive: bool,
    // Particle centre in the emitter's plane, before the quad's offset.
    center: vec2<f32>,
    // Offset of this vertex from the centre, rotated, in the plane's units.
    corner: vec2<f32>,
    uv: vec2<f32>,
    // Linear light, premultiplied unless the blend takes the tint as is.
    color: vec4<f32>,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn fetch(vi: u32, ii: u32) -> Quad {
    var q: Quad;
    q.alive = false;
    if (ii >= header[1]) {
        return q;
    }
    let p = particles[slots[lut.capacity + ii]];
    let t = clamp(p.age / max(p.life, 1e-6), 0.0, 1.0);
    let f = t * 127.0;
    let i0 = u32(floor(f));
    let i1 = min(i0 + 1u, 127u);
    let w = f - floor(f);
    var col = mix(lut.color[i0], lut.color[i1], w) * unpack4x8unorm(p.tint);
    let k = mix(lut.curve[i0], lut.curve[i1], w);
    col.a = col.a * k.z;
    let width = p.size * k.x;
    if (width <= 0.0 || col.a <= 0.0) {
        return q;
    }
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5));
    let c = corners[vi % 6u];
    let local = c * vec2<f32>(width, width * lut.aspect);
    let r = vec2<f32>(cos(p.rot), sin(p.rot));
    q.corner = vec2<f32>(local.x * r.x - local.y * r.y, local.x * r.y + local.y * r.x);
    q.center = p.pos + lut.offset;
    q.uv = c + vec2<f32>(0.5);
    if (lut.frame_count > 0u) {
        let slot = min(u32(k.w * f32(lut.frame_count)), lut.frame_count - 1u);
        let rect = lut.frames[min(slot, 63u)];
        q.uv = mix(rect.xy, rect.zw, q.uv);
    }
    let lin = srgb_to_linear(col.rgb);
    q.color = vec4<f32>(select(lin, lin * col.a, lut.premult == 1u), col.a);
    q.alive = true;
    return q;
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.color;
}
