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

/// The images a scene's chain makes when an effect first asks for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    /// The two spare HDR images the chain passes the picture between.
    Ping(u8),
    /// The temporal pass's two history images, alternating each frame.
    History(u8),
    /// Depth as R32Float, with the nearest-depth mips reflections trace.
    Depth,
    /// Ambient occlusion and its blur's middle step.
    Occlusion(u8),
    /// The reflection trace: reflected colour, fade in alpha.
    Reflection,
    /// Motion of moved objects: this frame's minus last frame's texture
    /// coordinates, 1 in alpha where an object wrote it.
    Velocity,
    /// Depth of field's half-size gather.
    Focus,
    /// The light shafts at half size.
    Rays,
}

impl Kind {
    fn spec(self, size: UVec2) -> (UVec2, wgpu::TextureFormat, u32) {
        let half = (size / 2).max(UVec2::ONE);
        match self {
            Kind::Ping(_) | Kind::History(_) | Kind::Reflection | Kind::Velocity => (size, HDR, 1),
            Kind::Depth => (size, DEPTH, depth_mips(size)),
            Kind::Occlusion(_) => (size, wgpu::TextureFormat::R8Unorm, 1),
            Kind::Focus | Kind::Rays => (half, HDR, 1),
        }
    }
}

/// Format of the chain's copy of the scene depth.
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// Mips of the depth copy: down to about four pixels, at most eight.
pub(crate) fn depth_mips(size: UVec2) -> u32 {
    let long = size.x.max(size.y).max(1);
    (32 - long.leading_zeros()).saturating_sub(2).clamp(1, 8)
}

/// Everything one scene or 2D frame keeps for its post chain.
#[derive(Debug)]
pub(crate) struct PostTargets {
    pub(crate) size: UVec2,
    pub(crate) uniform: wgpu::Buffer,
    pub(crate) common: wgpu::BindGroup,
    pub(crate) images: HashMap<Kind, Image>,
    pub(crate) bloom: Option<BloomChain>,
    pub(crate) ldr: Option<Image>,
    /// Auto exposure's state (exposure, adapted luminance, whether it
    /// has a value, then the luminance histogram) and its id.
    pub(crate) exposure: Option<(wgpu::Buffer, u64)>,
    /// The moved draws' placements for the velocity pass.
    pub(crate) velocity_instances: Option<wgpu::Buffer>,
    /// Views of each mip of the depth image, made with it.
    pub(crate) depth_mips: Vec<(wgpu::TextureView, u64)>,
    /// Which history image the next temporal pass writes.
    pub(crate) history_next: u8,
    /// The other history image holds last frame's resolved picture.
    pub(crate) history_valid: bool,
    /// The game time of the last frame, for auto exposure's adaptation.
    pub(crate) last_time: Option<f32>,
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
            images: HashMap::new(),
            bloom: None,
            ldr: None,
            exposure: None,
            velocity_instances: None,
            depth_mips: Vec::new(),
            history_next: 0,
            history_valid: false,
            last_time: None,
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
        self.images.clear();
        self.depth_mips.clear();
        self.history_valid = false;
        self.bloom = None;
        self.ldr = None;
        self.groups.clear();
    }

    /// The image of `kind`, made the first time it is asked for.
    pub(crate) fn image(&mut self, device: &wgpu::Device, kind: Kind) -> &Image {
        let size = self.size;
        self.images.entry(kind).or_insert_with(|| {
            let (size, format, mips) = kind.spec(size);
            Image::new(
                device,
                "yowie post image",
                size,
                format,
                mips,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            )
        })
    }

    /// The view and id of the image of `kind`, made when missing.
    pub(crate) fn view(&mut self, device: &wgpu::Device, kind: Kind) -> (wgpu::TextureView, u64) {
        let i = self.image(device, kind);
        (i.view.clone(), i.id)
    }

    /// Views of every mip of the depth image, each with its own id.
    pub(crate) fn depth_mips(&mut self, device: &wgpu::Device) -> Vec<(wgpu::TextureView, u64)> {
        if self.depth_mips.is_empty() {
            let i = self.image(device, Kind::Depth);
            let levels = i.texture.mip_level_count();
            let mips: Vec<_> = (0..levels)
                .map(|l| (i.mip(l), super::chain::mix(&[i.id, u64::from(l)])))
                .collect();
            self.depth_mips = mips;
        }
        self.depth_mips.clone()
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
}
