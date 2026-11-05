//! The GPU objects behind a [`Texture`]: the wgpu texture, its view and
//! bind group, created together and replaced together.

use yowie_core::UVec2;

use crate::error::{GfxError, Result};
use crate::shared::Shared;
use crate::texture::{Purpose, Texture, TextureOptions};

/// One generation of a texture's GPU objects.
#[derive(Debug)]
pub(crate) struct TextureGpu {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) size: UVec2,
    pub(crate) options: TextureOptions,
}

impl TextureGpu {
    /// Creates empty GPU objects with `mips` mip levels.
    pub(crate) fn new(
        shared: &Shared,
        size: UVec2,
        format: wgpu::TextureFormat,
        options: TextureOptions,
        purpose: Purpose,
        mips: u32,
    ) -> Result<Self> {
        let max = shared.device().limits().max_texture_dimension_2d;
        if size.x == 0 || size.y == 0 || size.x > max || size.y > max {
            return Err(GfxError::InvalidSize {
                width: size.x,
                height: size.y,
                max,
            });
        }
        let compressed = format.is_compressed();
        if compressed && (!size.x.is_multiple_of(4) || !size.y.is_multiple_of(4)) {
            return Err(GfxError::UnsupportedFormat(format!(
                "a block-compressed texture must be a multiple of 4 texels in each direction, not {}x{}",
                size.x, size.y
            )));
        }
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if !compressed {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        if (options.mipmaps && !compressed) || purpose == Purpose::RenderTarget {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = shared.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie texture"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = shared
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("yowie texture"),
                layout: &shared.layouts.texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(shared.samplers.get(&options)),
                    },
                ],
            });
        Ok(Self {
            texture,
            view,
            bind_group,
            size,
            options,
        })
    }

    /// Whether the texture stores 8-bit RGBA, the only format pixel
    /// updates accept.
    pub(crate) fn is_rgba8(&self) -> bool {
        matches!(
            self.texture.format(),
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        )
    }

    /// Writes `data` into a rectangle of mip `level`, `bytes_per_texel`
    /// bytes per texel, with no conversion.
    pub(crate) fn write_level(
        &self,
        shared: &Shared,
        level: u32,
        origin: [u32; 2],
        extent: [u32; 2],
        data: &[u8],
        bytes_per_texel: u32,
    ) {
        self.write_rows(
            shared,
            level,
            origin,
            extent,
            data,
            extent[0] * bytes_per_texel,
            extent[1],
        );
    }

    /// Writes `data` into mip `level` with an explicit row pitch in bytes
    /// and row count (rows of blocks for compressed formats).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_rows(
        &self,
        shared: &Shared,
        level: u32,
        origin: [u32; 2],
        extent: [u32; 2],
        data: &[u8],
        bytes_per_row: u32,
        rows: u32,
    ) {
        shared.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: level,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(rows),
            },
            wgpu::Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
        );
    }
}

impl Texture {
    /// Writes raw texels into level 0 with no conversion.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_level0(
        &self,
        shared: &Shared,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
        bytes_per_texel: u32,
    ) {
        self.gpu()
            .write_level(shared, 0, [x, y], [width, height], data, bytes_per_texel);
    }
}
