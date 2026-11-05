//! The 47 distinct blob neighbourhoods and the six-tile template that
//! produces their tiles.

use std::sync::OnceLock;

use image::{RgbaImage, imageops};

use super::layout::Dir;

struct Tables {
    masks: [u8; 47],
    index: [u8; 256],
}

/// Clears each diagonal bit unless both edges beside it are set: a
/// diagonal neighbour only changes the tile when the two edges next to it
/// connect. Only 47 of the 256 masks remain distinct.
fn normalize(mut mask: u8) -> u8 {
    let bit = |d: Dir| 1u8 << d.index();
    for (diag, a, b) in [
        (Dir::NE, Dir::N, Dir::E),
        (Dir::SE, Dir::S, Dir::E),
        (Dir::SW, Dir::S, Dir::W),
        (Dir::NW, Dir::N, Dir::W),
    ] {
        if mask & bit(a) == 0 || mask & bit(b) == 0 {
            mask &= !bit(diag);
        }
    }
    mask
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut seen = [false; 256];
        for m in 0..=255u8 {
            seen[normalize(m) as usize] = true;
        }
        let mut masks = [0u8; 47];
        let mut n = 0;
        for (m, s) in seen.iter().enumerate() {
            if *s {
                masks[n] = m as u8;
                n += 1;
            }
        }
        debug_assert_eq!(n, 47);
        let mut index = [0u8; 256];
        for m in 0..=255u8 {
            let norm = normalize(m);
            index[m as usize] = masks.partition_point(|&x| x < norm) as u8;
        }
        Tables { masks, index }
    })
}

/// Maps a raw 8-bit neighbour mask (bit `d` set when the neighbour in
/// direction `d` connects; see [`Dir`]) to its place, 0 to 46, in the
/// canonical blob order that [`Rules::blob47`](super::Rules::blob47)
/// frames use.
pub fn blob_index(mask: u8) -> usize {
    tables().index[mask as usize] as usize
}

/// The 47 canonical masks in frame order, ascending, for building or
/// checking a sheet layout.
pub fn blob_masks() -> [u8; 47] {
    tables().masks
}

/// Composes the 47 blob tiles from a six-tile template, so an artist draws
/// six tiles instead of 47.
///
/// The template is two tiles wide and three tall, each tile `tile` pixels
/// square:
///
/// ```text
/// inner  preview
///  TL      TR
///  BL      BR
/// ```
///
/// TL, TR, BL and BR are the four corner tiles of a filled two-by-two
/// block; they supply the outer corners, the edges and the interior. The
/// inner tile holds the four inside-corner pieces, drawn as if a hole sat
/// at each diagonal. The preview tile is not read; RPG Maker style
/// autotile blocks put a display tile there.
///
/// Every output tile is built from four quarter tiles chosen by the
/// neighbour mask. The result is a sheet eight tiles wide holding the 47
/// tiles in canonical order: upload it, cut it with a sheet of the same
/// tile size, and use frame `i` for the `i`-th canonical mask (the frames
/// of [`Rules::blob47`](super::Rules::blob47) are `0..47`).
///
/// `tile` must be a positive even number and `template` at least two tiles
/// wide and three tall; otherwise `None`.
pub fn expand_blob(template: &RgbaImage, tile: u32) -> Option<RgbaImage> {
    if tile == 0
        || !tile.is_multiple_of(2)
        || template.width() < 2 * tile
        || template.height() < 3 * tile
    {
        return None;
    }
    const COLUMNS: u32 = 8;
    let half = tile / 2;
    let mut out = RgbaImage::new(COLUMNS * tile, 47u32.div_ceil(COLUMNS) * tile);
    // Template tiles by position.
    const INNER: usize = 0;
    const TL: usize = 2;
    const TR: usize = 3;
    const BL: usize = 4;
    const BR: usize = 5;
    let pos = |t: usize| ((t % 2) as u32 * tile, (t / 2) as u32 * tile);
    // Quarter q of a tile: 0 top-left, 1 top-right, 2 bottom-left, 3
    // bottom-right.
    let quarter = |q: usize| ((q % 2) as u32 * half, (q / 2) as u32 * half);
    for (i, mask) in blob_masks().into_iter().enumerate() {
        let dst = ((i as u32 % COLUMNS) * tile, (i as u32 / COLUMNS) * tile);
        let has = |d: Dir| mask & (1 << d.index()) != 0;
        let (n, e, s, w) = (has(Dir::N), has(Dir::E), has(Dir::S), has(Dir::W));
        let mut blit = |dq: usize, t: usize, sq: usize| {
            let (tx, ty) = pos(t);
            let (sx, sy) = quarter(sq);
            let (dx, dy) = quarter(dq);
            let piece = imageops::crop_imm(template, tx + sx, ty + sy, half, half).to_image();
            imageops::replace(
                &mut out,
                &piece,
                i64::from(dst.0 + dx),
                i64::from(dst.1 + dy),
            );
        };
        // Each quadrant is shaped by its two edge neighbours; the
        // diagonal decides interior against inside corner. Interior
        // quarters come from the diagonally opposite block tile.
        match (n, w) {
            (false, false) => blit(0, TL, 0),
            (true, false) => blit(0, TL, 2),
            (false, true) => blit(0, TL, 1),
            _ if has(Dir::NW) => blit(0, BR, 0),
            _ => blit(0, INNER, 0),
        }
        match (n, e) {
            (false, false) => blit(1, TR, 1),
            (true, false) => blit(1, TR, 3),
            (false, true) => blit(1, TR, 0),
            _ if has(Dir::NE) => blit(1, BL, 1),
            _ => blit(1, INNER, 1),
        }
        match (s, w) {
            (false, false) => blit(2, BL, 2),
            (true, false) => blit(2, BL, 0),
            (false, true) => blit(2, BL, 3),
            _ if has(Dir::SW) => blit(2, TR, 2),
            _ => blit(2, INNER, 2),
        }
        match (s, e) {
            (false, false) => blit(3, BR, 3),
            (true, false) => blit(3, BR, 1),
            (false, true) => blit(3, BR, 2),
            _ if has(Dir::SE) => blit(3, TL, 3),
            _ => blit(3, INNER, 3),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical masks as Bunyip's `BlobMasks` returns them.
    const BUNYIP_MASKS: [u8; 47] = [
        0, 1, 4, 5, 7, 16, 17, 20, 21, 23, 28, 29, 31, 64, 65, 68, 69, 71, 80, 81, 84, 85, 87, 92,
        93, 95, 112, 113, 116, 117, 119, 124, 125, 127, 193, 197, 199, 209, 213, 215, 221, 223,
        241, 245, 247, 253, 255,
    ];

    /// Bunyip's `BlobIndex` for every raw mask.
    #[rustfmt::skip]
    const BUNYIP_INDEX: [u8; 256] = [
        0, 1, 0, 1, 2, 3, 2, 4, 0, 1, 0, 1, 2, 3, 2, 4, 5, 6, 5, 6, 7, 8, 7, 9, 5, 6, 5, 6, 10, 11,
        10, 12, 0, 1, 0, 1, 2, 3, 2, 4, 0, 1, 0, 1, 2, 3, 2, 4, 5, 6, 5, 6, 7, 8, 7, 9, 5, 6, 5, 6,
        10, 11, 10, 12, 13, 14, 13, 14, 15, 16, 15, 17, 13, 14, 13, 14, 15, 16, 15, 17, 18, 19, 18,
        19, 20, 21, 20, 22, 18, 19, 18, 19, 23, 24, 23, 25, 13, 14, 13, 14, 15, 16, 15, 17, 13, 14,
        13, 14, 15, 16, 15, 17, 26, 27, 26, 27, 28, 29, 28, 30, 26, 27, 26, 27, 31, 32, 31, 33, 0,
        1, 0, 1, 2, 3, 2, 4, 0, 1, 0, 1, 2, 3, 2, 4, 5, 6, 5, 6, 7, 8, 7, 9, 5, 6, 5, 6, 10, 11,
        10, 12, 0, 1, 0, 1, 2, 3, 2, 4, 0, 1, 0, 1, 2, 3, 2, 4, 5, 6, 5, 6, 7, 8, 7, 9, 5, 6, 5, 6,
        10, 11, 10, 12, 13, 34, 13, 34, 15, 35, 15, 36, 13, 34, 13, 34, 15, 35, 15, 36, 18, 37, 18,
        37, 20, 38, 20, 39, 18, 37, 18, 37, 23, 40, 23, 41, 13, 34, 13, 34, 15, 35, 15, 36, 13, 34,
        13, 34, 15, 35, 15, 36, 26, 42, 26, 42, 28, 43, 28, 44, 26, 42, 26, 42, 31, 45, 31, 46,
    ];

    #[test]
    fn mask_to_tile_table_matches_bunyip() {
        assert_eq!(blob_masks(), BUNYIP_MASKS);
        for m in 0..=255u8 {
            assert_eq!(blob_index(m), BUNYIP_INDEX[m as usize] as usize, "mask {m}");
        }
    }

    #[test]
    fn expansion_builds_tiles_from_template_quarters() {
        // A template where every quarter has its own colour.
        let tile = 4;
        let mut t = RgbaImage::new(8, 12);
        for (x, y, p) in t.enumerate_pixels_mut() {
            let (tx, ty, qx, qy) = (x / 4, y / 4, (x % 4) / 2, (y % 4) / 2);
            let id = (ty * 2 + tx) * 4 + qy * 2 + qx;
            *p = image::Rgba([id as u8 * 10, 0, 0, 255]);
        }
        let out = expand_blob(&t, tile).expect("valid template");
        assert_eq!(out.dimensions(), (32, 24));
        // Mask 0 is an isolated tile: its four quarters are the outer
        // corners of the block: TL quarter 0, TR quarter 1, BL quarter 2,
        // BR quarter 3.
        let px = |x: u32, y: u32| out.get_pixel(x, y).0[0] / 10;
        assert_eq!(px(0, 0), 2 * 4);
        assert_eq!(px(2, 0), 3 * 4 + 1);
        assert_eq!(px(0, 2), 4 * 4 + 2);
        assert_eq!(px(2, 2), 5 * 4 + 3);
        // The fully surrounded tile (the last) is all interior and inner
        // quarters never appear because every diagonal connects.
        let last = |x: u32, y: u32| out.get_pixel(6 * 4 + x, 5 * 4 + y).0[0] / 10;
        assert_eq!(last(0, 0), 5 * 4);
        assert_eq!(last(2, 0), 4 * 4 + 1);
        assert_eq!(last(0, 2), 3 * 4 + 2);
        assert_eq!(last(2, 2), 2 * 4 + 3);
        assert!(expand_blob(&t, 3).is_none());
        assert!(expand_blob(&RgbaImage::new(4, 4), 4).is_none());
    }
}
