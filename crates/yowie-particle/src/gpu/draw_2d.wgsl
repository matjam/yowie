// The 2D camera block and vertex program of GPU particles. The block is
// the engine's `YowieView`.

struct YowieView {
    view_proj: mat4x4<f32>,
    view_size: vec2<f32>,
    pixel_scale: f32,
    time: f32,
};

@group(0) @binding(0) var<uniform> view: YowieView;

