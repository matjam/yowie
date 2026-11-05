//! The post passes' bind group layouts, shader modules and pipelines.
//!
//! Group 0 of every pass is the post block and the linear sampler, one
//! bind group per scene or 2D frame. Group 1 holds the pass's textures.
//! Pipelines are built the first time a pass runs for an output format
//! and blend, and kept.

use std::collections::HashMap;
use std::num::NonZeroU64;

use wgpu::{BindGroupLayoutEntry as Entry, BindingType, ShaderStages as S};

use super::uniform::PostUniform;
use crate::pipelines::trusted_module;

const PRELUDE: &str = include_str!("shaders/prelude.wgsl");
const TONEMAP: &str = include_str!("shaders/tonemap.wgsl");
const COMPOSITE: &str = include_str!("shaders/composite.wgsl");
const BLOOM: &str = include_str!("shaders/bloom.wgsl");
const FXAA: &str = include_str!("shaders/fxaa.wgsl");

/// What a binding of a pass's group 1 holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// A filterable float 2D texture.
    Tex,
    /// A float 2D texture read with `textureLoad` only (R32Float).
    TexLoad,
    /// A filterable float 3D texture.
    Tex3d,
    /// A depth texture.
    Depth,
    /// A multisampled depth texture.
    DepthMs,
    /// A read-only storage buffer.
    Storage,
    /// A read-write storage buffer.
    StorageRw,
}

fn entry(binding: u32, slot: Slot, visibility: S) -> Entry {
    let ty = match slot {
        Slot::Tex | Slot::TexLoad | Slot::Tex3d => BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: slot != Slot::TexLoad,
            },
            view_dimension: if slot == Slot::Tex3d {
                wgpu::TextureViewDimension::D3
            } else {
                wgpu::TextureViewDimension::D2
            },
            multisampled: false,
        },
        Slot::Depth | Slot::DepthMs => BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: slot == Slot::DepthMs,
        },
        Slot::Storage | Slot::StorageRw => BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage {
                read_only: slot == Slot::Storage,
            },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    };
    Entry {
        binding,
        visibility,
        ty,
        count: None,
    }
}

/// The group-1 shape of each post program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Program {
    Composite,
    Bloom,
    Fxaa,
}

impl Program {
    fn slots(self) -> &'static [Slot] {
        use Slot::*;
        match self {
            Program::Composite => &[Tex, Tex, Tex, Tex, Tex3d, Storage, Tex],
            Program::Bloom => &[Tex, Storage],
            Program::Fxaa => &[Tex],
        }
    }

    fn source(self) -> String {
        match self {
            Program::Composite => [PRELUDE, TONEMAP, COMPOSITE].join("\n"),
            Program::Bloom => [PRELUDE, BLOOM].join("\n"),
            Program::Fxaa => [PRELUDE, FXAA].join("\n"),
        }
    }

    const ALL: [Program; 3] = [Program::Composite, Program::Bloom, Program::Fxaa];
}

/// How a pass's output combines with what its target holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Blend {
    Replace,
    Premultiplied,
    Additive,
}

impl Blend {
    fn state(self) -> Option<wgpu::BlendState> {
        match self {
            Blend::Replace => None,
            Blend::Premultiplied => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            Blend::Additive => Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
        }
    }
}

/// Identifies one post pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PipeKey {
    pub(crate) program: Program,
    pub(crate) entry: &'static str,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) blend: Blend,
}

/// The post passes' layouts, modules and pipelines.
#[derive(Debug)]
pub(crate) struct PostPipelines {
    pub(crate) common: wgpu::BindGroupLayout,
    groups: HashMap<Program, wgpu::BindGroupLayout>,
    layouts: HashMap<Program, wgpu::PipelineLayout>,
    modules: HashMap<Program, wgpu::ShaderModule>,
    pipelines: HashMap<PipeKey, wgpu::RenderPipeline>,
}

impl PostPipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let size = std::mem::size_of::<PostUniform>() as u64;
        let all = S::VERTEX_FRAGMENT | S::COMPUTE;
        let common = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie post common"),
            entries: &[
                Entry {
                    binding: 0,
                    visibility: all,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size),
                    },
                    count: None,
                },
                Entry {
                    binding: 1,
                    visibility: all,
                    ty: BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mut groups = HashMap::new();
        let mut layouts = HashMap::new();
        for p in Program::ALL {
            let entries: Vec<Entry> = p
                .slots()
                .iter()
                .enumerate()
                .map(|(i, s)| entry(i as u32, *s, S::FRAGMENT))
                .collect();
            let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yowie post pass"),
                entries: &entries,
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("yowie post pass"),
                bind_group_layouts: &[Some(&common), Some(&group)],
                immediate_size: 0,
            });
            groups.insert(p, group);
            layouts.insert(p, layout);
        }
        Self {
            common,
            groups,
            layouts,
            modules: HashMap::new(),
            pipelines: HashMap::new(),
        }
    }

    /// The group-1 layout of `program`.
    pub(crate) fn group(&self, program: Program) -> &wgpu::BindGroupLayout {
        // `new` builds a layout for every program in `Program::ALL`.
        &self.groups[&program]
    }

    /// The pipeline for `key`, built on first use.
    pub(crate) fn get(&mut self, device: &wgpu::Device, key: PipeKey) -> wgpu::RenderPipeline {
        if let Some(p) = self.pipelines.get(&key) {
            return p.clone();
        }
        let module = self
            .modules
            .entry(key.program)
            .or_insert_with(|| trusted_module(device, "yowie post", &key.program.source()));
        let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yowie post"),
            layout: Some(&self.layouts[&key.program]),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(key.entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: key.format,
                    blend: key.blend.state(),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, p.clone());
        p
    }
}
