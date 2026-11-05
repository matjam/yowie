//! Bind group layouts and pipeline layouts of the scene passes.
//!
//! - Group 0 is the scene: the frame block, lights, clusters, materials,
//!   joint matrices, morph weights, the shadow atlas and the environment
//!   maps; bindings 20 to 22 are the reflection probe and light probe grid
//!   block, the probe cubes and the grid texture (`probes/`); bindings 23
//!   to 26 are the opaque scene copy transmission reads and the decal
//!   records and atlases. The shadow pass binds a smaller group 0 without
//!   the atlas, with the same binding numbers.
//! - Group 1 is a material's textures and their samplers: the five core
//!   textures at 0 to 4 with their samplers at 5 to 9, then the layered
//!   feature textures at 10 to 18 sharing the sampler at 19.
//! - Group 2 is a mesh's morph target deltas.
//! - Group 3 is a game surface shader's uniform block, at a dynamic offset.

use std::num::NonZeroU64;

use wgpu::{BindGroupLayoutEntry as Entry, BindingType, ShaderStages as S};

/// Colour format of the HDR scene target.
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Depth format of the shadow atlas. The scene's depth and stencil format
/// depends on the device: see [`SceneLayouts::depth_format`].
pub(crate) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The core material textures: base colour, metallic-roughness, normal,
/// occlusion, emissive.
pub(crate) const CORE_TEXTURES: u32 = 5;
/// The layered feature textures after them: clearcoat, clearcoat normal,
/// sheen, specular, transmission, thickness, iridescence, anisotropy, fur.
pub(crate) const LAYER_TEXTURES: u32 = 9;
/// The number of textures in a material's bind group.
pub(crate) const MATERIAL_TEXTURES: u32 = CORE_TEXTURES + LAYER_TEXTURES;
/// The binding of the first layered feature texture in group 1.
pub(crate) const LAYER_BINDING: u32 = 2 * CORE_TEXTURES;
/// The binding of the sampler the layered feature textures share.
pub(crate) const LAYER_SAMPLER_BINDING: u32 = LAYER_BINDING + LAYER_TEXTURES;

/// Group 0 bindings of the material features, after the probes' 20 to 22.
pub(crate) const SCENE_COPY_BINDING: u32 = 23;
pub(crate) const DECALS_BINDING: u32 = 24;
pub(crate) const DECAL_COLOR_BINDING: u32 = 25;
pub(crate) const DECAL_NORMAL_BINDING: u32 = 26;

/// The scene's depth format: 32-bit float depth with 8 bits of stencil
/// where the device has it, otherwise 24-bit depth with stencil. The
/// stencil holds the game's masks in its low seven bits and the outline
/// mark in the top bit.
pub(crate) fn scene_depth_format(device: &wgpu::Device) -> wgpu::TextureFormat {
    if device
        .features()
        .contains(wgpu::Features::DEPTH32FLOAT_STENCIL8)
    {
        wgpu::TextureFormat::Depth32FloatStencil8
    } else {
        wgpu::TextureFormat::Depth24PlusStencil8
    }
}

pub(crate) fn storage(binding: u32, visibility: wgpu::ShaderStages) -> Entry {
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

pub(crate) fn uniform(binding: u32, visibility: wgpu::ShaderStages, size: u64) -> Entry {
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
    /// The scene's depth and stencil format (see [`scene_depth_format`]).
    pub(crate) depth_format: wgpu::TextureFormat,
    pub(crate) frame: wgpu::BindGroupLayout,
    pub(crate) shadow_frame: wgpu::BindGroupLayout,
    pub(crate) material: wgpu::BindGroupLayout,
    pub(crate) morph: wgpu::BindGroupLayout,
    pub(crate) user: wgpu::BindGroupLayout,
    /// One texture and its sampler: the OIT composite reads two of these,
    /// the scene copy reads one per level.
    pub(crate) sampled: wgpu::BindGroupLayout,
    pub(crate) mesh: wgpu::PipelineLayout,
    /// The mesh layout with a surface shader's uniform block at group 3.
    pub(crate) mesh_user: wgpu::PipelineLayout,
    pub(crate) shadow: wgpu::PipelineLayout,
    pub(crate) frame_only: wgpu::PipelineLayout,
    pub(crate) oit_composite: wgpu::PipelineLayout,
    pub(crate) copy: wgpu::PipelineLayout,
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
                storage(4, vf),
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
                texture(SCENE_COPY_BINDING, wgpu::TextureViewDimension::D2),
                storage(DECALS_BINDING, S::FRAGMENT),
                texture(DECAL_COLOR_BINDING, wgpu::TextureViewDimension::D2),
                texture(DECAL_NORMAL_BINDING, wgpu::TextureViewDimension::D2),
            ],
        });
        let shadow_frame = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie shadow frame"),
            entries: &[
                uniform(0, vf, frame_size),
                storage(4, vf),
                storage(5, S::VERTEX),
                storage(6, S::VERTEX),
            ],
        });
        let mut mat_entries = Vec::new();
        for i in 0..CORE_TEXTURES {
            mat_entries.push(texture(i, wgpu::TextureViewDimension::D2));
            mat_entries.push(sampler(CORE_TEXTURES + i));
        }
        for i in 0..LAYER_TEXTURES {
            mat_entries.push(texture(LAYER_BINDING + i, wgpu::TextureViewDimension::D2));
        }
        mat_entries.push(sampler(LAYER_SAMPLER_BINDING));
        let material = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie material"),
            entries: &mat_entries,
        });
        let morph = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie morph"),
            entries: &[storage(0, S::VERTEX)],
        });
        let user = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie surface shader uniforms"),
            entries: &[Entry {
                binding: 0,
                visibility: vf,
                ty: BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let sampled = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie sampled texture"),
            entries: &[texture(0, wgpu::TextureViewDimension::D2), sampler(1)],
        });
        let pl = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let mesh = pl("yowie mesh", &[Some(&frame), Some(&material), Some(&morph)]);
        let mesh_user = pl(
            "yowie mesh with uniforms",
            &[Some(&frame), Some(&material), Some(&morph), Some(&user)],
        );
        let shadow = pl(
            "yowie shadow",
            &[Some(&shadow_frame), Some(&material), Some(&morph)],
        );
        let frame_only = pl("yowie scene frame only", &[Some(&frame)]);
        let oit_composite = pl("yowie oit composite", &[Some(&sampled), Some(&sampled)]);
        let copy = pl("yowie scene copy", &[Some(&sampled)]);
        Self {
            depth_format: scene_depth_format(device),
            frame,
            shadow_frame,
            material,
            morph,
            user,
            sampled,
            mesh,
            mesh_user,
            shadow,
            frame_only,
            oit_composite,
            copy,
        }
    }
}
