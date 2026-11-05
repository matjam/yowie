//! Order-independent transparency: weighted blended accumulation
//! (McGuire and Bavoil, 2013) into two targets beside the scene, then a
//! composite over the scene before the resolve.
//!
//! Each blended fragment adds its premultiplied colour times a depth weight
//! to the accumulation target and multiplies the revealage target by one
//! minus its alpha (`fs_oit` in `shaders/passes.wgsl`). The composite
//! divides the accumulated colour by its accumulated weight and blends it
//! over the scene by one minus the revealage, so a pixel no fragment
//! touched keeps the scene exactly. The result does not depend on the
//! order surfaces were drawn in.

use yowie_core::UVec2;

use super::layouts::{HDR_FORMAT, SceneLayouts};
use super::pipelines::{OIT_ACCUM_FORMAT, OIT_REVEAL_FORMAT, PREMULTIPLIED, depth};
use super::submit::Scene;
use crate::pipelines::trusted_module;

impl Scene<'_> {
    /// Sets how the scene composites materials with
    /// [`AlphaMode::Blend`](super::AlphaMode::Blend). The default is
    /// [`Transparency::OrderIndependent`].
    pub fn set_transparency(&mut self, transparency: Transparency) {
        self.rec.transparency = transparency;
    }
}

/// How a scene composites blended surfaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transparency {
    /// Weighted blended order-independent transparency (the default): no
    /// sorting, so intersecting and crossing surfaces composite without
    /// popping, and equal blended draws batch into one call. Nearer
    /// surfaces weigh more, but the result is an approximation of exact
    /// ordering.
    #[default]
    OrderIndependent,
    /// Draws blended surfaces back to front by the centre of their bounds:
    /// exact for separate surfaces, wrong where surfaces cross.
    Sorted,
}

const COMPOSITE: &str = "
@group(0) @binding(0) var accum: texture_2d<f32>;
@group(0) @binding(1) var accum_sampler: sampler;
@group(1) @binding(0) var reveal: texture_2d<f32>;
@group(1) @binding(1) var reveal_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The average colour of the pixel's blended fragments, covering the
// scene by one minus the revealage, premultiplied.
@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(frag.xy);
    let a = textureLoad(accum, p, 0);
    let r = textureLoad(reveal, p, 0).r;
    let cover = 1.0 - clamp(r, 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let color = a.rgb / max(a.a, 1e-5);
    return vec4<f32>(color * cover, cover);
}
";

/// A scene slot's accumulation and revealage targets.
#[derive(Debug)]
pub(crate) struct OitTargets {
    pub(crate) size: UVec2,
    pub(crate) accum: wgpu::TextureView,
    pub(crate) reveal: wgpu::TextureView,
    /// The composite's two bind groups: accumulation, revealage.
    pub(crate) groups: [wgpu::BindGroup; 2],
}

impl OitTargets {
    pub(crate) fn new(
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        sampler: &wgpu::Sampler,
        size: UVec2,
    ) -> Self {
        let make = |format, label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let accum = make(OIT_ACCUM_FORMAT, "yowie oit accumulation");
        let reveal = make(OIT_REVEAL_FORMAT, "yowie oit revealage");
        let group = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie oit composite"),
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
        let groups = [group(&accum), group(&reveal)];
        Self {
            size,
            accum,
            reveal,
            groups,
        }
    }

    /// Begins the accumulation pass: both targets cleared (colour to zero,
    /// revealage to one), testing against the scene's depth and stencil
    /// without writing them.
    pub(crate) fn begin<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        depth_stencil: &wgpu::TextureView,
    ) -> wgpu::RenderPass<'e> {
        let attachment = |view, clear| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yowie oit accumulation"),
            color_attachments: &[
                attachment(&self.accum, wgpu::Color::TRANSPARENT),
                attachment(&self.reveal, wgpu::Color::WHITE),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_stencil,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }
}

/// The composite pipeline, drawn inside the HDR scene pass.
pub(crate) fn composite_pipeline(
    device: &wgpu::Device,
    layouts: &SceneLayouts,
) -> wgpu::RenderPipeline {
    let module = trusted_module(device, "yowie oit composite", COMPOSITE);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yowie oit composite"),
        layout: Some(&layouts.oit_composite),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(depth(
            layouts.depth_format,
            false,
            wgpu::CompareFunction::Always,
        )),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(PREMULTIPLIED),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
