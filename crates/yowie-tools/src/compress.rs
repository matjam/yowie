//! Block compression of one RGBA8 mip level with intel_tex_2.

use crate::format::BlockFormat;
use intel_tex_2::{RSurface, RgSurface, RgbaSurface, bc1, bc3, bc4, bc5, bc7};

/// Compresses an RGBA8 image (`width * height * 4` bytes, tightly packed) to
/// `format` and returns the blocks, `ceil(w/4) * ceil(h/4)` of them.
///
/// Dimensions that are not multiples of 4 are padded by repeating the edge
/// texels, which keeps edge blocks from bleeding black. BC4 reads the red
/// channel and BC5 the red and green channels. `fast` selects quicker BC7
/// settings at a small cost in quality; other formats ignore it.
pub fn compress_level(
    rgba: &[u8],
    width: u32,
    height: u32,
    format: BlockFormat,
    fast: bool,
) -> Vec<u8> {
    let (pw, ph) = (width.next_multiple_of(4), height.next_multiple_of(4));
    let padded = pad(rgba, width, height, pw, ph);
    let stride = pw * 4;
    match format {
        BlockFormat::Bc1 => bc1::compress_blocks(&RgbaSurface {
            data: &padded,
            width: pw,
            height: ph,
            stride,
        }),
        BlockFormat::Bc3 => bc3::compress_blocks(&RgbaSurface {
            data: &padded,
            width: pw,
            height: ph,
            stride,
        }),
        BlockFormat::Bc4 => {
            let plane = planes::<1>(&padded);
            bc4::compress_blocks(&RSurface {
                data: &plane,
                width: pw,
                height: ph,
                stride: pw,
            })
        }
        BlockFormat::Bc5 => {
            let plane = planes::<2>(&padded);
            bc5::compress_blocks(&RgSurface {
                data: &plane,
                width: pw,
                height: ph,
                stride: pw * 2,
            })
        }
        BlockFormat::Bc7 => {
            let surface = RgbaSurface {
                data: &padded,
                width: pw,
                height: ph,
                stride,
            };
            let has_alpha = padded.as_chunks::<4>().0.iter().any(|t| t[3] != 255);
            let settings = match (has_alpha, fast) {
                (true, true) => bc7::alpha_very_fast_settings(),
                (true, false) => bc7::alpha_basic_settings(),
                (false, true) => bc7::opaque_fast_settings(),
                (false, false) => bc7::opaque_basic_settings(),
            };
            bc7::compress_blocks(&settings, &surface)
        }
    }
}

/// Keeps the first `N` channels of each RGBA texel, tightly packed, which is
/// the layout the one- and two-channel surfaces of intel_tex_2 expect.
fn planes<const N: usize>(rgba: &[u8]) -> Vec<u8> {
    let (texels, _) = rgba.as_chunks::<4>();
    texels.iter().flat_map(|t| &t[..N]).copied().collect()
}

/// Returns the image grown to `pw` by `ph` by repeating its last column and row.
fn pad(rgba: &[u8], w: u32, h: u32, pw: u32, ph: u32) -> Vec<u8> {
    if (w, h) == (pw, ph) {
        return rgba.to_vec();
    }
    let mut out = Vec::with_capacity((pw * ph * 4) as usize);
    for y in 0..ph {
        let sy = y.min(h - 1);
        for x in 0..pw {
            let sx = x.min(w - 1);
            let at = ((sy * w + sx) * 4) as usize;
            out.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_sizes_cover_partial_blocks() {
        for (w, h) in [(1, 1), (2, 2), (4, 4), (5, 3), (8, 1)] {
            let rgba = vec![128u8; (w * h * 4) as usize];
            for f in [
                BlockFormat::Bc1,
                BlockFormat::Bc3,
                BlockFormat::Bc4,
                BlockFormat::Bc5,
                BlockFormat::Bc7,
            ] {
                assert_eq!(
                    compress_level(&rgba, w, h, f, true).len(),
                    f.level_bytes(w, h),
                    "{f:?} {w}x{h}"
                );
            }
        }
    }
}
