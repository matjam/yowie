// Two-phase culling of a static group's items. `cs_early` appends the
// items inside the view that were visible last frame to the early draws;
// after they and the ordinary draws are drawn and the depth pyramid is
// built, `cs_late` tests every item inside the view against the pyramid,
// appends those that became visible to the late draws, and records the
// visibility for next frame. Mirrors `CullUniform` and `CullItem` in
// statics/gpu.rs.

struct Cull {
    view_proj: mat4x4<f32>,
    // Frustum planes, normals pointing in.
    planes: array<vec4<f32>, 6>,
    // Depth target width and height in pixels, pyramid mips, 0.
    size: vec4<f32>,
    // Items, batches, 0, 0.
    counts: vec4<u32>,
}

struct CullItem {
    // The top three rows of the model matrix.
    rows: array<vec4<f32>, 3>,
    // World box.
    bmin: vec4<f32>,
    bmax: vec4<f32>,
    // Material in the group's table, batch, 0, 0.
    info: vec4<u32>,
}

struct Instance {
    rows: array<vec4<f32>, 3>,
    params: vec4<u32>,
}

@group(0) @binding(0) var<uniform> cull: Cull;
@group(0) @binding(1) var<storage, read> items: array<CullItem>;
@group(0) @binding(2) var<storage, read> batch_first: array<u32>;
@group(0) @binding(3) var<storage, read> remap: array<u32>;
@group(0) @binding(4) var<storage, read_write> visibility: array<u32>;
// Early draw arguments (five words a batch), late ones, then the counters:
// outside the view, hidden.
@group(0) @binding(5) var<storage, read_write> args: array<atomic<u32>>;
// Early instances (one region the item count long), then late ones.
@group(0) @binding(6) var<storage, read_write> visible: array<Instance>;
@group(0) @binding(7) var pyramid: texture_2d<f32>;

fn in_view(it: CullItem) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let p = cull.planes[i];
        let far = select(it.bmin.xyz, it.bmax.xyz, p.xyz >= vec3<f32>(0.0));
        if dot(p.xyz, far) + p.w < 0.0 {
            return false;
        }
    }
    return true;
}

// Appends item i to the draws of phase `late`.
fn emit(i: u32, late: bool) {
    let it = items[i];
    let b = it.info.y;
    let base = select(0u, cull.counts.y * 5u, late);
    let slot = atomicAdd(&args[base + b * 5u + 1u], 1u);
    let region = select(0u, cull.counts.x, late);
    let material = remap[it.info.x];
    visible[region + batch_first[b] + slot] = Instance(
        it.rows,
        vec4<u32>(material, 0xffffffffu, 0u, 0u),
    );
}

// Whether the box is hidden behind the depth the pyramid holds: its
// nearest point is farther than the farthest depth over the pixels it
// covers. Depth is reversed (1 near, 0 far); level j's texel x holds the
// farthest depth of pixels [x 2^(j+1), (x + 1) 2^(j+1)).
fn hidden(it: CullItem) -> bool {
    let mips = u32(cull.size.z);
    if mips == 0u {
        return false;
    }
    var lo = vec2<f32>(1e30);
    var hi = vec2<f32>(-1e30);
    var nearest = 0.0;
    for (var c = 0u; c < 8u; c++) {
        let p = vec3<f32>(
            select(it.bmin.x, it.bmax.x, (c & 1u) != 0u),
            select(it.bmin.y, it.bmax.y, (c & 2u) != 0u),
            select(it.bmin.z, it.bmax.z, (c & 4u) != 0u),
        );
        let clip = cull.view_proj * vec4<f32>(p, 1.0);
        if clip.w <= 1e-5 {
            // The box reaches behind the eye.
            return false;
        }
        let ndc = clip.xyz / clip.w;
        lo = min(lo, ndc.xy);
        hi = max(hi, ndc.xy);
        nearest = max(nearest, ndc.z);
    }
    if nearest >= 1.0 {
        return false;
    }
    let w = cull.size.x;
    let h = cull.size.y;
    // Pixel rows run down while clip y runs up.
    let x0 = clamp((lo.x * 0.5 + 0.5) * w, 0.0, w - 1.0);
    let x1 = clamp((hi.x * 0.5 + 0.5) * w, 0.0, w - 1.0);
    let y0 = clamp((0.5 - hi.y * 0.5) * h, 0.0, h - 1.0);
    let y1 = clamp((0.5 - lo.y * 0.5) * h, 0.0, h - 1.0);
    let p0 = vec2<u32>(u32(x0), u32(y0));
    let p1 = vec2<u32>(u32(x1), u32(y1));
    var level = 0u;
    loop {
        let t0 = p0 >> vec2<u32>(level + 1u);
        let t1 = p1 >> vec2<u32>(level + 1u);
        if (t1.x - t0.x <= 1u && t1.y - t0.y <= 1u) || level + 1u >= mips {
            break;
        }
        level += 1u;
    }
    let dims = textureDimensions(pyramid, level) - vec2<u32>(1u);
    let t0 = min(p0 >> vec2<u32>(level + 1u), dims);
    let t1 = min(p1 >> vec2<u32>(level + 1u), dims);
    let a = textureLoad(pyramid, t0, level).r;
    let b = textureLoad(pyramid, vec2<u32>(t1.x, t0.y), level).r;
    let c = textureLoad(pyramid, vec2<u32>(t0.x, t1.y), level).r;
    let d = textureLoad(pyramid, t1, level).r;
    return nearest < min(min(a, b), min(c, d));
}

@compute @workgroup_size(64)
fn cs_early(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= cull.counts.x {
        return;
    }
    if visibility[i] != 0u && in_view(items[i]) {
        emit(i, false);
    }
}

@compute @workgroup_size(64)
fn cs_late(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= cull.counts.x {
        return;
    }
    let counters = cull.counts.y * 10u;
    let it = items[i];
    if !in_view(it) {
        visibility[i] = 0u;
        atomicAdd(&args[counters], 1u);
        return;
    }
    if hidden(it) {
        visibility[i] = 0u;
        atomicAdd(&args[counters + 1u], 1u);
        return;
    }
    if visibility[i] == 0u {
        emit(i, true);
    }
    visibility[i] = 1u;
}
