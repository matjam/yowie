// The mesh vertex programs: morph targets, then skinning, then the
// instance's model matrix. The lit programs write the varyings the
// surface program reads; the shadow programs project with the shadow view
// the instance names.

@group(2) @binding(0) var<storage, read> morph: MorphData;

struct VertexIn {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) color: vec4<f32>,
    @location(6) row0: vec4<f32>,
    @location(7) row1: vec4<f32>,
    @location(8) row2: vec4<f32>,
    // Material, first joint (or 0xffffffff), first morph weight,
    // morph weight count in the low byte and shadow view above it.
    @location(9) params: vec4<u32>,
}

struct SkinIn {
    @location(10) joints: vec4<u32>,
    @location(11) weights: vec4<f32>,
}

struct VertexOut {
    // Invariant, so the x-ray pass's depth matches the opaque pass's
    // exactly where a mesh covers itself.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) color: vec4<f32>,
    @location(6) @interpolate(flat) material: u32,
    @location(7) view_depth: f32,
    // A fur shell's height from 0 (the surface) to 1 (the outermost).
    @location(8) @interpolate(flat) shell: f32,
}

struct Placed {
    position: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec3<f32>,
}

// Adds the instance's weighted morph target deltas.
fn apply_morph(v: VertexIn, p: ptr<function, Placed>) {
    let count = v.params.w & 0xffu;
    for (var i = 0u; i < count; i++) {
        let mw = morph_weights[v.params.z + i];
        let base = (mw.target_index * morph.vertex_count + v.vertex_index) * 6u;
        // A mesh updated to more vertices than its targets cover reads
        // nothing past the deltas (the module has no bounds checks).
        if base + 6u > arrayLength(&morph.deltas) {
            break;
        }
        (*p).position += vec3<f32>(morph.deltas[base], morph.deltas[base + 1u], morph.deltas[base + 2u]) * mw.weight;
        (*p).normal += vec3<f32>(morph.deltas[base + 3u], morph.deltas[base + 4u], morph.deltas[base + 5u]) * mw.weight;
    }
}

// The module is built without runtime bounds checks, and joint numbers
// come from the game's vertex data, so each is clamped to the table.
fn joint(i: u32) -> mat4x4<f32> {
    return joints[min(i, arrayLength(&joints) - 1u)];
}

fn skin_matrix(first: u32, s: SkinIn) -> mat4x4<f32> {
    return joint(first + s.joints.x) * s.weights.x
        + joint(first + s.joints.y) * s.weights.y
        + joint(first + s.joints.z) * s.weights.z
        + joint(first + s.joints.w) * s.weights.w;
}

fn model_matrix(v: VertexIn) -> mat4x4<f32> {
    return transpose(mat4x4<f32>(v.row0, v.row1, v.row2, vec4<f32>(0.0, 0.0, 0.0, 1.0)));
}

// The inverse transpose of m's upper 3x3 up to a positive scale, which
// carries normals through non-uniform scale and mirroring.
fn cofactor(m: mat4x4<f32>) -> mat3x3<f32> {
    let a = m[0].xyz;
    let b = m[1].xyz;
    let c = m[2].xyz;
    let bc = cross(b, c);
    let s = select(1.0, -1.0, dot(a, bc) < 0.0);
    return mat3x3<f32>(bc * s, cross(c, a) * s, cross(a, b) * s);
}

fn place(v: VertexIn, skin: mat4x4<f32>) -> Placed {
    var p = Placed(v.position, v.normal, v.tangent.xyz);
    apply_morph(v, &p);
    let m = model_matrix(v) * skin;
    var out: Placed;
    out.position = (m * vec4<f32>(p.position, 1.0)).xyz;
    out.normal = cofactor(m) * p.normal;
    out.tangent = (m * vec4<f32>(p.tangent, 0.0)).xyz;
    return out;
}

fn lit_out(v: VertexIn, p: Placed) -> VertexOut {
    var o: VertexOut;
    o.world = p.position;
    o.clip = frame.view_proj * vec4<f32>(p.position, 1.0);
    o.normal = p.normal;
    o.tangent = vec4<f32>(p.tangent, v.tangent.w);
    o.uv = v.uv;
    o.uv2 = v.uv2;
    o.color = v.color;
    o.material = v.params.x;
    o.view_depth = -(frame.view * vec4<f32>(p.position, 1.0)).z;
    return o;
}

@vertex
fn vs_static(v: VertexIn) -> VertexOut {
    return lit_out(v, place(v, mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0))));
}

@vertex
fn vs_skinned(v: VertexIn, s: SkinIn) -> VertexOut {
    return lit_out(v, place(v, skin_matrix(v.params.y, s)));
}

struct ShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) uv2: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) material: u32,
}

fn shadow_out(v: VertexIn, p: Placed) -> ShadowOut {
    let view = frame.shadow_views[v.params.w >> 8u];
    var o: ShadowOut;
    o.clip = view.view_proj * vec4<f32>(p.position, 1.0);
    o.uv = v.uv;
    o.uv2 = v.uv2;
    o.color = v.color;
    o.material = v.params.x;
    return o;
}

@vertex
fn vs_shadow_static(v: VertexIn) -> ShadowOut {
    return shadow_out(v, place(v, mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0))));
}

@vertex
fn vs_shadow_skinned(v: VertexIn, s: SkinIn) -> ShadowOut {
    return shadow_out(v, place(v, skin_matrix(v.params.y, s)));
}
