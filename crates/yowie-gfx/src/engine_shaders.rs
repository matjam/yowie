//! The engine's own fragment variants: signed-distance-field text and
//! radial gradients. Each is the sprite prelude plus one fragment function,
//! built as a trusted module like the default shader.

use crate::batch::{RADIAL_SHADER, SDF_SHADER};
use crate::pipelines::trusted_module;
use crate::shader::compose;

/// Draws text from a distance field: the red channel holds the signed
/// distance to the glyph outline, 0.5 on the outline, and the edge is
/// smoothed over one pixel of the output however large the glyph is drawn.
const SDF_FRAGMENT: &str = "fn fragment(input: SpriteInput) -> vec4<f32> {
    let d = textureSample(yowie_texture, yowie_sampler, input.uv).r;
    let w = max(fwidth(d) * 0.75, 0.0005);
    let a = smoothstep(0.5 - w, 0.5 + w, d);
    return input.color * a;
}
";

/// Draws a radial gradient: the vertex texture coordinate is the offset
/// from the centre in radii, and the distance indexes a colour ramp.
const RADIAL_FRAGMENT: &str = "fn fragment(input: SpriteInput) -> vec4<f32> {
    let t = length(input.uv);
    return textureSample(yowie_texture, yowie_sampler, vec2<f32>(t, 0.5)) * input.color;
}
";

/// Pipeline ids the engine variants use; game shader ids count up from 1.
const SDF_ID: u64 = u64::MAX - 1;
const RADIAL_ID: u64 = u64::MAX - 2;

/// The compiled engine variants.
#[derive(Debug)]
pub(crate) struct EngineShaders {
    sdf: wgpu::ShaderModule,
    radial: wgpu::ShaderModule,
}

impl EngineShaders {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            sdf: trusted_module(device, "yowie sdf text", &compose(SDF_FRAGMENT)),
            radial: trusted_module(device, "yowie radial gradient", &compose(RADIAL_FRAGMENT)),
        }
    }

    /// The pipeline id, generation, module and uniform size of the engine
    /// shader in `slot`.
    pub(crate) fn module(&self, slot: u32) -> (u64, u32, &wgpu::ShaderModule, u64) {
        match slot {
            SDF_SHADER => (SDF_ID, 0, &self.sdf, 0),
            RADIAL_SHADER => (RADIAL_ID, 0, &self.radial, 0),
            _ => (SDF_ID, 0, &self.sdf, 0),
        }
    }
}
