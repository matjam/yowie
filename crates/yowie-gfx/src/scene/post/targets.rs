//! The images one scene's or one 2D frame's post chain renders through.
//! Each is made the first time a setting asks for it and kept, at the
//! picture's size, until the size changes; a setting that is off costs
//! no memory once it was never on.

use std::collections::HashMap;

use yowie_core::UVec2;

use super::uniform::PostUniform;

/// Colour format of the HDR images the chain passes between its steps.
pub(crate) const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Format of the composited picture FXAA reads.
pub(crate) const LDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A texture and its default view, with an id for bind group caching.
#[derive(Debug)]
pub(crate) struct Image {
    pub(crate) id: u64,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

impl Image {
    pub(crate) fn new(
        device: &wgpu::Device,
        label: &str,
        size: UVec2,
        format: wgpu::TextureFormat,
        mips: u32,
        usage: wgpu::TextureUsages,
    ) -> Image {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.x.max(1),
                height: size.y.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: mips.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        Image {
            id: crate::texture::next_id(),
            view: texture.create_view(&Default::default()),
            texture,
        }
    }

    /// A render target the chain also samples.
    pub(crate) fn target(
        device: &wgpu::Device,
        label: &str,
        size: UVec2,
        format: wgpu::TextureFormat,
    ) -> Image {
        Image::new(
            device,
            label,
            size,
            format,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        )
    }

    /// A view of one mip level.
    pub(crate) fn mip(&self, level: u32) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        })
    }
}

/// The bloom mips: half the picture's size and smaller, each a view.
#[derive(Debug)]
pub(crate) struct BloomChain {
    pub(crate) image: Image,
    pub(crate) mips: Vec<wgpu::TextureView>,
}

impl BloomChain {
    fn new(device: &wgpu::Device, size: UVec2) -> BloomChain {
        let half = (size / 2).max(UVec2::ONE);
        // Down to about eight pixels on the short side, at most six mips.
        let short = half.x.min(half.y).max(1);
        let levels = (32 - short.leading_zeros()).saturating_sub(3).clamp(1, 6);
        let image = Image::new(
            device,
            "yowie bloom",
            half,
            HDR,
            levels,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let mips = (0..levels).map(|l| image.mip(l)).collect();
        BloomChain { image, mips }
    }
}

/// Which full-size HDR image holds the picture between passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Src {
    /// The scene's own colour target (or the 2D frame's).
    Scene,
    /// One of the chain's two spare images.
    Ping(usize),
    /// One of the temporal pass's two history images.
    History(usize),
}

/// Everything one scene or 2D frame keeps for its post chain.
#[derive(Debug)]
pub(crate) struct PostTargets {
    pub(crate) size: UVec2,
    pub(crate) uniform: wgpu::Buffer,
    pub(crate) common: wgpu::BindGroup,
    pub(crate) ping: [Option<Image>; 2],
    pub(crate) history: [Option<Image>; 2],
    pub(crate) bloom: Option<BloomChain>,
    pub(crate) ldr: Option<Image>,
    /// Bind groups by pass and inputs; cleared when an input changes.
    pub(crate) groups: HashMap<[u64; 4], wgpu::BindGroup>,
}

impl PostTargets {
    pub(crate) fn new(
        device: &wgpu::Device,
        common_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: UVec2,
    ) -> PostTargets {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yowie post block"),
            size: std::mem::size_of::<PostUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let common = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yowie post common"),
            layout: common_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        PostTargets {
            size,
            uniform,
            common,
            ping: [None, None],
            history: [None, None],
            bloom: None,
            ldr: None,
            groups: HashMap::new(),
        }
    }

    /// Drops every image when the picture's size changed, keeping the
    /// block and its bind group.
    pub(crate) fn resize(&mut self, size: UVec2) {
        if self.size == size {
            return;
        }
        self.size = size;
        self.ping = [None, None];
        self.history = [None, None];
        self.bloom = None;
        self.ldr = None;
        self.groups.clear();
    }

    pub(crate) fn bloom(&mut self, device: &wgpu::Device) -> &BloomChain {
        let size = self.size;
        self.bloom
            .get_or_insert_with(|| BloomChain::new(device, size))
    }

    pub(crate) fn ldr(&mut self, device: &wgpu::Device) -> &Image {
        let size = self.size;
        self.ldr
            .get_or_insert_with(|| Image::target(device, "yowie post ldr", size, LDR))
    }

    /// Makes spare image `i`.
    pub(crate) fn ensure_ping(&mut self, device: &wgpu::Device, i: usize) {
        let size = self.size;
        self.ping[i].get_or_insert_with(|| Image::target(device, "yowie post image", size, HDR));
    }

    /// Makes both history images.
    pub(crate) fn ensure_history(&mut self, device: &wgpu::Device) {
        let size = self.size;
        for h in &mut self.history {
            h.get_or_insert_with(|| Image::target(device, "yowie history", size, HDR));
        }
    }
}
