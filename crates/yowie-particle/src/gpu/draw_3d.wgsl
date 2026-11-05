// The 3D camera block of GPU particles: the first fields of the scene's
// frame block, which group 0 is bound to.

struct Frame {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

