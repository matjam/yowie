// Image-based lighting bakes: a panorama or the procedural sky into a
// radiance cube, its mip chain, the prefiltered specular cube (one mip per
// roughness) and the diffuse irradiance cube. Each pass renders one face
// of one mip with a full-screen triangle.

struct Bake {
    // Face, source mip count, roughness, face size of the source mip 0.
    params: vec4<f32>,
}

@group(1) @binding(0) var<uniform> bake: Bake;
@group(1) @binding(1) var src_cube: texture_cube<f32>;
@group(1) @binding(2) var src_pano: texture_2d<f32>;
@group(1) @binding(3) var src_sampler: sampler;

struct BakeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_bake(@builtin(vertex_index) i: u32) -> BakeOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BakeOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    // u to the right, v down, both -1 to 1.
    o.uv = vec2<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0);
    return o;
}

// The direction through a face texel, in the cube map convention wgpu
// samples with.
fn face_dir(face: u32, uv: vec2<f32>) -> vec3<f32> {
    let u = uv.x;
    let v = uv.y;
    var d: vec3<f32>;
    switch face {
        case 0u: { d = vec3<f32>(1.0, -v, -u); }
        case 1u: { d = vec3<f32>(-1.0, -v, u); }
        case 2u: { d = vec3<f32>(u, 1.0, v); }
        case 3u: { d = vec3<f32>(u, -1.0, -v); }
        case 4u: { d = vec3<f32>(u, -v, 1.0); }
        default: { d = vec3<f32>(-u, -v, -1.0); }
    }
    return normalize(d);
}

fn dir_of(in: BakeOut) -> vec3<f32> {
    return face_dir(u32(bake.params.x), in.uv);
}

@fragment
fn fs_panorama(in: BakeOut) -> @location(0) vec4<f32> {
    let d = dir_of(in);
    let uv = vec2<f32>(atan2(d.x, -d.z) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
    let c = textureSampleLevel(src_pano, src_sampler, uv, 0.0).rgb;
    return vec4<f32>(min(c, vec3<f32>(60000.0)), 1.0);
}

@fragment
fn fs_sky_bake(in: BakeOut) -> @location(0) vec4<f32> {
    return vec4<f32>(sky_color(dir_of(in)), 1.0);
}

@fragment
fn fs_downsample(in: BakeOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSampleLevel(src_cube, src_sampler, dir_of(in), 0.0).rgb, 1.0);
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    var b = i;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return vec2<f32>(f32(i) / f32(n), f32(b) * 2.3283064365386963e-10);
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return mat3x3<f32>(t, b, n);
}

const SPECULAR_SAMPLES: u32 = 256u;

@fragment
fn fs_prefilter(in: BakeOut) -> @location(0) vec4<f32> {
    let n = dir_of(in);
    let roughness = bake.params.z;
    let a = max(roughness * roughness, 1e-4);
    let a2 = a * a;
    let tbn = basis(n);
    let texel_solid = 4.0 * PI / (6.0 * bake.params.w * bake.params.w);
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < SPECULAR_SAMPLES; i++) {
        let xi = hammersley(i, SPECULAR_SAMPLES);
        let phi = 2.0 * PI * xi.x;
        let cos_t = sqrt((1.0 - xi.y) / (1.0 + (a2 - 1.0) * xi.y));
        let sin_t = sqrt(1.0 - cos_t * cos_t);
        let h = tbn * vec3<f32>(cos(phi) * sin_t, sin(phi) * sin_t, cos_t);
        let l = 2.0 * dot(n, h) * h - n;
        let nl = dot(n, l);
        if nl > 0.0 {
            let nh = max(dot(n, h), 0.0);
            let dd = nh * nh * (a2 - 1.0) + 1.0;
            let pdf = a2 / (PI * dd * dd) / 4.0 + 1e-6;
            let sample_solid = 1.0 / (f32(SPECULAR_SAMPLES) * pdf);
            let lod = clamp(0.5 * log2(sample_solid / texel_solid) + 1.0, 0.0, bake.params.y - 1.0);
            sum += textureSampleLevel(src_cube, src_sampler, l, lod).rgb * nl;
            weight += nl;
        }
    }
    return vec4<f32>(sum / max(weight, 1e-4), 1.0);
}

const DIFFUSE_SAMPLES: u32 = 512u;

@fragment
fn fs_irradiance(in: BakeOut) -> @location(0) vec4<f32> {
    let n = dir_of(in);
    let tbn = basis(n);
    let texel_solid = 4.0 * PI / (6.0 * bake.params.w * bake.params.w);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < DIFFUSE_SAMPLES; i++) {
        let xi = hammersley(i, DIFFUSE_SAMPLES);
        let phi = 2.0 * PI * xi.x;
        let cos_t = sqrt(1.0 - xi.y);
        let sin_t = sqrt(xi.y);
        let l = tbn * vec3<f32>(cos(phi) * sin_t, sin(phi) * sin_t, cos_t);
        let pdf = cos_t / PI + 1e-6;
        let lod = clamp(0.5 * log2(1.0 / (f32(DIFFUSE_SAMPLES) * pdf * texel_solid)) + 1.0, 0.0, bake.params.y - 1.0);
        sum += textureSampleLevel(src_cube, src_sampler, l, lod).rgb;
    }
    // Cosine-weighted sampling: the mean is the irradiance divided by pi,
    // which is what the surface program multiplies by the albedo.
    return vec4<f32>(sum / f32(DIFFUSE_SAMPLES), 1.0);
}
