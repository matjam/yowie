// Screen-space reflections: each surface pixel smooth enough to reflect
// sends its reflected ray (normal and roughness from the surface image)
// through the nearest-depth pyramid. The ray is a line in texture
// coordinates and stored depth, both linear on screen; the march steps
// cell to cell, climbing a mip while the ray stays in front of a cell's
// nearest depth and descending where it might hit (the hierarchical
// traversal of AMD FidelityFX SSSR). A hit must lie within a thickness of
// the surface behind it. The output is the colour at the hit times the
// fades (screen edge, distance, rays turning back to the eye, roughness),
// with the fade in alpha. A miss writes zero and the surface keeps the
// environment reflection it was shaded with.

@group(1) @binding(0) var depth_tex: texture_2d<f32>;
@group(1) @binding(1) var surface_tex: texture_2d<f32>;
@group(1) @binding(2) var scene_tex: texture_2d<f32>;

fn mip_res(mip: i32) -> vec2<f32> {
    return vec2<f32>(textureDimensions(depth_tex, mip));
}

fn nearest_depth(cell: vec2<f32>, mip: i32) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_tex, mip));
    return textureLoad(depth_tex, clamp(vec2<i32>(cell), vec2<i32>(0), size - 1), mip).r;
}

// Marches from `origin` along `dir` (texture coordinates and depth; the
// ray's end is origin + dir). Returns the stop point and, in w, the ray
// parameter there, negative for a miss.
fn trace(origin: vec3<f32>, dir: vec3<f32>, max_steps: i32) -> vec4<f32> {
    let big = 1e30;
    let inv = vec3<f32>(
        select(big, 1.0 / dir.x, dir.x != 0.0),
        select(big, 1.0 / dir.y, dir.y != 0.0),
        select(big, 1.0 / dir.z, dir.z != 0.0),
    );
    let max_mip = i32(textureNumLevels(depth_tex)) - 1;
    let neg = dir.xy < vec2<f32>(0.0);
    let floor_off = select(vec2<f32>(1.0), vec2<f32>(0.0), neg);
    let nudge = 0.005 * post.size.zw;
    let uv_off = select(nudge, -nudge, neg);
    var mip = 0;
    var res = mip_res(0);
    let plane0 = (floor(res * origin.xy) + floor_off) / res + uv_off;
    let t0 = (plane0 - origin.xy) * inv.xy;
    var t = min(t0.x, t0.y);
    var pos = origin + t * dir;
    var i = 0;
    while i < max_steps && mip >= 0 {
        if t > 1.0 || any(pos.xy < vec2<f32>(0.0)) || any(pos.xy > vec2<f32>(1.0)) {
            return vec4<f32>(pos, -1.0);
        }
        let cell = res * pos.xy;
        let surface = nearest_depth(cell, mip);
        let plane = (floor(cell) + floor_off) / res + uv_off;
        var tt = vec3<f32>((plane - origin.xy) * inv.xy, (surface - origin.z) * inv.z);
        // The depth plane counts only for a ray going away from the eye
        // (depth falling, being reversed).
        if dir.z >= 0.0 {
            tt.z = big;
        }
        let t_min = min(min(tt.x, tt.y), tt.z);
        let above = pos.z > surface;
        let skipped = above && t_min != tt.z;
        if above {
            t = t_min;
            pos = origin + t * dir;
        }
        if skipped {
            mip = min(mip + 1, max_mip);
        } else {
            mip -= 1;
        }
        res = mip_res(max(mip, 0));
        i++;
    }
    if mip >= 0 || t > 1.0 {
        return vec4<f32>(pos, -1.0);
    }
    return vec4<f32>(pos, t);
}

@fragment
fn fs_ssr(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.clip.xy);
    let d = textureLoad(depth_tex, px, 0).r;
    if d <= 0.0 {
        return vec4<f32>(0.0);
    }
    let surf = textureLoad(surface_tex, px, 0);
    let max_rough = post.ssr.y;
    let rough_fade = 1.0 - smoothstep(max_rough * 0.5, max_rough, surf.z);
    if rough_fade <= 0.0 {
        return vec4<f32>(0.0);
    }
    let uv = in.clip.xy * post.size.zw;
    let p = view_pos(uv, d);
    let n = normalize(to_view(oct_decode(surf.xy)));
    let v = to_eye(p);
    let r = reflect(-v, n);
    let dist = max(post.ssr.z, 1e-3);
    // Start a little off the surface so the ray does not find it again.
    let start = p + n * max(-p.z, 1.0) * 0.002;
    let c0 = post.proj * vec4<f32>(start, 1.0);
    var c1 = post.proj * vec4<f32>(start + r * dist, 1.0);
    // Cut the ray at the near plane (clip z = w with reversed depth).
    let e0 = c0.z - c0.w;
    let e1 = c1.z - c1.w;
    if e1 > 0.0 {
        c1 = mix(c0, c1, 0.999 * e0 / (e0 - e1));
    }
    if c1.w <= 0.0 || c0.w <= 0.0 {
        return vec4<f32>(0.0);
    }
    let s0 = vec3<f32>(ndc_to_uv(c0.xy / c0.w), c0.z / c0.w);
    let s1 = vec3<f32>(ndc_to_uv(c1.xy / c1.w), c1.z / c1.w);
    let hit = trace(s0, s1 - s0, i32(post.ssr.w));
    if hit.w < 0.0 {
        return vec4<f32>(0.0);
    }
    let huv = hit.xy;
    let hd = nearest_depth(huv * post.size.xy, 0);
    if hd <= 0.0 {
        return vec4<f32>(0.0);
    }
    // The ray must not have passed far behind what it hit.
    let ray_z = view_distance(huv, hit.z);
    let scene_z = view_distance(huv, hd);
    let thickness = dist / max(post.ssr.w, 1.0) * 2.0 + 0.05 + scene_z * 0.02;
    if ray_z - scene_z > thickness {
        return vec4<f32>(0.0);
    }
    let travelled = distance(start, view_pos(huv, hd));
    let edge = smoothstep(vec2<f32>(0.0), vec2<f32>(0.1), huv)
        * (1.0 - smoothstep(vec2<f32>(0.9), vec2<f32>(1.0), huv));
    var fade = edge.x * edge.y * rough_fade;
    fade *= 1.0 - smoothstep(0.6, 1.0, travelled / dist);
    fade *= clamp(1.0 - dot(r, v), 0.0, 1.0);
    fade = clamp(fade, 0.0, 1.0);
    let color = textureSampleLevel(scene_tex, linear_clamp, huv, 0.0);
    return vec4<f32>(color.rgb / max(color.a, 1e-4) * fade, fade);
}
