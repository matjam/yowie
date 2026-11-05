//! The opaque scene copy transmissive materials refract: the HDR colour
//! after the opaque meshes and the sky, copied into a texture whose mips
//! are successively blurred halves, so rough glass reads a blurrier level
//! (`transmission_light` in `shaders/layers.wgsl`). The copy reads the
//! single-sample colour, which a multisampled scene resolves into at the
//! end of the pass before the copy.

use yowie_core::UVec2;

use super::layouts::{HDR_FORMAT, SceneLayouts};
use crate::pipelines::trusted_module;

const COPY: &str = "
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn fs_copy(in: Out) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, source_sampler, in.uv, 0.0);
}

// Each level from the one above: four bilinear taps a texel apart, a 4x4
// tent over the source, which blurs as it halves.
@fragment
fn fs_down(in: Out) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source, 0));
    var c = textureSampleLevel(source, source_sampler, in.uv + texel * vec2<f32>(-1.0, -1.0), 0.0);
    c += textureSampleLevel(source, source_sampler, in.uv + texel * vec2<f32>(1.0, -1.0), 0.0);
    c += textureSampleLevel(source, source_sampler, in.uv + texel * vec2<f32>(-1.0, 1.0), 0.0);
    c += textureSampleLevel(source, source_sampler, in.uv + texel * vec2<f32>(1.0, 1.0), 0.0);
    return c * 0.25;
}
";

/// The copy pipelines: level 0 from the scene, each further level from
/// the one above.
#[derive(Debug)]
pub(crate) struct CopyPipelines {
    copy: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
}

impl CopyPipelines {
    pub(crate) fn new(device: &wgpu::Device, layouts: &SceneLayouts) -> Self {
        let module = trusted_module(device, "yowie scene copy", COPY);
        let make = |entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("yowie scene copy"),
                layout: Some(&layouts.copy),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(HDR_FORMAT.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            copy: make("fs_copy"),
            down: make("fs_down"),
        }
    }
}

/// A scene slot's copy texture with a view and a source bind group per
/// level.
#[derive(Debug)]
pub(crate) struct SceneCopy {
    /// The id of the scene targets whose colour the copy reads.
    pub(crate) source: u64,
    pub(crate) id: u64,
    /// Every level, for sampling.
    pub(crate) view: wgpu::TextureView,
    levels: Vec<wgpu::TextureView>,
    /// Binds level `i` as the source of level `i + 1`.
    sources: Vec<wgpu::BindGroup>,
    /// Binds the scene's colour as the source of level 0.
    scene: wgpu::BindGroup,
}

/// The mip count of the copy: halving down to about 8 pixels.
fn level_count(size: UVec2) -> u32 {
    let largest = size.x.max(size.y).max(1);
    (32 - largest.leading_zeros()).saturating_sub(3).max(1)
}

impl SceneCopy {
    pub(crate) fn new(
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        sampler: &wgpu::Sampler,
        (source, scene_color): (u64, &wgpu::TextureView),
        size: UVec2,
    ) -> Self {
        let mips = level_count(size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie scene copy"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels: Vec<wgpu::TextureView> = (0..mips)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("yowie scene copy level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie scene copy source"),
                layout: &layouts.sampled,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let sources = levels[..levels.len() - 1].iter().map(group).collect();
        Self {
            source,
            id: crate::texture::next_id(),
            view: texture.create_view(&Default::default()),
            scene: group(scene_color),
            levels,
            sources,
        }
    }

    /// Records the copy of the scene's colour and the blurred levels.
    pub(crate) fn record(&self, pipelines: &CopyPipelines, encoder: &mut wgpu::CommandEncoder) {
        for (level, target) in self.levels.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yowie scene copy"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if level == 0 {
                pass.set_pipeline(&pipelines.copy);
                pass.set_bind_group(0, &self.scene, &[]);
            } else {
                pass.set_pipeline(&pipelines.down);
                pass.set_bind_group(0, &self.sources[level - 1], &[]);
            }
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_halve_to_about_eight_pixels() {
        let cases = [(1, 1), (8, 1), (16, 2), (1920, 8), (1024, 8)];
        for (size, want) in cases {
            assert_eq!(level_count(UVec2::new(size, size / 2 + 1)), want, "{size}");
        }
    }
}
