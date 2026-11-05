//! The compute pipelines static groups are culled with, the depth pyramid
//! they are tested against, and the bind groups between them.

use yowie_core::UVec2;

use super::gpu::ViewGpu;
use super::state::{CullPipelines, Pyramid};
use crate::pipelines::trusted_module;

/// Builds the cull and pyramid pipelines and their layouts.
pub(super) fn pipelines(device: &wgpu::Device) -> CullPipelines {
    use wgpu::{BindGroupLayoutEntry as E, BindingType as B, ShaderStages as S};
    let buffer = |binding, ty| E {
        binding,
        visibility: S::COMPUTE,
        ty: B::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let read = wgpu::BufferBindingType::Storage { read_only: true };
    let write = wgpu::BufferBindingType::Storage { read_only: false };
    let texture = |binding, sample_type| E {
        binding,
        visibility: S::COMPUTE,
        ty: B::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let storage_texture = E {
        binding: 2,
        visibility: S::COMPUTE,
        ty: B::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::R32Float,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    };
    let unfilterable = wgpu::TextureSampleType::Float { filterable: false };
    let layout = |label, entries: &[E]| {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries,
        })
    };
    let cull_layout = layout(
        "yowie cull",
        &[
            buffer(0, wgpu::BufferBindingType::Uniform),
            buffer(1, read),
            buffer(2, read),
            buffer(3, read),
            buffer(4, write),
            buffer(5, write),
            buffer(6, write),
            texture(7, unfilterable),
        ],
    );
    let first_layout = layout(
        "yowie depth pyramid",
        &[texture(0, wgpu::TextureSampleType::Depth), storage_texture],
    );
    let down_layout = layout(
        "yowie depth pyramid",
        &[texture(1, unfilterable), storage_texture],
    );
    let first_ms_layout = layout(
        "yowie depth pyramid",
        &[
            E {
                binding: 3,
                visibility: S::COMPUTE,
                ty: B::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            },
            storage_texture,
        ],
    );
    let cull_module = trusted_module(device, "yowie cull", include_str!("cull.wgsl"));
    let hiz_module = trusted_module(device, "yowie depth pyramid", include_str!("hiz.wgsl"));
    let pipeline = |layout: &wgpu::BindGroupLayout, module, entry| {
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie cull"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&pl),
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    CullPipelines {
        early: pipeline(&cull_layout, &cull_module, "cs_early"),
        late: pipeline(&cull_layout, &cull_module, "cs_late"),
        first: pipeline(&first_layout, &hiz_module, "cs_first"),
        first_ms: pipeline(&first_ms_layout, &hiz_module, "cs_first_ms"),
        down: pipeline(&down_layout, &hiz_module, "cs_down"),
        cull_layout,
        first_layout,
        first_ms_layout,
        down_layout,
    }
}

/// The depth pyramid of a depth target `size` pixels large: level 0 half
/// its size rounded up, each level after half the one before rounded down,
/// down to one texel. `multisampled` depth (a scene with MSAA) is reduced
/// over its samples too.
pub(super) fn pyramid(
    device: &wgpu::Device,
    p: &CullPipelines,
    depth: &wgpu::TextureView,
    size: UVec2,
    multisampled: bool,
) -> Pyramid {
    let (w0, h0) = (size.x.div_ceil(2).max(1), size.y.div_ceil(2).max(1));
    let mips = 32 - w0.max(h0).leading_zeros();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("yowie depth pyramid"),
        size: wgpu::Extent3d {
            width: w0,
            height: h0,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mip = |m: u32| {
        texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: m,
            mip_level_count: Some(1),
            ..Default::default()
        })
    };
    let group = |layout, src: u32, from: &wgpu::TextureView, to: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie depth pyramid"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: src,
                    resource: wgpu::BindingResource::TextureView(from),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(to),
                },
            ],
        })
    };
    let views: Vec<wgpu::TextureView> = (0..mips).map(mip).collect();
    let first = if multisampled {
        group(&p.first_ms_layout, 3, depth, &views[0])
    } else {
        group(&p.first_layout, 0, depth, &views[0])
    };
    let mut steps = vec![(first, (w0, h0))];
    for m in 1..mips as usize {
        let s = ((w0 >> m).max(1), (h0 >> m).max(1));
        steps.push((group(&p.down_layout, 1, &views[m - 1], &views[m]), s));
    }
    Pyramid {
        depth: depth.clone(),
        multisampled,
        size: (size.x, size.y),
        view: texture.create_view(&Default::default()),
        mips,
        steps,
    }
}

/// The cull bind group of one group's view against `pyramid`.
pub(super) fn cull_group(
    device: &wgpu::Device,
    p: &CullPipelines,
    v: &ViewGpu,
    items: &wgpu::Buffer,
    batch_first: &wgpu::Buffer,
    pyramid: &wgpu::TextureView,
) -> wgpu::BindGroup {
    fn buf(binding: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding,
            resource: b.as_entire_binding(),
        }
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("yowie cull"),
        layout: &p.cull_layout,
        entries: &[
            buf(0, &v.uniform),
            buf(1, items),
            buf(2, batch_first),
            buf(3, &v.remap),
            buf(4, &v.visibility),
            buf(5, &v.args),
            buf(6, &v.visible),
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(pyramid),
            },
        ],
    })
}
