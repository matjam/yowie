//! Textures: images on the GPU that sprites, text and shapes sample.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use image::RgbaImage;
use parking_lot::RwLock;
use yowie_core::{UVec2, uvec2};

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::pixels;
use crate::shared::Shared;
pub(crate) use crate::texture_gpu::TextureGpu;

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
    /// update. Compressed textures carry the chain stored in their file
    /// and ignore this.
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
    /// The GPU objects, swapped as a whole when the image is replaced.
    pub(crate) gpu: RwLock<Arc<TextureGpu>>,
}

/// An image on the GPU. Cloning shares the image; it is freed when the
/// last clone and every queued draw that uses it are gone.
///
/// The image can be replaced without changing the texture's identity:
/// [`replace_data`](Texture::replace_data) swaps in new pixels (even of
/// another size) and every clone, sprite and queued draw that refers to the
/// texture sees them.
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
        let mips = if options.mipmaps {
            32 - size.x.max(size.y).leading_zeros()
        } else {
            1
        };
        let gpu = TextureGpu::new(shared, size, format, options, purpose, mips)?;
        Ok(Texture::from_gpu(gpu))
    }

    pub(crate) fn from_gpu(gpu: TextureGpu) -> Texture {
        Texture {
            inner: Arc::new(TextureInner {
                id: next_id(),
                gpu: RwLock::new(Arc::new(gpu)),
            }),
        }
    }

    /// The current GPU objects. A replaced texture hands out the new ones;
    /// holders of the old `Arc` keep drawing the old image until they let go.
    pub(crate) fn gpu(&self) -> Arc<TextureGpu> {
        self.inner.gpu.read().clone()
    }

    /// Replaces a rectangle of texels with `width * height` RGBA8 pixels,
    /// converted the way the texture's options say. The mip chain, if any,
    /// is rebuilt. Draws queued earlier in the frame see the new pixels.
    /// Fails for compressed and floating-point textures.
    pub fn update_pixels(
        &self,
        gfx: &Graphics,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        let gpu = self.gpu();
        let size = gpu.size;
        if !gpu.is_rgba8() {
            return Err(GfxError::UnsupportedFormat(format!(
                "{:?} textures cannot take RGBA8 pixel updates",
                gpu.texture.format()
            )));
        }
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
        let opts = gpu.options;
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
        gpu.write_level(&gfx.shared, 0, [x, y], [width, height], data, 4);
        gfx.shared
            .mipmaps
            .generate(gfx.shared.device(), gfx.shared.queue(), &gpu.texture);
        Ok(())
    }

    /// The size in texels.
    pub fn size(&self) -> UVec2 {
        self.gpu().size
    }

    /// The width in texels.
    pub fn width(&self) -> u32 {
        self.gpu().size.x
    }

    /// The height in texels.
    pub fn height(&self) -> u32 {
        self.gpu().size.y
    }

    /// The options the texture was created with.
    pub fn options(&self) -> TextureOptions {
        self.gpu().options
    }

    /// The underlying wgpu texture, for renderer extensions. After
    /// [`replace_data`](Texture::replace_data) this is a different wgpu
    /// texture, so do not keep it across frames.
    pub fn wgpu_texture(&self) -> wgpu::Texture {
        self.gpu().texture.clone()
    }

    /// A view of the whole texture, for renderer extensions. See
    /// [`wgpu_texture`](Texture::wgpu_texture) about replacement.
    pub fn wgpu_view(&self) -> wgpu::TextureView {
        self.gpu().view.clone()
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
