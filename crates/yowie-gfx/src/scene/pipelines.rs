//! The scene's shader modules and the cache of its render pipelines.

use std::collections::HashMap;

use super::layouts::{DEPTH_FORMAT, HDR_FORMAT, SceneLayouts};
use super::vertex::buffer_layouts;
use crate::pipelines::trusted_module;

const COMMON: &str = include_str!("shaders/common.wgsl");
const ATMOSPHERE: &str = include_str!("shaders/atmosphere.wgsl");
const VERTEX: &str = include_str!("shaders/vertex.wgsl");
const SURFACE: &str = include_str!("shaders/surface.wgsl");
const SKY: &str = include_str!("shaders/sky.wgsl");

/// Which pass and alpha mode a mesh pipeline draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum MeshPass {
    Opaque,
    Mask,
    Blend,
    ShadowOpaque,
    ShadowMask,
}

/// Identifies one mesh pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct MeshKey {
    pub(crate) pass: MeshPass,
    pub(crate) double_sided: bool,
    pub(crate) skinned: bool,
}

impl MeshKey {
    /// A small dense number for sort keys: 5 passes x 2 x 2.
    pub(crate) fn index(&self) -> u32 {
        (self.pass as u32) * 4 + u32::from(self.double_sided) * 2 + u32::from(self.skinned)
    }
}

/// The fixed pipelines of the background and lines.
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
    mesh: HashMap<MeshKey, wgpu::RenderPipeline>,
    fixed: HashMap<FixedPipeline, wgpu::RenderPipeline>,
}

fn depth(write: bool, compare: wgpu::CompareFunction) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

const PREMULTIPLIED: wgpu::BlendState = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;

impl ScenePipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let mesh_src = [COMMON, ATMOSPHERE, VERTEX, SURFACE].join("\n");
        let sky_src = [COMMON, ATMOSPHERE, SKY].join("\n");
        Self {
            mesh_module: trusted_module(device, "yowie mesh", &mesh_src),
            sky_module: trusted_module(device, "yowie sky", &sky_src),
            mesh: HashMap::new(),
            fixed: HashMap::new(),
        }
    }

    pub(crate) fn mesh(
        &mut self,
        device: &wgpu::Device,
        layouts: &SceneLayouts,
        key: MeshKey,
    ) -> wgpu::RenderPipeline {
        if let Some(p) = self.mesh.get(&key) {
            return p.clone();
        }
        let p = build_mesh(device, layouts, &self.mesh_module, key);
        self.mesh.insert(key, p.clone());
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
        let (vs, fs, topology, depth_state, buffers) = match which {
            FixedPipeline::Sky => (
                "vs_sky",
                "fs_sky",
                wgpu::PrimitiveTopology::TriangleList,
                depth(false, wgpu::CompareFunction::GreaterEqual),
                vec![],
            ),
            FixedPipeline::Lines | FixedPipeline::LinesOnTop => (
                "vs_line",
                "fs_line",
                wgpu::PrimitiveTopology::LineList,
                depth(
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
}

const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x4,
];

fn build_mesh(
    device: &wgpu::Device,
    layouts: &SceneLayouts,
    module: &wgpu::ShaderModule,
    key: MeshKey,
) -> wgpu::RenderPipeline {
    let shadow = matches!(key.pass, MeshPass::ShadowOpaque | MeshPass::ShadowMask);
    let vs = match (shadow, key.skinned) {
        (false, false) => "vs_static",
        (false, true) => "vs_skinned",
        (true, false) => "vs_shadow_static",
        (true, true) => "vs_shadow_skinned",
    };
    let fs = match key.pass {
        MeshPass::Opaque => Some("fs_opaque"),
        MeshPass::Mask => Some("fs_mask"),
        MeshPass::Blend => Some("fs_blend"),
        MeshPass::ShadowOpaque => None,
        MeshPass::ShadowMask => Some("fs_shadow_mask"),
    };
    let buffers = buffer_layouts(key.skinned);
    let (depth_state, cull) = if shadow {
        let mut d = depth(true, wgpu::CompareFunction::LessEqual);
        d.bias = wgpu::DepthBiasState {
            constant: 2,
            slope_scale: 2.0,
            clamp: 0.0,
        };
        (d, None)
    } else {
        let write = key.pass != MeshPass::Blend;
        let cull = (!key.double_sided).then_some(wgpu::Face::Back);
        (depth(write, wgpu::CompareFunction::Greater), cull)
    };
    let targets = [Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: (key.pass == MeshPass::Blend).then_some(PREMULTIPLIED),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let fragment = fs.map(|entry| wgpu::FragmentState {
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        targets: if shadow { &[] } else { &targets },
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yowie mesh"),
        layout: Some(if shadow {
            &layouts.shadow
        } else {
            &layouts.mesh
        }),
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
