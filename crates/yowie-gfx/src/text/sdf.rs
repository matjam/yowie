//! Signed distance fields for glyphs: each glyph is rasterised once at a
//! large size and reduced to a small tile of distances, which the SDF
//! shader turns back into a sharp edge at any size.

/// The em size of a distance-field tile in texels.
pub(crate) const SDF_EM: f32 = 32.0;
/// The size glyphs are rasterised at before reduction.
pub(crate) const SOURCE_EM: f32 = 128.0;
/// Source texels per tile texel.
const FACTOR: u32 = 4;
/// Distance covered either side of the outline, in tile texels.
const SPREAD: u32 = 6;
const INF: f32 = 1e20;

/// A glyph's distance tile and where it sits relative to the pen, in tile
/// texels (so scale by `font_size / SDF_EM` to draw).
#[derive(Clone, Debug)]
pub(crate) struct SdfTile {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// One byte per texel: 128 on the outline, higher inside.
    pub(crate) data: Vec<u8>,
    /// Offset of the tile's left edge from the pen.
    pub(crate) left: f32,
    /// Height of the tile's top edge above the baseline.
    pub(crate) top: f32,
}

/// One-dimensional squared distance transform of `f` into `d`, by
/// Felzenszwalb and Huttenlocher's lower envelope of parabolas.
fn transform_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    let cross = |q: usize, p: usize| {
        ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * (q as f32 - p as f32))
    };
    let mut k = 0;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        let mut s = cross(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = cross(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = INF;
    }
    k = 0;
    for (q, slot) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        *slot = (q as f32 - p as f32).powi(2) + f[p];
    }
}

/// Squared distance from every cell to the nearest cell where `feature` is
/// true.
fn squared_distance(feature: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mut grid: Vec<f32> = feature.iter().map(|f| if *f { 0.0 } else { INF }).collect();
    let n = w.max(h);
    let (mut line, mut out) = (vec![0.0f32; n], vec![0.0f32; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0f32; n + 1]);
    for x in 0..w {
        for y in 0..h {
            line[y] = grid[y * w + x];
        }
        transform_1d(&line[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        line[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        transform_1d(&line[..w], &mut out[..w], &mut v, &mut z);
        grid[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    grid
}

/// Builds the tile for a glyph from its coverage mask rendered at
/// [`SOURCE_EM`]. `left` and `top` are the mask's placement (texels right
/// of the pen and above the baseline). `None` for an empty mask.
pub(crate) fn tile(mask: &[u8], mw: u32, mh: u32, left: i32, top: i32) -> Option<SdfTile> {
    if mw == 0 || mh == 0 {
        return None;
    }
    let pad = SPREAD * FACTOR;
    let round_up = |n: u32| n.div_ceil(FACTOR) * FACTOR;
    let (bw, bh) = (round_up(mw + 2 * pad), round_up(mh + 2 * pad));
    let (w, h) = (bw as usize, bh as usize);
    let mut inside = vec![false; w * h];
    for y in 0..mh as usize {
        for x in 0..mw as usize {
            inside[(y + pad as usize) * w + x + pad as usize] = mask[y * mw as usize + x] >= 128;
        }
    }
    let outside: Vec<bool> = inside.iter().map(|i| !i).collect();
    let to_inside = squared_distance(&inside, w, h);
    let to_outside = squared_distance(&outside, w, h);
    let signed = |i: usize| {
        if inside[i] {
            to_outside[i].sqrt() - 0.5
        } else {
            0.5 - to_inside[i].sqrt()
        }
    };
    let (tw, th) = (bw / FACTOR, bh / FACTOR);
    let range = 2.0 * (SPREAD * FACTOR) as f32;
    let mut data = Vec::with_capacity((tw * th) as usize);
    for ty in 0..th as usize {
        for tx in 0..tw as usize {
            let mut sum = 0.0;
            for sy in 0..FACTOR as usize {
                for sx in 0..FACTOR as usize {
                    sum += signed((ty * FACTOR as usize + sy) * w + tx * FACTOR as usize + sx);
                }
            }
            let avg = sum / (FACTOR * FACTOR) as f32;
            data.push(((0.5 + avg / range).clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    Some(SdfTile {
        width: tw,
        height: th,
        data,
        left: (left as f32 - pad as f32) / FACTOR as f32,
        top: (top as f32 + pad as f32) / FACTOR as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_tile_has_the_outline_at_one_half() {
        let mask = vec![255u8; 32 * 32];
        let Some(t) = tile(&mask, 32, 32, 0, 32) else {
            panic!("tile");
        };
        let at = |x: u32, y: u32| t.data[(y * t.width + x) as usize];
        let pad = SPREAD;
        let mid = pad + 4;
        assert!(
            at(mid, mid) > 185,
            "deep inside is near 1: {}",
            at(mid, mid)
        );
        assert!(at(0, 0) < 30, "far outside is near 0: {}", at(0, 0));
        let edge = at(pad, mid);
        assert!((100..=156).contains(&edge), "on the outline: {edge}");
        assert!(at(pad - 2, mid) < at(pad + 2, mid));
        assert!((t.left + SPREAD as f32).abs() < 1e-4);
    }

    #[test]
    fn empty_mask_has_no_tile() {
        assert!(tile(&[], 0, 0, 0, 0).is_none());
    }

    #[test]
    fn distance_transform_counts_steps() {
        let mut feature = vec![false; 7];
        feature[3] = true;
        let d = squared_distance(&feature, 7, 1);
        assert_eq!(d, [9.0, 4.0, 1.0, 0.0, 1.0, 4.0, 9.0]);
    }
}
