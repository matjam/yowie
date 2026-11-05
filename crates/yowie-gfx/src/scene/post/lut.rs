//! Colour lookup tables for the grade.

use std::sync::Arc;

use image::{Rgba, RgbaImage};

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::shared::Shared;

/// A colour lookup table: a cube of n by n by n colours that the grade
/// replaces each colour with, interpolating between entries.
///
/// The table is loaded from a strip image n squared pixels wide and n
/// high: n slices side by side, slice b holding red across and green
/// down at blue b, as [`Lut::neutral_image`] lays it out and image
/// editors export it after grading a screenshot that carries the neutral
/// strip. Entries are display (sRGB-encoded) colours, and the lookup runs
/// on display colours after tone mapping. Cloning is cheap; the GPU
/// table is shared.
#[derive(Clone)]
pub struct Lut {
    inner: Arc<LutInner>,
}

struct LutInner {
    id: u64,
    size: u32,
    view: wgpu::TextureView,
}

impl std::fmt::Debug for Lut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lut")
            .field("size", &self.inner.size)
            .finish_non_exhaustive()
    }
}

impl PartialEq for Lut {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Lut {
    /// The identity table of `size` slices (16 or 32 are usual, at least
    /// 2): paste it into a screenshot, grade the screenshot, crop the
    /// strip back out and load it with [`Lut::from_image`].
    pub fn neutral_image(size: u32) -> RgbaImage {
        let n = size.max(2);
        let v = |i: u32| ((i * 255 + (n - 1) / 2) / (n - 1)) as u8;
        RgbaImage::from_fn(n * n, n, |x, y| {
            let (b, r) = (x / n, x % n);
            Rgba([v(r), v(y), v(b), 255])
        })
    }

    /// Uploads a table from a strip image (see the type documentation).
    /// Fails when the image is not n squared by n pixels.
    pub fn from_image(gfx: &Graphics, image: &RgbaImage) -> Result<Lut> {
        Lut::create(&gfx.shared, image)
    }

    /// Decodes a strip image (PNG or another format the `image` crate
    /// reads) and uploads it.
    pub fn from_bytes(gfx: &Graphics, bytes: &[u8]) -> Result<Lut> {
        let image = image::load_from_memory(bytes)?.to_rgba8();
        Lut::create(&gfx.shared, &image)
    }

    /// The number of entries along each side of the cube.
    pub fn size(&self) -> u32 {
        self.inner.size
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.inner.view
    }

    pub(crate) fn id(&self) -> u64 {
        self.inner.id
    }

    pub(crate) fn create(shared: &Shared, image: &RgbaImage) -> Result<Lut> {
        let n = image.height();
        if n < 2 || image.width() != n * n {
            return Err(GfxError::Lut(format!(
                "a lookup table strip must be n squared by n pixels, got {}x{}",
                image.width(),
                image.height()
            )));
        }
        let max = shared.device().limits().max_texture_dimension_3d;
        if n > max {
            return Err(GfxError::InvalidSize {
                width: n,
                height: n,
                max,
            });
        }
        // Reorder the strip into slices: blue outermost, then green rows,
        // then red texels.
        let mut texels = Vec::with_capacity((n * n * n * 4) as usize);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    texels.extend_from_slice(&image.get_pixel(b * n + r, g).0);
                }
            }
        }
        let size = wgpu::Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: n,
        };
        let texture = shared.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("yowie colour lookup table"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        shared.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n * 4),
                rows_per_image: Some(n),
            },
            size,
        );
        Ok(Lut {
            inner: Arc::new(LutInner {
                id: crate::texture::next_id(),
                size: n,
                view: texture.create_view(&Default::default()),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_strip_lays_slices_side_by_side() {
        let img = Lut::neutral_image(4);
        assert_eq!(img.dimensions(), (16, 4));
        // Slice 2 (blue 170), red 1 (85), green 3 (255).
        assert_eq!(img.get_pixel(2 * 4 + 1, 3).0, [85, 255, 170, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(15, 3).0, [255, 255, 255, 255]);
    }
}
