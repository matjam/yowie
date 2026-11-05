// Billboards and impostors: six vertices per instance build a quad that
// faces the viewer, either fully (towards the view plane) or about the
// up axis only. An impostor instance also picks the view of its atlas
// nearest the direction it is seen from, and its normal map turns the
// baked normals of that view back into the world. Mirrors `billboard.rs`
// and `billboard_render.rs`.

struct Sheet {
    // Views, columns, rows and pitch of an impostor atlas.
    grid: vec4<f32>,
    // Alpha cutoff (negative for none), roughness, 1 when lit, 1 when
    // there is a normal map.
    params: vec4<f32>,
}

@group(1) @binding(32) var sheet_albedo: texture_2d<f32>;
@group(1) @binding(33) var sheet_normal: texture_2d<f32>;
@group(1) @binding(34) var sheet_sampler: sampler;
@group(1) @binding(35) var<uniform> sheet: Sheet;

const BB_UPRIGHT: u32 = 1u;
const BB_IMPOSTOR: u32 = 2u;

struct BillboardIn {
    @builtin(vertex_index) vertex: u32,
    @location(0) position: vec3<f32>,
    @location(1) flags: u32,
    @location(2) size_pivot: vec4<f32>,
    @location(3) uv: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) fade_yaw: vec2<f32>,
    @location(6) view: vec2<u32>,
}

struct BillboardOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) right: vec3<f32>,
    @location(4) up: vec3<f32>,
    @location(5) toward: vec3<f32>,
    @location(6) @interpolate(flat) fade: f32,
}

struct SheetCorner {
    world: vec3<f32>,
    uv: vec2<f32>,
    // The frame the texture's normals are in.
    right: vec3<f32>,
    up: vec3<f32>,
    toward: vec3<f32>,
}

// The camera's right, up and backward directions in the world.
fn camera_axes() -> mat3x3<f32> {
    let v = frame.view;
    return mat3x3<f32>(
        vec3<f32>(v[0].x, v[1].x, v[2].x),
        vec3<f32>(v[0].y, v[1].y, v[2].y),
        vec3<f32>(v[0].z, v[1].z, v[2].z),
    );
}

// Places corner `vertex` of a billboard seen along `to_viewer` (from the
// billboard towards the viewer).
fn place_billboard(b: BillboardIn, to_viewer: vec3<f32>) -> SheetCorner {
    let cam = camera_axes();
    var right = cam[0];
    var up = cam[1];
    var toward = cam[2];
    if (b.flags & (BB_UPRIGHT | BB_IMPOSTOR)) != 0u {
        var t = vec3<f32>(to_viewer.x, 0.0, to_viewer.z);
        if dot(t, t) < 1e-8 {
            t = vec3<f32>(cam[2].x, 0.0, cam[2].z);
        }
        if dot(t, t) < 1e-8 {
            t = vec3<f32>(0.0, 0.0, 1.0);
        }
        toward = normalize(t);
        right = vec3<f32>(toward.z, 0.0, -toward.x);
        up = vec3<f32>(0.0, 1.0, 0.0);
    }
    // Two triangles: (0,0) (1,0) (1,1), (0,0) (1,1) (0,1).
    let k = b.vertex % 6u;
    let cx = select(0.0, 1.0, k == 1u || k == 2u || k == 4u);
    let cy = select(0.0, 1.0, k == 2u || k == 4u || k == 5u);
    let size = b.size_pivot.xy;
    let pivot = b.size_pivot.zw;
    var p: SheetCorner;
    p.world = b.position + right * (cx - pivot.x) * size.x + up * (cy - pivot.y) * size.y;
    var uv0 = b.uv.xy;
    var uv1 = b.uv.zw;
    p.right = right;
    p.up = up;
    p.toward = toward;
    if (b.flags & BB_IMPOSTOR) != 0u {
        // The view whose direction is nearest the viewer's, and the frame
        // its normals were baked in.
        let views = max(sheet.grid.x, 1.0);
        let step = 6.2831853 / views;
        let theta = atan2(toward.x, toward.z) - b.fade_yaw.y;
        let n = u32(views);
        let k = u32(i32(round(theta / step)) % i32(n) + i32(n)) % n;
        let cols = u32(max(sheet.grid.y, 1.0));
        let cell = vec2<f32>(f32(k % cols), f32(k / cols));
        let scale = vec2<f32>(1.0 / max(sheet.grid.y, 1.0), 1.0 / max(sheet.grid.z, 1.0));
        uv0 = cell * scale;
        uv1 = uv0 + scale;
        let a = f32(k) * step + b.fade_yaw.y;
        let pitch = sheet.grid.w;
        p.toward = vec3<f32>(sin(a) * cos(pitch), sin(pitch), cos(a) * cos(pitch));
        p.right = vec3<f32>(cos(a), 0.0, -sin(a));
        p.up = cross(p.toward, p.right);
    }
    p.uv = vec2<f32>(mix(uv0.x, uv1.x, cx), mix(uv1.y, uv0.y, cy));
    return p;
}

@vertex
fn vs_billboard(b: BillboardIn) -> BillboardOut {
    let p = place_billboard(b, frame.camera.xyz - b.position);
    var o: BillboardOut;
    o.clip = frame.view_proj * vec4<f32>(p.world, 1.0);
    o.world = p.world;
    o.uv = p.uv;
    o.color = b.color;
    o.right = p.right;
    o.up = p.up;
    o.toward = p.toward;
    o.fade = b.fade_yaw.x;
    return o;
}

struct BillboardShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
}

@vertex
fn vs_billboard_shadow(b: BillboardIn) -> BillboardShadowOut {
    // Sun cascades see upright quads turned towards the sun, so the
    // shadow is the full silhouette; other maps see them as the camera
    // does.
    var to_viewer = frame.camera.xyz - b.position;
    if b.view.x < 3u && frame.sun_dir.w > 0.5 {
        to_viewer = frame.sun_dir.xyz;
    }
    let p = place_billboard(b, to_viewer);
    var o: BillboardShadowOut;
    o.clip = frame.shadow_views[b.view.x].view_proj * vec4<f32>(p.world, 1.0);
    o.uv = p.uv;
    o.alpha = b.color.a;
    return o;
}

@fragment
fn fs_billboard_shadow(in: BillboardShadowOut) {
    let a = textureSample(sheet_albedo, sheet_sampler, in.uv).a * in.alpha;
    if a < max(sheet.params.x, 0.0) {
        discard;
    }
}

// The surface of a billboard fragment, unlit when the sheet is; its
// alpha is the coverage.
fn billboard_surface(in: BillboardOut, front: bool) -> Surface {
    let texel = textureSample(sheet_albedo, sheet_sampler, in.uv);
    let nt = textureSample(sheet_normal, sheet_sampler, in.uv);
    let base = texel.rgb / max(texel.a, 1e-4) * in.color.rgb;
    var n = normalize(in.toward);
    if sheet.params.w > 0.5 {
        // Normal maps are stored premultiplied by coverage like colour.
        let t = nt.xyz / max(nt.a, 1e-4) * 2.0 - 1.0;
        n = normalize(in.right * t.x + in.up * t.y + in.toward * t.z);
    }
    if !front {
        n = -n;
    }
    var s = plain_surface(base, n, sheet.params.y, in.world);
    s.alpha = texel.a * in.color.a;
    s.unlit = sheet.params.z < 0.5;
    return s;
}

// The lit colour and alpha of a billboard fragment.
fn billboard_color(in: BillboardOut, front: bool) -> vec4<f32> {
    let s = billboard_surface(in, front);
    let c = shade(s, in.clip, view_depth_of(in.world));
    return vec4<f32>(apply_fog(c, in.world), s.alpha);
}

@fragment
fn fs_billboard_cutout(in: BillboardOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = billboard_color(in, front);
    if c.a < sheet.params.x || dither_threshold(in.clip.xy) >= in.fade {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}

// `fs_billboard_cutout` with the surface images the post chain reads.
@fragment
fn fs_billboard_cutout_post(in: BillboardOut, @builtin(front_facing) front: bool) -> PostOut {
    let s = billboard_surface(in, front);
    var out = post_out(s, shade(s, in.clip, view_depth_of(in.world)), in.world);
    if out.color.a < sheet.params.x || dither_threshold(in.clip.xy) >= in.fade {
        discard;
    }
    out.color.a = 1.0;
    return out;
}

@fragment
fn fs_billboard_blend(in: BillboardOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = billboard_color(in, front);
    let a = clamp(c.a * in.fade, 0.0, 1.0);
    return vec4<f32>(c.rgb * a, a);
}
