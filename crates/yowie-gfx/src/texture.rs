//! Textures: images on the GPU that sprites, text and shapes sample.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use image::RgbaImage;
use yowie_core::{UVec2, uvec2};

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::pixels;
use crate::shared::Shared;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Hands out ids that tell resources apart for batching and caching.
pub(crate) fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// How a texture is sampled between texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    /// The nearest texel; keeps pixel art crisp.
    #[default]
    Nearest,
    /// A blend of the nearest texels; smooth when scaled or rotated.
    Linear,
}

/// What sampling outside 0..1 returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Wrap {
    /// The edge texel.
    #[default]
    Clamp,
    /// The texture tiles.
    Repeat,
    /// The texture tiles, mirrored on every other repeat.
    Mirror,
}

/// How a texture is stored and sampled. The default suits pixel art:
/// nearest filtering, clamped edges, no mipmaps, sRGB colour with straight
/// alpha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextureOptions {
    /// Sampling between texels.
    pub filter: Filter,
    /// Sampling outside the texture.
    pub wrap: Wrap,
    /// Whether to build a mip chain, which keeps a texture drawn smaller
    /// than its size from shimmering. It is regenerated on every pixel
    /// update.
    pub mipmaps: bool,
    /// Whether the supplied pixels are already premultiplied by alpha. When
    /// false the engine premultiplies them on upload.
    pub premultiplied: bool,
    /// Whether the pixels are data (masks, normal maps) rather than sRGB
    /// colour. Data is stored and sampled without colour conversion.
    pub data: bool,
}

impl TextureOptions {
    /// Linear filtering with mipmaps, for smooth scaling and rotation.
    pub fn smooth() -> Self {
        Self {
            filter: Filter::Linear,
            mipmaps: true,
            ..Self::default()
        }
    }
}

#[derive(Debug)]
pub(crate) struct TextureInner {
    pub(crate) id: u64,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) size: UVec2,
    pub(crate) options: TextureOptions,
}

/// An image on the GPU. Cloning shares the image; it is freed when the
/// last clone and every queued draw that uses it are gone.
#[derive(Clone, Debug)]
pub struct Texture {
    pub(crate) inner: Arc<TextureInner>,
}

/// What a texture is for, which decides its usage flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    Sampled,
    RenderTarget,
}

impl Texture {
    /// Decodes PNG or JPEG bytes and uploads the image.
    pub fn from_bytes(gfx: &Graphics, bytes: &[u8], options: TextureOptions) -> Result<Texture> {
        let image = image::load_from_memory(bytes)?.to_rgba8();
        Texture::from_image(gfx, &image, options)
    }

    /// Uploads an RGBA image with straight alpha (or premultiplied, if the
    /// options say so).
    pub fn from_image(
        gfx: &Graphics,
        image: &RgbaImage,
        options: TextureOptions,
    ) -> Result<Texture> {
        Texture::from_pixels(gfx, image.width(), image.height(), image.as_raw(), options)
    }

    /// Uploads `width * height` RGBA8 pixels, row by row from the top-left.
    pub fn from_pixels(
        gfx: &Graphics,
        width: u32,
        height: u32,
        rgba: &[u8],
        options: TextureOptions,
    ) -> Result<Texture> {
        let format = if options.data {
            wgpu::TextureFormat::Rgba8Unorm
        } else {
            wgpu::TextureFormat::Rgba8UnormSrgb
        };
        let tex = Texture::create(
            &gfx.shared,
            uvec2(width, height),
            format,
            options,
            Purpose::Sampled,
        )?;
        tex.update_pixels(gfx, 0, 0, width, height, rgba)?;
        Ok(tex)
    }

    /// Creates an empty texture of the given format and purpose.
    pub(crate) fn create(
        shared: &Shared,
        size: UVec2,
        format: wgpu::TextureFormat,
        options: TextureOptions,
        purpose: Purpose,
    ) -> Result<Texture> {
        let max = shared.device().limits().max_texture_dimension_2d;
        if size.x == 0 || size.y == 0 || size.x > max || size.y > max {
            return Err(GfxError::InvalidSize {
                width: size.x,
                height: size.y,
                max,
            });
        }
        let mip_level_count = if options.mipmaps {
            32 - size.x.max(size.y).leading_zeros()
        } else {
            1
        };
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC;
        if options.mipmaps || purpose == Purpose::RenderTarget {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = shared.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie texture"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count,
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
        Ok(Texture {
            inner: Arc::new(TextureInner {
                id: next_id(),
                texture,
                view,
                bind_group,
                size,
                options,
            }),
        })
    }

    /// Replaces a rectangle of texels with `width * height` RGBA8 pixels,
    /// converted the way the texture's options say. The mip chain, if any,
    /// is rebuilt. Draws queued earlier in the frame see the new pixels.
    pub fn update_pixels(
        &self,
        gfx: &Graphics,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        let size = self.inner.size;
        if x.checked_add(width).is_none_or(|r| r > size.x)
            || y.checked_add(height).is_none_or(|b| b > size.y)
            || width == 0
            || height == 0
        {
            return Err(GfxError::RegionOutOfBounds {
                x,
                y,
                width,
                height,
                tex_width: size.x,
                tex_height: size.y,
            });
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(GfxError::PixelDataSize {
                expected,
                got: rgba.len(),
            });
        }
        let opts = self.inner.options;
        let converted;
        let data = if opts.premultiplied {
            rgba
        } else {
            let mut copy = rgba.to_vec();
            if opts.data {
                pixels::premultiply_linear(&mut copy);
            } else {
                pixels::premultiply_srgb(&mut copy);
            }
            converted = copy;
            &converted
        };
        self.write_level0(&gfx.shared, x, y, width, height, data, 4);
        gfx.shared
            .mipmaps
            .generate(gfx.shared.device(), gfx.shared.queue(), &self.inner.texture);
        Ok(())
    }

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
        shared.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.inner.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes_per_texel),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// The size in texels.
    pub fn size(&self) -> UVec2 {
        self.inner.size
    }

    /// The width in texels.
    pub fn width(&self) -> u32 {
        self.inner.size.x
    }

    /// The height in texels.
    pub fn height(&self) -> u32 {
        self.inner.size.y
    }

    /// The options the texture was created with.
    pub fn options(&self) -> TextureOptions {
        self.inner.options
    }

    /// The underlying wgpu texture, for renderer extensions.
    pub fn wgpu_texture(&self) -> &wgpu::Texture {
        &self.inner.texture
    }

    /// A view of the whole texture, for renderer extensions.
    pub fn wgpu_view(&self) -> &wgpu::TextureView {
        &self.inner.view
    }

    pub(crate) fn id(&self) -> u64 {
        self.inner.id
    }
}

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Eq for Texture {}
