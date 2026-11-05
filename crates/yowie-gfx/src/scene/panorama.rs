//! Equirectangular panoramas decoded to linear floating-point light, from
//! OpenEXR, Radiance HDR, or ordinary sRGB PNG and JPEG images.

use std::io::Cursor;

use yowie_core::Color;

use crate::error::{GfxError, Result};
use crate::half::f32_to_f16;
use crate::image_data::ImageData;

/// An equirectangular image of linear RGB light, row by row from the top
/// (straight up) to the bottom (straight down); the centre column faces
/// -Z.
#[derive(Clone, Debug, PartialEq)]
pub struct Panorama {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
}

impl Panorama {
    /// A panorama from `width * height` linear RGB pixels.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<[f32; 3]>) -> Result<Self> {
        if width == 0 || height == 0 || pixels.len() != width as usize * height as usize {
            return Err(GfxError::Panorama(format!(
                "{} pixels do not fill {width}x{height}",
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Decodes an OpenEXR file (`exr` crate: scanline or tiled, any
    /// compression it reads), a Radiance `.hdr` file, or a PNG or JPEG,
    /// whose sRGB colours are converted to linear light. The format is
    /// recognised from the first bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.starts_with(&[0x76, 0x2f, 0x31, 0x01]) {
            return decode_exr(bytes);
        }
        match ImageData::decode(bytes)? {
            ImageData::Rgba32F {
                width,
                height,
                pixels,
            } => {
                let rgb = pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| [p[0], p[1], p[2]])
                    .collect();
                Panorama::from_pixels(width, height, rgb)
            }
            ImageData::Rgba8 {
                width,
                height,
                pixels,
            } => {
                let rgb = pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| {
                        let c = Color::rgb8(p[0], p[1], p[2]).to_linear();
                        [c[0], c[1], c[2]]
                    })
                    .collect();
                Panorama::from_pixels(width, height, rgb)
            }
            ImageData::Compressed(_) => Err(GfxError::Panorama(
                "a block-compressed image cannot be a panorama".into(),
            )),
        }
    }

    /// The width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The linear RGB pixels, row by row.
    pub fn pixels(&self) -> &[[f32; 3]] {
        &self.pixels
    }

    /// The pixels as half floats with alpha 1, for an `Rgba16Float`
    /// texture.
    pub(crate) fn to_rgba16f(&self) -> Vec<u16> {
        let one = f32_to_f16(1.0);
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for p in &self.pixels {
            for c in p {
                out.push(f32_to_f16(c.clamp(0.0, 65000.0)));
            }
            out.push(one);
        }
        out
    }
}

fn decode_exr(bytes: &[u8]) -> Result<Panorama> {
    use exr::prelude::*;
    let image = read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |size, _| {
                (
                    size.width(),
                    vec![[0.0f32; 3]; size.width() * size.height()],
                )
            },
            |(w, px): &mut (usize, Vec<[f32; 3]>), pos, (r, g, b, _a): (f32, f32, f32, f32)| {
                px[pos.y() * *w + pos.x()] = [r, g, b];
            },
        )
        .first_valid_layer()
        .all_attributes()
        .from_buffered(Cursor::new(bytes))
        .map_err(|e| GfxError::Panorama(format!("OpenEXR: {e}")))?;
    let size = image.layer_data.size;
    let (_, pixels) = image.layer_data.channel_data.pixels;
    Panorama::from_pixels(size.width() as u32, size.height() as u32, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_exr_hdr_and_png() {
        // An EXR written by the exr crate.
        use exr::prelude::{Image, SpecificChannels, Vec2, WritableImage};
        let (w, h) = (8usize, 4usize);
        let mut buf = Cursor::new(Vec::new());
        let channels = SpecificChannels::rgb(|Vec2(x, y)| (x as f32 * 0.5, y as f32 * 2.0, 3.0f32));
        Image::from_channels((w, h), channels)
            .write()
            .to_buffered(&mut buf)
            .expect("write exr");
        let p = Panorama::decode(buf.get_ref()).expect("exr");
        assert_eq!((p.width(), p.height()), (8, 4));
        assert_eq!(p.pixels()[w + 3], [1.5, 2.0, 3.0]);

        // A PNG: sRGB 128 becomes about 0.216 linear.
        let img = image::RgbImage::from_pixel(4, 2, image::Rgb([128, 255, 0]));
        let mut png = Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png)
            .expect("png");
        let p = Panorama::decode(png.get_ref()).expect("png");
        assert!((p.pixels()[0][0] - 0.2158).abs() < 1e-3);
        assert_eq!(p.pixels()[0][1], 1.0);

        // A Radiance file keeps values above one.
        let hdr = image::Rgb32FImage::from_pixel(4, 2, image::Rgb([4.0, 0.5, 0.25]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb32F(hdr)
            .write_to(&mut out, image::ImageFormat::Hdr)
            .expect("hdr");
        let p = Panorama::decode(out.get_ref()).expect("hdr");
        assert!((p.pixels()[0][0] - 4.0).abs() < 0.05);
        assert!(Panorama::decode(b"not an image").is_err());
    }
}
