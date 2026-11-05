//! Image to KTX2: colour handling, mip chain and block compression.

use crate::compress::compress_level;
use crate::container::Ktx2File;
use crate::error::{Error, Result};
use crate::format::{BlockFormat, mip_count};
use crate::mips::{MipFilter, Surface, linear_to_srgb, srgb_to_linear};
use image::RgbaImage;

/// What [`encode_image`] produces.
#[derive(Debug, Clone, Copy)]
pub struct EncodeOptions {
    /// The block format to compress to.
    pub format: BlockFormat,
    /// True when the image holds sRGB colour; false when it holds data such
    /// as a normal map or mask. Ignored for BC4 and BC5, which are linear.
    pub srgb: bool,
    /// Write the whole mip chain; false writes level 0 alone.
    pub mips: bool,
    /// The filter that builds each mip level.
    pub filter: MipFilter,
    /// Use quicker, slightly worse BC7 settings.
    pub fast: bool,
    /// Multiply colour by alpha in linear light before compressing, for
    /// sRGB colour formats with alpha (BC3, BC7), so the texture blends like
    /// an uncompressed premultiplied one. The file records the choice.
    pub premultiply: bool,
}

impl EncodeOptions {
    /// BC7, sRGB, full mip chain, box filter, premultiplied alpha.
    pub fn new(format: BlockFormat) -> Self {
        Self {
            format,
            srgb: true,
            mips: true,
            filter: MipFilter::Box,
            fast: false,
            premultiply: true,
        }
    }

    fn is_srgb(&self) -> bool {
        self.srgb && self.format.is_color()
    }

    fn is_premultiplied(&self) -> bool {
        self.premultiply && self.is_srgb() && self.format.has_alpha()
    }
}

/// Compresses an image into a [`Ktx2File`].
///
/// sRGB colour is converted to linear light for premultiplication and mip
/// filtering, then re-encoded for each level, so edges do not darken. Data
/// formats are filtered as they stand. Alpha is always linear.
pub fn encode_image(image: &RgbaImage, opts: &EncodeOptions) -> Result<Ktx2File> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err(Error::Invalid("the image is empty".into()));
    }
    let srgb = opts.is_srgb();
    let premultiplied = opts.is_premultiplied();
    let mut level = Surface {
        width,
        height,
        texels: image
            .pixels()
            .map(|p| {
                let [r, g, b, a] = p.0.map(|v| f32::from(v) / 255.0);
                let mut rgb = [r, g, b];
                if srgb {
                    rgb = rgb.map(srgb_to_linear);
                }
                if premultiplied {
                    rgb = rgb.map(|v| v * a);
                }
                [rgb[0], rgb[1], rgb[2], a]
            })
            .collect(),
    };
    let count = if opts.mips {
        mip_count(width, height)
    } else {
        1
    };
    let mut levels = Vec::with_capacity(count as usize);
    for i in 0..count {
        if i > 0 {
            level = level.downsample(opts.filter);
        }
        let rgba = to_rgba8(&level, srgb);
        levels.push(compress_level(
            &rgba,
            level.width,
            level.height,
            opts.format,
            opts.fast,
        ));
    }
    Ok(Ktx2File {
        format: opts.format.vk_format(srgb),
        width,
        height,
        premultiplied,
        levels,
    })
}

/// Quantises a surface to RGBA8, re-encoding colour to sRGB when asked.
fn to_rgba8(s: &Surface, srgb: bool) -> Vec<u8> {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    s.texels
        .iter()
        .flat_map(|t| {
            let c = |v: f32| {
                if srgb {
                    q(linear_to_srgb(v.clamp(0.0, 1.0)))
                } else {
                    q(v)
                }
            };
            [c(t[0]), c(t[1]), c(t[2]), q(t[3])]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Decodes level `i` of a BC7/BC3/BC1/BC4/BC5 file to RGBA8 with texture2ddecoder.
    fn decode(file: &Ktx2File, i: usize) -> Vec<[u8; 4]> {
        let (w, h) = file.level_size(i as u32);
        let (block, _) = file.block_format().unwrap();
        let mut px = vec![0u32; (w * h) as usize];
        let data = &file.levels[i];
        let (w, h) = (w as usize, h as usize);
        match block {
            BlockFormat::Bc1 => texture2ddecoder::decode_bc1(data, w, h, &mut px),
            BlockFormat::Bc3 => texture2ddecoder::decode_bc3(data, w, h, &mut px),
            BlockFormat::Bc4 => texture2ddecoder::decode_bc4(data, w, h, &mut px),
            BlockFormat::Bc5 => texture2ddecoder::decode_bc5(data, w, h, &mut px),
            BlockFormat::Bc7 => texture2ddecoder::decode_bc7(data, w, h, &mut px),
        }
        .unwrap();
        // texture2ddecoder packs texels as 0xAARRGGBB in a native u32.
        px.iter()
            .map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8])
            .collect()
    }

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([
                (x * 255 / (w - 1)) as u8,
                (y * 255 / (h - 1)) as u8,
                ((x + y) * 255 / (w + h - 2)) as u8,
                255,
            ])
        })
    }

    fn max_error(a: &[[u8; 4]], b: &RgbaImage, channels: usize) -> i32 {
        a.iter()
            .zip(b.pixels())
            .flat_map(|(x, y)| {
                (0..channels).map(move |c| (i32::from(x[c]) - i32::from(y.0[c])).abs())
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn block_decodes_within_tolerance() {
        let img = gradient(32, 16);
        for (format, channels, tolerance) in [
            (BlockFormat::Bc7, 3, 6),
            (BlockFormat::Bc1, 3, 24),
            (BlockFormat::Bc3, 3, 24),
            (BlockFormat::Bc4, 1, 6),
            (BlockFormat::Bc5, 2, 6),
        ] {
            let mut opts = EncodeOptions::new(format);
            opts.srgb = false;
            opts.mips = false;
            let file = encode_image(&img, &opts).unwrap();
            let err = max_error(&decode(&file, 0), &img, channels);
            assert!(
                err <= tolerance,
                "{format:?}: max error {err} over {tolerance}"
            );
        }
    }

    #[test]
    fn mip_chain_dimensions_and_sizes() {
        let img = gradient(32, 8);
        for filter in [MipFilter::Box, MipFilter::Kaiser] {
            let mut opts = EncodeOptions::new(BlockFormat::Bc7);
            opts.filter = filter;
            opts.fast = true;
            let file = encode_image(&img, &opts).unwrap();
            assert_eq!(file.levels.len(), 6);
            assert_eq!(file.format, ktx2::Format::BC7_SRGB_BLOCK);
            assert_eq!(Ktx2File::parse(&file.to_bytes().unwrap()).unwrap(), file);
            let sizes: Vec<_> = (0..6).map(|i| file.level_size(i)).collect();
            assert_eq!(sizes, [(32, 8), (16, 4), (8, 2), (4, 1), (2, 1), (1, 1)]);
        }
    }

    #[test]
    fn srgb_mips_average_in_linear_light() {
        // A checkerboard of black and white averages to linear 0.5, which is
        // sRGB 188, not 128.
        let img = RgbaImage::from_fn(8, 8, |x, y| {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            Rgba([v, v, v, 255])
        });
        let mut opts = EncodeOptions::new(BlockFormat::Bc7);
        opts.fast = true;
        let file = encode_image(&img, &opts).unwrap();
        let top = decode(&file, 3)[0];
        assert!((i32::from(top[0]) - 188).abs() <= 4, "got {}", top[0]);
    }

    #[test]
    fn premultiply_applies_only_to_srgb_alpha_formats() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 128]));
        let mut opts = EncodeOptions::new(BlockFormat::Bc7);
        opts.mips = false;
        assert!(encode_image(&img, &opts).unwrap().premultiplied);
        opts.srgb = false;
        assert!(!encode_image(&img, &opts).unwrap().premultiplied);
        let bc1 = EncodeOptions::new(BlockFormat::Bc1);
        assert!(!encode_image(&img, &bc1).unwrap().premultiplied);
    }
}
