//! Decoded image data that can be uploaded to a texture: 8-bit colour,
//! Radiance HDR, or a block-compressed KTX2 file.

use yowie_core::{UVec2, uvec2};
use yowie_ktx2::Ktx2File;

use crate::error::{GfxError, Result};

/// The CPU side of a texture, decoded from a file and ready to upload with
/// [`Texture::from_data`](crate::Texture::from_data) or to swap into a
/// texture with [`Texture::replace_data`](crate::Texture::replace_data).
///
/// Decoding touches no GPU object, so it can run on a worker thread (the
/// asset loaders do).
#[derive(Clone, Debug, PartialEq)]
pub enum ImageData {
    /// 8-bit RGBA, straight alpha unless the texture options say
    /// otherwise, rows from the top.
    Rgba8 {
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// `width * height * 4` bytes.
        pixels: Vec<u8>,
    },
    /// Linear floating-point RGBA, from a Radiance HDR file. It is stored
    /// as 16-bit floats and is not colour converted.
    Rgba32F {
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// `width * height * 4` floats.
        pixels: Vec<f32>,
    },
    /// A KTX2 file of BC1, BC3, BC4, BC5 or BC7 blocks with its mip chain.
    /// Uploading needs [`Graphics::supports_bc`](crate::Graphics::supports_bc).
    /// Colour is used as stored, so author it with premultiplied alpha
    /// (`yowie-tex` does by default).
    Compressed(Ktx2File),
}

impl ImageData {
    /// Decodes a file's bytes, recognising the format from their start:
    /// KTX2, Radiance HDR, or PNG and JPEG through the `image` crate.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if Ktx2File::is_ktx2(bytes) {
            return Ok(Self::Compressed(Ktx2File::parse(bytes)?));
        }
        if bytes.starts_with(b"#?RADIANCE") || bytes.starts_with(b"#?RGBE") {
            let img =
                image::load_from_memory_with_format(bytes, image::ImageFormat::Hdr)?.to_rgba32f();
            return Ok(Self::Rgba32F {
                width: img.width(),
                height: img.height(),
                pixels: img.into_raw(),
            });
        }
        let img = image::load_from_memory(bytes)?.to_rgba8();
        Ok(Self::Rgba8 {
            width: img.width(),
            height: img.height(),
            pixels: img.into_raw(),
        })
    }

    /// The size of the full-resolution image in texels.
    pub fn size(&self) -> UVec2 {
        match self {
            Self::Rgba8 { width, height, .. } | Self::Rgba32F { width, height, .. } => {
                uvec2(*width, *height)
            }
            Self::Compressed(f) => uvec2(f.width, f.height),
        }
    }

    /// Checks that the pixel buffers match the stated size.
    pub(crate) fn check(&self) -> Result<()> {
        let (len, size, per) = match self {
            Self::Rgba8 {
                width,
                height,
                pixels,
            } => (pixels.len(), (*width, *height), 4),
            Self::Rgba32F {
                width,
                height,
                pixels,
            } => (pixels.len(), (*width, *height), 4),
            Self::Compressed(_) => return Ok(()),
        };
        let expected = size.0 as usize * size.1 as usize * per;
        if len == expected {
            Ok(())
        } else {
            Err(GfxError::PixelDataSize { expected, got: len })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_and_rejects_garbage() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 4]));
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap_or_else(|e| panic!("encode: {e}"));
        match ImageData::decode(&png) {
            Ok(ImageData::Rgba8 {
                width,
                height,
                pixels,
            }) => {
                assert_eq!((width, height), (3, 2));
                assert_eq!(&pixels[..4], &[1, 2, 3, 4]);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(ImageData::decode(b"not an image").is_err());
    }

    #[test]
    fn decodes_radiance_hdr() {
        // A 2x1 flat-format Radiance file: pixel RGBE (128,64,32, 129).
        let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 2\n".to_vec();
        hdr.extend_from_slice(&[128, 64, 32, 129, 0, 0, 0, 0]);
        match ImageData::decode(&hdr) {
            Ok(ImageData::Rgba32F { width, pixels, .. }) => {
                assert_eq!(width, 2);
                assert!((pixels[0] - 1.0).abs() < 0.02, "{pixels:?}");
                assert_eq!(pixels[3], 1.0);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
