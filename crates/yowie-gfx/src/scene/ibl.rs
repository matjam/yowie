//! Baking image-based lighting on the GPU: a radiance cube (from a
//! panorama or the procedural sky) with its mip chain, a prefiltered
//! specular cube with one mip per roughness step, and a diffuse
//! irradiance cube.

use std::num::NonZeroU64;

use super::frame_data::FrameUniform;
use super::layouts::{HDR_FORMAT, sampler, texture};
use crate::pipelines::trusted_module;

const COMMON: &str = include_str!("shaders/common.wgsl");
const ATMOSPHERE: &str = include_str!("shaders/atmosphere.wgsl");
const IBL: &str = include_str!("shaders/ibl.wgsl");

/// Face size of the irradiance cube.
const IRRADIANCE_SIZE: u32 = 16;
/// The uniform stride between passes' parameters.
const PARAM_STRIDE: u64 = 256;

/// The two cubes a scene lights with.
#[derive(Debug)]
pub(crate) struct EnvMaps {
    pub(crate) specular: wgpu::TextureView,
    pub(crate) irradiance: wgpu::TextureView,
    /// The number of mips in `specular`; roughness 1 is the last.
    pub(crate) specular_mips: u32,
}

/// The pipelines that bake environment maps.
#[derive(Debug)]
pub(crate) struct IblBaker {
    frame_layout: wgpu::BindGroupLayout,
    bake_layout: wgpu::BindGroupLayout,
    panorama: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    prefilter: wgpu::RenderPipeline,
    irradiance: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    dummy_cube: wgpu::TextureView,
    dummy_pano: wgpu::TextureView,
}

fn cube_texture(device: &wgpu::Device, size: u32, mips: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

pub(crate) fn cube_view(t: &wgpu::Texture, base_mip: u32, mips: Option<u32>) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        base_mip_level: base_mip,
        mip_level_count: mips,
        ..Default::default()
    })
}

pub(crate) fn face_view(t: &wgpu::Texture, face: u32, mip: u32) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_mip_level: mip,
        mip_level_count: Some(1),
        base_array_layer: face,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

/// One pass of a bake: its target, source and parameters.
struct Pass<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    target: wgpu::TextureView,
    cube: Option<wgpu::TextureView>,
    params: [f32; 4],
}

impl IblBaker {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie bake frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<FrameUniform>() as u64),
                },
                count: None,
            }],
        });
        let bake_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yowie bake"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::Cube),
                texture(2, wgpu::TextureViewDimension::D2),
                sampler(3),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yowie bake"),
            bind_group_layouts: &[Some(&frame_layout), Some(&bake_layout)],
            immediate_size: 0,
        });
        let src = [COMMON, ATMOSPHERE, IBL].join("\n");
        let module = trusted_module(device, "yowie ibl", &src);
        let pipeline = |fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("yowie bake"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_bake"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(HDR_FORMAT.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yowie bake"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let dummy_cube = cube_view(&cube_texture(device, 1, 1, "yowie bake dummy"), 0, None);
        let dummy_pano = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("yowie bake dummy"),
                size: wgpu::Extent3d::default(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            panorama: pipeline("fs_panorama"),
            sky: pipeline("fs_sky_bake"),
            downsample: pipeline("fs_downsample"),
            prefilter: pipeline("fs_prefilter"),
            irradiance: pipeline("fs_irradiance"),
            frame_layout,
            bake_layout,
            sampler,
            dummy_cube,
            dummy_pano,
        }
    }

    /// Records the bake of a radiance cube `size` texels a side into
    /// `encoder`. The source is the panorama `pano` (an `Rgba16Float`
    /// texture view), or the procedural sky described by `frame` when
    /// `pano` is `None`.
    pub(crate) fn bake(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Buffer,
        pano: Option<&wgpu::TextureView>,
        size: u32,
    ) -> EnvMaps {
        let size = size.clamp(8, 2048).next_power_of_two();
        let mips = size.trailing_zeros() + 1;
        let spec_mips = (mips - 2).clamp(1, 6);
        let radiance = cube_texture(device, size, mips, "yowie radiance");
        let specular = cube_texture(device, size, spec_mips, "yowie specular");
        let irradiance = cube_texture(device, IRRADIANCE_SIZE, 1, "yowie irradiance");
        let s = size as f32;
        let mut passes: Vec<Pass> = Vec::new();
        for face in 0..6 {
            passes.push(Pass {
                pipeline: if pano.is_some() {
                    &self.panorama
                } else {
                    &self.sky
                },
                target: face_view(&radiance, face, 0),
                cube: None,
                params: [face as f32, 1.0, 0.0, s],
            });
        }
        self.filter_passes(&radiance, &specular, &mut passes);
        let full = cube_view(&radiance, 0, None);
        for face in 0..6 {
            passes.push(Pass {
                pipeline: &self.irradiance,
                target: face_view(&irradiance, face, 0),
                cube: Some(full.clone()),
                params: [face as f32, mips as f32, 1.0, s],
            });
        }
        self.run(device, queue, encoder, frame, pano, &passes);
        EnvMaps {
            specular: cube_view(&specular, 0, None),
            irradiance: cube_view(&irradiance, 0, None),
            specular_mips: spec_mips,
        }
    }

    /// Records the mip chain of `radiance` (a cube whose mip 0 is filled)
    /// and its prefiltering into every mip of `specular`, roughness 0 at
    /// mip 0 to 1 at the last. Reflection probes filter their captures
    /// with it.
    pub(crate) fn filter(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Buffer,
        radiance: &wgpu::Texture,
        specular: &wgpu::Texture,
    ) {
        let mut passes = Vec::new();
        self.filter_passes(radiance, specular, &mut passes);
        self.run(device, queue, encoder, frame, None, &passes);
    }

    /// The downsample passes of `radiance`'s mips and the prefilter passes
    /// of `specular`'s.
    fn filter_passes<'a>(
        &'a self,
        radiance: &wgpu::Texture,
        specular: &wgpu::Texture,
        passes: &mut Vec<Pass<'a>>,
    ) {
        let mips = radiance.mip_level_count();
        let spec_mips = specular.mip_level_count();
        let s = radiance.width() as f32;
        for mip in 1..mips {
            for face in 0..6 {
                passes.push(Pass {
                    pipeline: &self.downsample,
                    target: face_view(radiance, face, mip),
                    cube: Some(cube_view(radiance, mip - 1, Some(1))),
                    params: [face as f32, 1.0, 0.0, s],
                });
            }
        }
        let full = cube_view(radiance, 0, None);
        for mip in 0..spec_mips {
            let roughness = mip as f32 / (spec_mips - 1).max(1) as f32;
            for face in 0..6 {
                passes.push(Pass {
                    pipeline: &self.prefilter,
                    target: face_view(specular, face, mip),
                    cube: Some(full.clone()),
                    params: [face as f32, mips as f32, roughness, s],
                });
            }
        }
    }

    /// Records `passes`, each a full-screen triangle into its target.
    fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Buffer,
        pano: Option<&wgpu::TextureView>,
        passes: &[Pass],
    ) {
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie bake params"),
            size: PARAM_STRIDE * passes.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut bytes = vec![0u8; params.size() as usize];
        for (i, p) in passes.iter().enumerate() {
            let at = i * PARAM_STRIDE as usize;
            bytes[at..at + 16].copy_from_slice(bytemuck::bytes_of(&p.params));
        }
        queue.write_buffer(&params, 0, &bytes);
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie bake frame"),
            layout: &self.frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            }],
        });
        for (i, p) in passes.iter().enumerate() {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie bake"),
                layout: &self.bake_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &params,
                            offset: i as u64 * PARAM_STRIDE,
                            size: NonZeroU64::new(16),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            p.cube.as_ref().unwrap_or(&self.dummy_cube),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(
                            pano.unwrap_or(&self.dummy_pano),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yowie bake"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &p.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(p.pipeline);
            pass.set_bind_group(0, &frame_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
