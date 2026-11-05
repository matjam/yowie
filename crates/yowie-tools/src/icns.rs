//! Apple icon (`.icns`) generation from a PNG.

use crate::error::{Error, Result};
use image::codecs::png::PngEncoder;
use image::imageops::{FilterType, resize};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

/// The icon types written, as `(type code, pixel size)`. The `ic1x` codes
/// hold PNG data, which macOS 10.7 and later read.
const ENTRIES: [(&[u8; 4], u32); 8] = [
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];

/// Builds the bytes of an `.icns` file from an image.
///
/// A non-square image is centred on a transparent square first. Each size
/// from 32 to 1024 pixels up to the size of the source is resized with a
/// Lanczos filter and stored as PNG; a source below 32 pixels gets the
/// 32-pixel entries by upscaling. Returns an error for an empty image.
pub fn icns_from_image(source: &RgbaImage) -> Result<Vec<u8>> {
    let (sw, sh) = source.dimensions();
    if sw == 0 || sh == 0 {
        return Err(Error::Invalid("the icon is empty".into()));
    }
    let squared;
    let source = if sw == sh {
        source
    } else {
        let side = sw.max(sh);
        let mut canvas = RgbaImage::new(side, side);
        image::imageops::replace(
            &mut canvas,
            source,
            i64::from((side - sw) / 2),
            i64::from((side - sh) / 2),
        );
        squared = canvas;
        &squared
    };
    let w = source.width();
    let mut body = Vec::new();
    for (code, size) in ENTRIES {
        if size > w && size != 32 {
            continue;
        }
        let scaled;
        let image = if size == w {
            source
        } else {
            scaled = resize(source, size, size, FilterType::Lanczos3);
            &scaled
        };
        let mut png = Vec::new();
        PngEncoder::new(&mut png).write_image(
            image.as_raw(),
            size,
            size,
            ExtendedColorType::Rgba8,
        )?;
        body.extend_from_slice(code);
        body.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
        body.extend_from_slice(&png);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Reads a PNG (or JPEG) file and builds `.icns` bytes with [`icns_from_image`].
pub fn icns_from_file(path: &std::path::Path) -> Result<Vec<u8>> {
    icns_from_image(&image::open(path)?.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Lists `(type, payload)` of each entry, checking the lengths add up.
    fn entries(data: &[u8]) -> Vec<(String, Vec<u8>)> {
        assert_eq!(&data[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize,
            data.len()
        );
        let mut at = 8;
        let mut out = Vec::new();
        while at < data.len() {
            let len = u32::from_be_bytes(data[at + 4..at + 8].try_into().unwrap()) as usize;
            out.push((
                String::from_utf8(data[at..at + 4].to_vec()).unwrap(),
                data[at + 8..at + len].to_vec(),
            ));
            at += len;
        }
        assert_eq!(at, data.len());
        out
    }

    #[test]
    fn sizes_follow_the_source() {
        let src = RgbaImage::from_pixel(256, 256, Rgba([200, 30, 30, 255]));
        let found = entries(&icns_from_image(&src).unwrap());
        let codes: Vec<_> = found.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(codes, ["ic11", "ic12", "ic07", "ic13", "ic08"]);
        for (_, png) in &found {
            assert_eq!(&png[1..4], b"PNG");
        }
        let big = entries(&icns_from_image(&RgbaImage::new(1024, 1024)).unwrap());
        assert_eq!(big.len(), 8);
    }

    #[test]
    fn non_square_is_padded_and_empty_is_rejected() {
        let found = entries(&icns_from_image(&RgbaImage::new(64, 32)).unwrap());
        assert_eq!(found.len(), 2);
        assert!(icns_from_image(&RgbaImage::new(0, 0)).is_err());
    }
}
