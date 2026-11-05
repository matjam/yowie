//! The compute and render programs every GPU particle system on a device
//! shares: shader modules, bind group layouts and pipelines.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use yowie_gfx::BlendMode;

use super::layout::{DrawBlock, StepParams};

const SIM: &str = include_str!("sim.wgsl");
const DRAW_COMMON: &str = include_str!("draw_common.wgsl");
const DRAW_2D: &str = include_str!("draw_2d.wgsl");
const DRAW_2D_VS: &str = include_str!("draw_2d_vs.wgsl");
const DRAW_3D: &str = include_str!("draw_3d.wgsl");
const DRAW_3D_VS: &str = include_str!("draw_3d_vs.wgsl");

/// Builds a shader module with runtime bounds checks off. Only the
/// engine's own WGSL goes through here.
fn trusted(device: &wgpu::Device, label: &str, source: String) -> wgpu::ShaderModule {
    let desc = wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    };
    // SAFETY: the shaders index storage arrays only after comparing the
    // index with the capacity the buffers were made for, tables only with
    // clamped or constant indices, and sample textures through the
    // sampling builtins, so dropping the runtime checks reads nothing out
    // of bounds.
    unsafe { device.create_shader_module_trusted(desc, wgpu::ShaderRuntimeChecks::unchecked()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
    /// The scene's depth format for a 3D pass; `None` draws in 2D.
    depth: Option<wgpu::TextureFormat>,
    format: wgpu::TextureFormat,
    blend: BlendMode,
}

/// Shared programs of one device.
#[derive(Debug)]
pub(crate) struct Programs {
    pub(crate) sim_layout: wgpu::BindGroupLayout,
    pub(crate) data_layout: wgpu::BindGroupLayout,
    pub(crate) emit: wgpu::ComputePipeline,
    pub(crate) update: wgpu::ComputePipeline,
    pub(crate) finalize: wgpu::ComputePipeline,
    module_2d: wgpu::ShaderModule,
    module_3d: wgpu::ShaderModule,
    pipelines: Mutex<HashMap<PipelineKey, wgpu::RenderPipeline>>,
}

fn storage(binding: u32, vis: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform(
    binding: u32,
    vis: wgpu::ShaderStages,
    dynamic: bool,
    size: usize,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: wgpu::BufferSize::new(size as u64),
        },
        count: None,
    }
}

impl Programs {
    fn build(device: &wgpu::Device) -> Self {
        let c = wgpu::ShaderStages::COMPUTE;
        let sim_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie particle sim"),
            entries: &[
                uniform(0, c, true, std::mem::size_of::<StepParams>()),
                storage(1, c, false),
                storage(2, c, true),
                storage(3, c, false),
                storage(4, c, false),
                uniform(5, c, false, std::mem::size_of::<DrawBlock>()),
            ],
        });
        let v = wgpu::ShaderStages::VERTEX;
        let f = wgpu::ShaderStages::FRAGMENT;
        let data_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie particle draw"),
            entries: &[
                uniform(0, v, false, std::mem::size_of::<DrawBlock>()),
                storage(1, v, true),
                storage(2, v, true),
                storage(3, v, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: f,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: f,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sim = trusted(device, "yowie particle sim", SIM.to_owned());
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie particle sim"),
            bind_group_layouts: &[Some(&sim_layout)],
            immediate_size: 0,
        });
        let kernel = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                module: &sim,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            emit: kernel("emit"),
            update: kernel("update"),
            finalize: kernel("finalize"),
            sim_layout,
            data_layout,
            module_2d: trusted(
                device,
                "yowie particles 2d",
                format!("{DRAW_2D}{DRAW_COMMON}{DRAW_2D_VS}"),
            ),
            module_3d: trusted(
                device,
                "yowie particles 3d",
                format!("{DRAW_3D}{DRAW_COMMON}{DRAW_3D_VS}"),
            ),
            pipelines: Mutex::new(HashMap::new()),
        }
    }

    /// The programs of `gfx`'s device, built the first time and shared by
    /// every system made with that renderer.
    pub(crate) fn get(gfx: &yowie_gfx::Graphics) -> Arc<Programs> {
        gfx.extension(Programs::build)
    }

    /// The render pipeline for a pass: `camera` is the layout of group 0,
    /// and `depth` the scene's depth format in a 3D pass (`None` in 2D).
    pub(crate) fn pipeline(
        &self,
        device: &wgpu::Device,
        camera: &wgpu::BindGroupLayout,
        depth: Option<wgpu::TextureFormat>,
        format: wgpu::TextureFormat,
        blend: BlendMode,
    ) -> wgpu::RenderPipeline {
        let key = PipelineKey {
            depth,
            format,
            blend,
        };
        let three_d = depth.is_some();
        let mut map = self.pipelines.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = map.get(&key) {
            return p.clone();
        }
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie particle draw"),
            bind_group_layouts: &[Some(camera), Some(&self.data_layout)],
            immediate_size: 0,
        });
        let module = if three_d {
            &self.module_3d
        } else {
            &self.module_2d
        };
        let depth_stencil = depth.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });
        let mut blend_state = blend.state();
        if three_d {
            // The scene resolve reads the HDR target's alpha as coverage,
            // so every blend must raise it, additive ones included.
            blend_state.alpha = wgpu::BlendComponent::OVER;
        }
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yowie particles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend_state),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        map.insert(key, pipeline.clone());
        pipeline
    }
}
