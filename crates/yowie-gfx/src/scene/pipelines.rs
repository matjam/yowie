//! The scene's shader modules and the cache of its render pipelines.

use std::collections::HashMap;

use super::layouts::{HDR_FORMAT, SHADOW_FORMAT, SceneLayouts};
use super::stencil::StencilKey;
use super::vertex::buffer_layouts;
use crate::pipelines::trusted_module;

const COMMON: &str = include_str!("shaders/common.wgsl");
const ATMOSPHERE: &str = include_str!("shaders/atmosphere.wgsl");
const VERTEX: &str = include_str!("shaders/vertex.wgsl");
const LAYERS: &str = include_str!("shaders/layers.wgsl");
const LOBES: &str = include_str!("shaders/lobes.wgsl");
const DECALS: &str = include_str!("shaders/decals.wgsl");
const PASSES: &str = include_str!("shaders/passes.wgsl");
const SURFACE: &str = include_str!("shaders/surface.wgsl");
const SKY: &str = include_str!("shaders/sky.wgsl");
const RESOLVE: &str = include_str!("shaders/resolve.wgsl");

/// The surface program the engine draws with when a material has no
/// surface shader.
const DEFAULT_HOOK: &str = "fn material(s: Surface) -> Surface {\n    return s;\n}\n";

/// The format of the order-independent transparency accumulation target.
pub(crate) const OIT_ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The format of the revealage target.
pub(crate) const OIT_REVEAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// The engine's mesh program followed by `hook`, the source defining
/// `fn material`. The engine part ends with a newline, so the hook's first
/// line is the line after the engine's last.
pub(crate) fn mesh_source(hook: &str) -> String {
    let mut s = [
        COMMON, ATMOSPHERE, VERTEX, LAYERS, LOBES, DECALS, PASSES, SURFACE,
    ]
    .join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(hook);
    s
}

/// Which pass and alpha mode a mesh pipeline draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum MeshPass {
    #[default]
    Opaque,
    Mask,
    /// Blended, sorted back to front.
    Blend,
    ShadowOpaque,
    ShadowMask,
    /// Blended, accumulated order-independently.
    Oit,
    /// Refracting materials, drawn after the scene copy.
    Transmit,
    /// Fur shells.
    Fur,
    /// The outline hull.
    Outline,
    /// The x-ray tint where the mesh is hidden.
    XRay,
}

impl MeshPass {
    fn is_shadow(self) -> bool {
        matches!(self, MeshPass::ShadowOpaque | MeshPass::ShadowMask)
    }
}

/// Identifies one mesh pipeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct MeshKey {
    pub(crate) pass: MeshPass,
    pub(crate) double_sided: bool,
    pub(crate) skinned: bool,
    pub(crate) stencil: StencilKey,
    /// The surface shader's id (0 for the engine's own) and whether it has
    /// a uniform block.
    pub(crate) shader: (u64, bool),
}

impl MeshKey {
    /// A small dense number for sort keys, below 256: stencil writers
    /// first, then by pass, sidedness and skinning.
    pub(crate) fn index(&self) -> u32 {
        (u32::from(!self.stencil.writes()) << 7)
            | ((self.pass as u32) << 2)
            | (u32::from(self.double_sided) << 1)
            | u32::from(self.skinned)
    }
}

/// The fixed pipelines of the background, lines and resolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FixedPipeline {
    Sky,
    Lines,
    LinesOnTop,
}

/// The scene's shader modules and every pipeline built from them.
#[derive(Debug)]
pub(crate) struct ScenePipelines {
    mesh_module: wgpu::ShaderModule,
    sky_module: wgpu::ShaderModule,
    resolve_module: wgpu::ShaderModule,
    /// Mesh pipelines with the generation of the module they came from.
    mesh: HashMap<MeshKey, (u32, wgpu::RenderPipeline)>,
    fixed: HashMap<FixedPipeline, wgpu::RenderPipeline>,
    resolve: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

pub(crate) fn depth(
    format: wgpu::TextureFormat,
    write: bool,
    compare: wgpu::CompareFunction,
) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

pub(crate) const PREMULTIPLIED: wgpu::BlendState = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

impl ScenePipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let sky_src = [COMMON, ATMOSPHERE, SKY].join("\n");
        Self {
            mesh_module: trusted_module(device, "yowie mesh", &mesh_source(DEFAULT_HOOK)),
            sky_module: trusted_module(device, "yowie sky", &sky_src),
            resolve_module: trusted_module(device, "yowie resolve", RESOLVE),
            mesh: HashMap::new(),
            fixed: HashMap::new(),
            resolve: HashMap::new(),
        }
    }

    /// The pipeline for `key`. `custom` is the surface shader's module and
    /// generation when `key.shader` names one; a pipeline built from an
    /// older generation is rebuilt.
    pub(crate) fn mesh(
        &mut self,
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        key: MeshKey,
        custom: Option<(wgpu::ShaderModule, u32)>,
    ) -> wgpu::RenderPipeline {
        let generation = custom.as_ref().map_or(0, |c| c.1);
        if let Some((g, p)) = self.mesh.get(&key)
            && *g == generation
        {
            return p.clone();
        }
        let module = custom.map_or_else(|| self.mesh_module.clone(), |c| c.0);
        if key.shader.0 != 0 {
            // A reloaded shader's older pipelines are never drawn again.
            self.mesh
                .retain(|k, (g, _)| k.shader.0 != key.shader.0 || *g == generation);
        }
        let p = build_mesh(device, layouts, &module, key);
        self.mesh.insert(key, (generation, p.clone()));
        p
    }

    pub(crate) fn fixed(
        &mut self,
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        which: FixedPipeline,
    ) -> wgpu::RenderPipeline {
        if let Some(p) = self.fixed.get(&which) {
            return p.clone();
        }
        let module = &self.sky_module;
        let fmt = layouts.depth_format;
        let (vs, fs, topology, depth_state, buffers) = match which {
            FixedPipeline::Sky => (
                "vs_sky",
                "fs_sky",
                wgpu::PrimitiveTopology::TriangleList,
                depth(fmt, false, wgpu::CompareFunction::GreaterEqual),
                vec![],
            ),
            FixedPipeline::Lines | FixedPipeline::LinesOnTop => (
                "vs_line",
                "fs_line",
                wgpu::PrimitiveTopology::LineList,
                depth(
                    fmt,
                    false,
                    if which == FixedPipeline::Lines {
                        wgpu::CompareFunction::GreaterEqual
                    } else {
                        wgpu::CompareFunction::Always
                    },
                ),
                vec![Some(wgpu::VertexBufferLayout {
                    array_stride: 28,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &LINE_ATTRIBUTES,
                })],
            ),
        };
        let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yowie scene fixed"),
            layout: Some(&layouts.frame_only),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some(vs),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology,
                ..Default::default()
            },
            depth_stencil: Some(depth_state),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: (which != FixedPipeline::Sky).then_some(PREMULTIPLIED),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.fixed.insert(which, p.clone());
        p
    }

    /// The resolve pipeline writing `format`, blending premultiplied
    /// colour over what the output holds.
    pub(crate) fn resolve(
        &mut self,
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        if let Some(p) = self.resolve.get(&format) {
            return p.clone();
        }
        let module = &self.resolve_module;
        let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yowie resolve"),
            layout: Some(&layouts.resolve_pipeline),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_resolve"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_resolve"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(PREMULTIPLIED),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.resolve.insert(format, p.clone());
        p
    }
}

const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x4,
];

/// The vertex and fragment entry points of a mesh pipeline.
fn entry_points(key: MeshKey) -> (&'static str, Option<&'static str>) {
    let skinned = key.skinned;
    let vs = match key.pass {
        MeshPass::ShadowOpaque | MeshPass::ShadowMask if skinned => "vs_shadow_skinned",
        MeshPass::ShadowOpaque | MeshPass::ShadowMask => "vs_shadow_static",
        MeshPass::Fur if skinned => "vs_fur_skinned",
        MeshPass::Fur => "vs_fur_static",
        MeshPass::Outline if skinned => "vs_outline_skinned",
        MeshPass::Outline => "vs_outline_static",
        _ if skinned => "vs_skinned",
        _ => "vs_static",
    };
    let fs = match key.pass {
        MeshPass::Opaque => Some("fs_opaque"),
        MeshPass::Mask => Some("fs_mask"),
        MeshPass::Blend => Some("fs_blend"),
        MeshPass::Oit => Some("fs_oit"),
        MeshPass::Transmit => Some("fs_transmit"),
        MeshPass::Fur => Some("fs_fur"),
        MeshPass::Outline => Some("fs_outline"),
        MeshPass::XRay => Some("fs_xray"),
        MeshPass::ShadowOpaque => None,
        MeshPass::ShadowMask => Some("fs_shadow_mask"),
    };
    (vs, fs)
}

/// The colour targets of a mesh pass.
fn color_targets(pass: MeshPass) -> Vec<Option<wgpu::ColorTargetState>> {
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op};
    let hdr = |blend| {
        Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })
    };
    match pass {
        MeshPass::ShadowOpaque | MeshPass::ShadowMask => vec![],
        MeshPass::Opaque | MeshPass::Mask | MeshPass::Fur => vec![hdr(None)],
        MeshPass::Blend | MeshPass::Transmit | MeshPass::Outline | MeshPass::XRay => {
            vec![hdr(Some(PREMULTIPLIED))]
        }
        MeshPass::Oit => {
            let add = BlendComponent {
                src_factor: F::One,
                dst_factor: F::One,
                operation: Op::Add,
            };
            let reveal = BlendComponent {
                src_factor: F::Zero,
                dst_factor: F::OneMinusSrc,
                operation: Op::Add,
            };
            vec![
                Some(wgpu::ColorTargetState {
                    format: OIT_ACCUM_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: add,
                        alpha: add,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: OIT_REVEAL_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: reveal,
                        alpha: reveal,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ]
        }
    }
}

/// Depth, stencil and culling of a mesh pass.
fn depth_and_cull(
    layouts: &SceneLayouts,
    key: MeshKey,
) -> (wgpu::DepthStencilState, Option<wgpu::Face>) {
    use wgpu::CompareFunction as C;
    let back = (!key.double_sided).then_some(wgpu::Face::Back);
    let fmt = layouts.depth_format;
    let (mut d, cull) = match key.pass {
        MeshPass::ShadowOpaque | MeshPass::ShadowMask => {
            let mut d = depth(SHADOW_FORMAT, true, C::LessEqual);
            d.bias = wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            };
            return (d, None);
        }
        MeshPass::Opaque | MeshPass::Mask | MeshPass::Fur | MeshPass::Transmit => {
            (depth(fmt, true, C::Greater), back)
        }
        MeshPass::Blend | MeshPass::Oit => (depth(fmt, false, C::Greater), back),
        MeshPass::Outline => (depth(fmt, false, C::Greater), Some(wgpu::Face::Front)),
        MeshPass::XRay => (depth(fmt, false, C::Less), back),
    };
    d.stencil = key.stencil.state();
    (d, cull)
}

/// Builds the mesh pipeline `key` from `module`.
pub(crate) fn build_mesh(
    device: &wgpu::Device,
    layouts: &SceneLayouts,
    module: &wgpu::ShaderModule,
    key: MeshKey,
) -> wgpu::RenderPipeline {
    let shadow = key.pass.is_shadow();
    let (vs, fs) = entry_points(key);
    let buffers = buffer_layouts(key.skinned);
    let (depth_state, cull) = depth_and_cull(layouts, key);
    let targets = color_targets(key.pass);
    let fragment = fs.map(|entry| wgpu::FragmentState {
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        targets: &targets,
    });
    let layout = if shadow {
        &layouts.shadow
    } else if key.shader.1 {
        &layouts.mesh_user
    } else {
        &layouts.mesh
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yowie mesh"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: Some(depth_state),
        multisample: wgpu::MultisampleState::default(),
        fragment,
        multiview_mask: None,
        cache: None,
    })
}
