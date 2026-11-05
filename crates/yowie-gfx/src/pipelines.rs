//! Bind group layouts, the 2D vertex layout and the cache of 2D pipelines.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

use crate::blend::BlendMode;

/// Builds an engine shader module with runtime bounds checks off. Only the
/// engine's own WGSL goes through here; game shaders keep the checked path.
pub(crate) fn trusted_module(
    device: &wgpu::Device,
    label: &str,
    source: &str,
) -> wgpu::ShaderModule {
    let desc = wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    };
    // SAFETY: the engine's shaders index only fixed-size data with
    // in-range constants and sample textures through the sampling builtins,
    // so dropping the runtime bounds checks cannot read out of bounds.
    unsafe { device.create_shader_module_trusted(desc, wgpu::ShaderRuntimeChecks::unchecked()) }
}

/// One vertex of the 2D stream.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct Vertex2D {
    /// Position in drawing coordinates.
    pub(crate) position: [f32; 2],
    /// Texture coordinate.
    pub(crate) uv: [f32; 2],
    /// Linear, premultiplied colour.
    pub(crate) color: [f32; 4],
    /// 0 samples the texture as colour, 1 as a coverage mask in red.
    pub(crate) mode: u32,
}

pub(crate) const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x4,
    3 => Uint32,
];

/// The per-view uniform block the 2D vertex program reads; matches
/// `YowieView` in `sprite_prelude.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ViewUniform {
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) view_size: [f32; 2],
    pub(crate) pixel_scale: f32,
    pub(crate) time: f32,
}

/// The layouts every 2D pipeline shares.
#[derive(Debug)]
pub(crate) struct Layouts {
    /// Group 0: the drawn texture and its sampler.
    pub(crate) texture: wgpu::BindGroupLayout,
    /// Group 1: the view block, at a dynamic offset.
    pub(crate) view: wgpu::BindGroupLayout,
    /// Group 2: a game shader's uniform block, at a dynamic offset.
    pub(crate) user: wgpu::BindGroupLayout,
    pub(crate) base: wgpu::PipelineLayout,
    pub(crate) with_user: wgpu::PipelineLayout,
}

impl Layouts {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let dynamic_uniform = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let view = dynamic_uniform("yowie view");
        let user = dynamic_uniform("yowie shader uniforms");
        let base = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie 2d"),
            bind_group_layouts: &[Some(&texture), Some(&view)],
            immediate_size: 0,
        });
        let with_user = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie 2d with uniforms"),
            bind_group_layouts: &[Some(&texture), Some(&view), Some(&user)],
            immediate_size: 0,
        });
        Self {
            texture,
            view,
            user,
            base,
            with_user,
        }
    }
}

/// Builds a 2D pipeline for `module` drawing into `format` with `blend`.
pub(crate) fn sprite_pipeline(
    device: &wgpu::Device,
    layouts: &Layouts,
    module: &wgpu::ShaderModule,
    has_uniforms: bool,
    blend: BlendMode,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yowie 2d"),
        layout: Some(if has_uniforms {
            &layouts.with_user
        } else {
            &layouts.base
        }),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("yowie_vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex2D>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("yowie_fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend.state()),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Identifies one compiled 2D pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
    /// The shader's id, or 0 for the engine's default.
    pub(crate) shader: u64,
    /// The shader's compile generation.
    pub(crate) generation: u32,
    pub(crate) blend: BlendMode,
    pub(crate) format: wgpu::TextureFormat,
}

/// Every 2D pipeline built so far.
#[derive(Debug, Default)]
pub(crate) struct PipelineCache {
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
}

impl PipelineCache {
    /// The pipeline for `key`, building it with `build` the first time.
    /// Building a new generation of a shader drops its older ones.
    pub(crate) fn get(
        &mut self,
        key: PipelineKey,
        build: impl FnOnce() -> wgpu::RenderPipeline,
    ) -> wgpu::RenderPipeline {
        if let Some(p) = self.pipelines.get(&key) {
            return p.clone();
        }
        self.pipelines
            .retain(|k, _| k.shader != key.shader || k.generation >= key.generation);
        let p = build();
        self.pipelines.insert(key, p.clone());
        p
    }
}
