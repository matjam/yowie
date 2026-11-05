//! The device objects textures, shaders and frames share.

use crate::device::Gpu;
use crate::mipmap::MipmapGenerator;
use crate::pipelines::Layouts;
use crate::texture::{Filter, TextureOptions, Wrap};

/// One sampler for each combination of filter, edge handling and
/// mipmapping, created up front.
#[derive(Debug)]
pub(crate) struct Samplers {
    samplers: Vec<wgpu::Sampler>,
}

impl Samplers {
    const WRAPS: [Wrap; 3] = [Wrap::Clamp, Wrap::Repeat, Wrap::Mirror];

    fn new(device: &wgpu::Device) -> Self {
        let mut samplers = Vec::new();
        for filter in [Filter::Nearest, Filter::Linear] {
            for wrap in Self::WRAPS {
                for mips in [false, true] {
                    let address = match wrap {
                        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
                        Wrap::Repeat => wgpu::AddressMode::Repeat,
                        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
                    };
                    let f = match filter {
                        Filter::Nearest => wgpu::FilterMode::Nearest,
                        Filter::Linear => wgpu::FilterMode::Linear,
                    };
                    samplers.push(device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some("yowie sampler"),
                        address_mode_u: address,
                        address_mode_v: address,
                        address_mode_w: address,
                        mag_filter: f,
                        min_filter: f,
                        mipmap_filter: if mips {
                            wgpu::MipmapFilterMode::Linear
                        } else {
                            wgpu::MipmapFilterMode::Nearest
                        },
                        ..Default::default()
                    }));
                }
            }
        }
        Self { samplers }
    }

    /// The sampler for a texture's options.
    pub(crate) fn get(&self, options: &TextureOptions) -> &wgpu::Sampler {
        let f = match options.filter {
            Filter::Nearest => 0,
            Filter::Linear => 1,
        };
        let w = match options.wrap {
            Wrap::Clamp => 0,
            Wrap::Repeat => 1,
            Wrap::Mirror => 2,
        };
        let m = usize::from(options.mipmaps);
        // The index stays inside the 2 x 3 x 2 table built in `new`.
        &self.samplers[(f * 3 + w) * 2 + m]
    }
}

/// Device-wide state that resource constructors need.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) gpu: Gpu,
    pub(crate) layouts: Layouts,
    pub(crate) samplers: Samplers,
    pub(crate) mipmaps: MipmapGenerator,
}

impl Shared {
    pub(crate) fn new(gpu: Gpu) -> Self {
        let layouts = Layouts::new(&gpu.device);
        let samplers = Samplers::new(&gpu.device);
        let mipmaps = MipmapGenerator::new(&gpu.device, &layouts.texture);
        Self {
            gpu,
            layouts,
            samplers,
            mipmaps,
        }
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }
}
