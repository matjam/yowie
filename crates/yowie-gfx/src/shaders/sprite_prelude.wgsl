// The engine's half of every 2D shader. A game's fragment source is
// appended after this text; it defines `fn fragment(input: SpriteInput) ->
// vec4<f32>` and may declare a uniform block at group 2, binding 0.

struct YowieView {
    view_proj: mat4x4<f32>,
    view_size: vec2<f32>,
    pixel_scale: f32,
    time: f32,
};

@group(0) @binding(0) var yowie_texture: texture_2d<f32>;
@group(0) @binding(1) var yowie_sampler: sampler;
@group(1) @binding(0) var<uniform> yowie_view: YowieView;

// What the fragment function receives for each pixel.
struct SpriteInput {
    // Texture coordinate, 0 to 1 across the texture.
    uv: vec2<f32>,
    // The tint in linear light, premultiplied by its alpha.
    color: vec4<f32>,
    // The position in drawing coordinates: world units under a camera,
    // view units otherwise.
    world: vec2<f32>,
    // Seconds of game time.
    time: f32,
    // The size of the current view in view units.
    view_size: vec2<f32>,
    // Output pixels per view unit.
    pixel_scale: f32,
};

var<private> yowie_mask: bool;

// Samples the drawn texture at uv, premultiplied and linear. Glyph masks
// come back as white with the coverage in every channel.
fn sample_texture(uv: vec2<f32>) -> vec4<f32> {
    let t = textureSample(yowie_texture, yowie_sampler, uv);
    return select(t, vec4<f32>(t.r), yowie_mask);
}

struct YowieVertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) mode: u32,
};

struct YowieVertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec2<f32>,
    @location(3) @interpolate(flat) mode: u32,
};

@vertex
fn yowie_vs(v: YowieVertexIn) -> YowieVertexOut {
    var out: YowieVertexOut;
    out.clip = yowie_view.view_proj * vec4<f32>(v.position, 0.0, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    out.world = v.position;
    out.mode = v.mode;
    return out;
}

@fragment
fn yowie_fs(in: YowieVertexOut) -> @location(0) vec4<f32> {
    yowie_mask = in.mode == 1u;
    return fragment(SpriteInput(
        in.uv,
        in.color,
        in.world,
        yowie_view.time,
        yowie_view.view_size,
        yowie_view.pixel_scale,
    ));
}

// ---- game source follows ----
