//! The shader module, bind group layouts and render pipelines of the
//! large-scene draw classes.

use wgpu::{BindGroupLayoutEntry as Entry, BindingType, ShaderStages as S};

use super::layouts::{DEPTH_FORMAT, HDR_FORMAT, SceneLayouts, sampler, texture};
use super::vertex::buffer_layouts;
use crate::pipelines::trusted_module;

const SOURCES: [&str; 10] = [
    include_str!("shaders/common.wgsl"),
    include_str!("shaders/atmosphere.wgsl"),
    include_str!("shaders/vertex.wgsl"),
    include_str!("shaders/surface.wgsl"),
    include_str!("shaders/large.wgsl"),
    include_str!("shaders/terrain.wgsl"),
    include_str!("shaders/water.wgsl"),
    include_str!("shaders/billboard.wgsl"),
    include_str!("shaders/fade.wgsl"),
    include_str!("shaders/impostor_bake.wgsl"),
];

/// The colour formats of an impostor bake: albedo and normal.
pub(crate) const BAKE_FORMATS: [wgpu::TextureFormat; 2] = [
    wgpu::TextureFormat::Rgba8UnormSrgb,
    wgpu::TextureFormat::Rgba8Unorm,
];

fn storage(binding: u32, visibility: wgpu::ShaderStages) -> Entry {
    Entry {
        binding,
        visibility,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform(binding: u32) -> Entry {
    Entry {
        binding,
        visibility: S::VERTEX_FRAGMENT,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn tex2d(binding: u32) -> Entry {
    texture(binding, wgpu::TextureViewDimension::D2)
}

/// Every layout and pipeline of the large-scene classes.
#[derive(Debug)]
pub(crate) struct LargePipelines {
    pub(crate) terrain_layout: wgpu::BindGroupLayout,
    pub(crate) water_layout: wgpu::BindGroupLayout,
    pub(crate) sheet_layout: wgpu::BindGroupLayout,
    pub(crate) terrain: wgpu::RenderPipeline,
    pub(crate) terrain_shadow: wgpu::RenderPipeline,
    pub(crate) water: wgpu::RenderPipeline,
    pub(crate) billboard_cutout: wgpu::RenderPipeline,
    pub(crate) billboard_blend: wgpu::RenderPipeline,
    pub(crate) billboard_shadow: wgpu::RenderPipeline,
    pub(crate) fade: wgpu::RenderPipeline,
    pub(crate) bake_static: wgpu::RenderPipeline,
    pub(crate) bake_skinned: wgpu::RenderPipeline,
}

/// One pipeline's description.
struct Desc<'a> {
    label: &'a str,
    layout: &'a wgpu::PipelineLayout,
    vs: &'a str,
    fs: Option<&'a str>,
    buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    depth: wgpu::DepthStencilState,
    cull: Option<wgpu::Face>,
    targets: &'a [Option<wgpu::ColorTargetState>],
}

fn build(device: &wgpu::Device, module: &wgpu::ShaderModule, d: Desc<'_>) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(d.label),
        layout: Some(d.layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(d.vs),
            compilation_options: Default::default(),
            buffers: d.buffers,
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: d.cull,
            ..Default::default()
        },
        depth_stencil: Some(d.depth),
        multisample: wgpu::MultisampleState::default(),
        fragment: d.fs.map(|entry| wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: d.targets,
        }),
        multiview_mask: None,
        cache: None,
    })
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

fn shadow_depth() -> wgpu::DepthStencilState {
    let mut d = depth(true, wgpu::CompareFunction::LessEqual);
    d.bias = wgpu::DepthBiasState {
        constant: 2,
        slope_scale: 2.0,
        clamp: 0.0,
    };
    d
}

/// The per-instance stream of terrain chunks.
const CHUNK_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Uint32x4];

/// The per-instance stream of billboards; matches `GpuBillboard`.
pub(crate) const BILLBOARD_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Uint32,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x2,
    6 => Uint32x2,
];

impl LargePipelines {
    pub(crate) fn new(device: &wgpu::Device, layouts: &SceneLayouts) -> Self {
        let module = trusted_module(device, "yowie large scene", &SOURCES.join("\n"));
        let bgl = |label, entries: &[Entry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let terrain_layout = bgl(
            "yowie terrain",
            &[
                storage(32, S::VERTEX),
                uniform(33),
                tex2d(34),
                tex2d(35),
                tex2d(36),
                tex2d(37),
                tex2d(38),
                tex2d(39),
                sampler(40),
                sampler(41),
            ],
        );
        let water_layout = bgl("yowie water", &[uniform(32), storage(33, S::FRAGMENT)]);
        let sheet_layout = bgl(
            "yowie billboard sheet",
            &[tex2d(32), tex2d(33), sampler(34), uniform(35)],
        );
        let pl = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let terrain_lit = pl(
            "yowie terrain",
            &[Some(&layouts.frame), Some(&terrain_layout)],
        );
        let terrain_shadow_pl = pl(
            "yowie terrain shadow",
            &[Some(&layouts.shadow_frame), Some(&terrain_layout)],
        );
        let water_pl = pl("yowie water", &[Some(&layouts.frame), Some(&water_layout)]);
        let sheet_pl = pl(
            "yowie billboard",
            &[Some(&layouts.frame), Some(&sheet_layout)],
        );
        let sheet_shadow_pl = pl(
            "yowie billboard shadow",
            &[Some(&layouts.shadow_frame), Some(&sheet_layout)],
        );
        let hdr = |blend: bool| {
            [Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: blend.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let chunks = [Some(wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &CHUNK_ATTRIBUTES,
        })];
        let boards = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<super::billboard::GpuBillboard>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &BILLBOARD_ATTRIBUTES,
        })];
        let greater = wgpu::CompareFunction::Greater;
        let bake_targets = BAKE_FORMATS.map(|format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        });
        let b = |d: Desc<'_>| build(device, &module, d);
        Self {
            terrain: b(Desc {
                label: "yowie terrain",
                layout: &terrain_lit,
                vs: "vs_terrain",
                fs: Some("fs_terrain"),
                buffers: &chunks,
                depth: depth(true, greater),
                cull: Some(wgpu::Face::Back),
                targets: &hdr(false),
            }),
            terrain_shadow: b(Desc {
                label: "yowie terrain shadow",
                layout: &terrain_shadow_pl,
                vs: "vs_terrain_shadow",
                fs: None,
                buffers: &chunks,
                depth: shadow_depth(),
                cull: None,
                targets: &[],
            }),
            water: b(Desc {
                label: "yowie water",
                layout: &water_pl,
                vs: "vs_water",
                fs: Some("fs_water"),
                buffers: &[],
                depth: depth(false, greater),
                cull: None,
                targets: &hdr(true),
            }),
            billboard_cutout: b(Desc {
                label: "yowie billboard",
                layout: &sheet_pl,
                vs: "vs_billboard",
                fs: Some("fs_billboard_cutout"),
                buffers: &boards,
                depth: depth(true, greater),
                cull: None,
                targets: &hdr(false),
            }),
            billboard_blend: b(Desc {
                label: "yowie billboard blend",
                layout: &sheet_pl,
                vs: "vs_billboard",
                fs: Some("fs_billboard_blend"),
                buffers: &boards,
                depth: depth(false, greater),
                cull: None,
                targets: &hdr(true),
            }),
            billboard_shadow: b(Desc {
                label: "yowie billboard shadow",
                layout: &sheet_shadow_pl,
                vs: "vs_billboard_shadow",
                fs: Some("fs_billboard_shadow"),
                buffers: &boards,
                depth: shadow_depth(),
                cull: None,
                targets: &[],
            }),
            fade: b(Desc {
                label: "yowie fade",
                layout: &layouts.mesh,
                vs: "vs_fade",
                fs: Some("fs_fade"),
                buffers: &buffer_layouts(false),
                depth: depth(true, greater),
                cull: None,
                targets: &hdr(false),
            }),
            bake_static: b(Desc {
                label: "yowie impostor bake",
                layout: &layouts.mesh,
                vs: "vs_static",
                fs: Some("fs_impostor_bake"),
                buffers: &buffer_layouts(false),
                depth: depth(true, greater),
                cull: None,
                targets: &bake_targets,
            }),
            bake_skinned: b(Desc {
                label: "yowie impostor bake skinned",
                layout: &layouts.mesh,
                vs: "vs_skinned",
                fs: Some("fs_impostor_bake"),
                buffers: &buffer_layouts(true),
                depth: depth(true, greater),
                cull: None,
                targets: &bake_targets,
            }),
            terrain_layout,
            water_layout,
            sheet_layout,
        }
    }
}
