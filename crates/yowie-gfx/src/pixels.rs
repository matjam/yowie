//! Converting game pixels into the form the GPU stores: premultiplied
//! alpha in linear light, re-encoded as sRGB bytes.

use std::sync::OnceLock;

use yowie_core::color::{linear_to_srgb, srgb_to_linear};

fn decode_table() -> &'static [f32; 256] {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)))
}

fn encode(linear: f32) -> u8 {
    (linear_to_srgb(linear).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Premultiplies sRGB RGBA8 pixels in place, multiplying in linear light
/// so an sRGB sampler decodes the right colour. Opaque pixels are left as
/// they are.
pub(crate) fn premultiply_srgb(rgba: &mut [u8]) {
    let table = decode_table();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 255 {
            continue;
        }
        if a == 0 {
            px[..3].fill(0);
            continue;
        }
        let af = f32::from(a) / 255.0;
        for c in &mut px[..3] {
            *c = encode(table[usize::from(*c)] * af);
        }
    }
}

/// Premultiplies linear RGBA8 pixels in place, for data textures that are
/// asked to be premultiplied.
pub(crate) fn premultiply_linear(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
}

/// Divides premultiplied sRGB RGBA8 pixels by their alpha in linear light,
/// the inverse of [`premultiply_srgb`], for pixels read back from the GPU.
pub(crate) fn unpremultiply_srgb(rgba: &mut [u8]) {
    let table = decode_table();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 255 || a == 0 {
            continue;
        }
        let af = f32::from(a) / 255.0;
        for c in &mut px[..3] {
            *c = encode((table[usize::from(*c)] / af).min(1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_cases() {
        let cases = [
            ([200, 100, 50, 255], [200, 100, 50, 255]),
            ([255, 255, 255, 0], [0, 0, 0, 0]),
            // Half-transparent white: linear 1.0 * 0.5 encodes to 188.
            ([255, 255, 255, 128], [188, 188, 188, 128]),
        ];
        for (input, want) in cases {
            let mut px = input;
            premultiply_srgb(&mut px);
            assert_eq!(px, want, "{input:?}");
        }
    }

    #[test]
    fn unpremultiply_inverts() {
        let mut px = [255, 128, 0, 128];
        premultiply_srgb(&mut px);
        unpremultiply_srgb(&mut px);
        assert!(
            px[0] >= 254 && px[1].abs_diff(128) <= 2 && px[2] == 0 && px[3] == 128,
            "{px:?}"
        );
    }

    #[test]
    fn premultiply_linear_halves() {
        let mut px = [200, 100, 0, 128];
        premultiply_linear(&mut px);
        assert_eq!(px, [100, 50, 0, 128]);
    }
}
