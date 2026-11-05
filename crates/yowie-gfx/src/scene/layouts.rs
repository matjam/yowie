//! Bind group layouts and pipeline layouts of the scene passes.
//!
//! - Group 0 is the scene: the frame block, lights, clusters, materials,
//!   joint matrices, morph weights, the shadow atlas and the environment
//!   maps; bindings 20 to 22 are the reflection probe and light probe grid
//!   block, the probe cubes and the grid texture (`probes/`). The shadow
//!   pass binds a smaller group 0 without the atlas, with the same binding
//!   numbers.
//! - Group 1 is a material's five textures and their samplers.
//! - Group 2 is a mesh's morph target deltas.

use std::num::NonZeroU64;

use wgpu::{BindGroupLayoutEntry as Entry, BindingType, ShaderStages as S};

/// Colour format of the HDR scene target.
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Depth format of the scene and the shadow atlas.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The number of textures in a material's bind group.
pub(crate) const MATERIAL_TEXTURES: u32 = 5;

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

fn uniform(binding: u32, visibility: wgpu::ShaderStages, size: u64) -> Entry {
    Entry {
        binding,
        visibility,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(size),
        },
        count: None,
    }
}

pub(crate) fn texture(binding: u32, dim: wgpu::TextureViewDimension) -> Entry {
    Entry {
        binding,
        visibility: S::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: dim,
            multisampled: false,
        },
        count: None,
    }
}

pub(crate) fn sampler(binding: u32) -> Entry {
    Entry {
        binding,
        visibility: S::FRAGMENT,
        ty: BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// Every layout the scene renderer builds pipelines and bind groups with.
#[derive(Debug)]
pub(crate) struct SceneLayouts {
    pub(crate) frame: wgpu::BindGroupLayout,
    pub(crate) shadow_frame: wgpu::BindGroupLayout,
    pub(crate) material: wgpu::BindGroupLayout,
    pub(crate) morph: wgpu::BindGroupLayout,
    pub(crate) mesh: wgpu::PipelineLayout,
    pub(crate) shadow: wgpu::PipelineLayout,
    pub(crate) frame_only: wgpu::PipelineLayout,
}

impl SceneLayouts {
    /// The layouts; `cube_array` says whether reflection probes are bound
    /// as a cube array (see `probes::cube_arrays`).
    pub(crate) fn new(device: &wgpu::Device, cube_array: bool) -> Self {
        let frame_size = std::mem::size_of::<super::frame_data::FrameUniform>() as u64;
        let probe_size = std::mem::size_of::<super::probes::ProbeBlock>() as u64;
        let probe_dim = if cube_array {
            wgpu::TextureViewDimension::CubeArray
        } else {
            wgpu::TextureViewDimension::Cube
        };
        let vf = S::VERTEX_FRAGMENT;
        let frame = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie scene frame"),
            entries: &[
                uniform(0, vf, frame_size),
                storage(1, S::FRAGMENT),
                storage(2, S::FRAGMENT),
                storage(3, S::FRAGMENT),
                storage(4, S::FRAGMENT),
                storage(5, S::VERTEX),
                storage(6, S::VERTEX),
                Entry {
                    binding: 7,
                    visibility: S::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                Entry {
                    binding: 8,
                    visibility: S::FRAGMENT,
                    ty: BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                texture(9, wgpu::TextureViewDimension::Cube),
                texture(10, wgpu::TextureViewDimension::Cube),
                sampler(11),
                uniform(20, S::FRAGMENT, probe_size),
                texture(21, probe_dim),
                texture(22, wgpu::TextureViewDimension::D3),
            ],
        });
        let shadow_frame = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie shadow frame"),
            entries: &[
                uniform(0, vf, frame_size),
                storage(4, S::FRAGMENT),
                storage(5, S::VERTEX),
                storage(6, S::VERTEX),
            ],
        });
        let mut mat_entries = Vec::new();
        for i in 0..MATERIAL_TEXTURES {
            mat_entries.push(texture(i, wgpu::TextureViewDimension::D2));
        }
        for i in 0..MATERIAL_TEXTURES {
            mat_entries.push(sampler(MATERIAL_TEXTURES + i));
        }
        let material = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie material"),
            entries: &mat_entries,
        });
        let morph = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie morph"),
            entries: &[storage(0, S::VERTEX)],
        });
        let pl = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let mesh = pl("yowie mesh", &[Some(&frame), Some(&material), Some(&morph)]);
        let shadow = pl(
            "yowie shadow",
            &[Some(&shadow_frame), Some(&material), Some(&morph)],
        );
        let frame_only = pl("yowie scene frame only", &[Some(&frame)]);
        Self {
            frame,
            shadow_frame,
            material,
            morph,
            mesh,
            shadow,
            frame_only,
        }
    }
}
