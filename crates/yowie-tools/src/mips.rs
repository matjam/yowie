//! Mip chain generation on floating-point images.

use std::f32::consts::PI;

/// How each mip level is filtered down from the one above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MipFilter {
    /// Average of the 2x2 source texels. Fast, and slightly soft.
    #[default]
    Box,
    /// Kaiser-windowed sinc (radius 3, alpha 4), separable. Sharper than box.
    Kaiser,
}

/// An RGBA image of `f32` channels, rows top to bottom.
///
/// Colour channels are in whatever space the caller filters in (linear light
/// for sRGB colour, raw values for data); alpha is always linear.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// `width * height` texels of `[r, g, b, a]`.
    pub texels: Vec<[f32; 4]>,
}

impl Surface {
    /// Returns the next mip level: each dimension halved, never below 1.
    pub fn downsample(&self, filter: MipFilter) -> Surface {
        let nw = (self.width / 2).max(1);
        let nh = (self.height / 2).max(1);
        match filter {
            MipFilter::Box => self.box_down(nw, nh),
            MipFilter::Kaiser => {
                let wide = self.resample_axis(nw, true);
                wide.resample_axis(nh, false)
            }
        }
    }

    fn at(&self, x: u32, y: u32) -> [f32; 4] {
        self.texels[(y * self.width + x) as usize]
    }

    fn box_down(&self, nw: u32, nh: u32) -> Surface {
        let mut texels = Vec::with_capacity((nw * nh) as usize);
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let t = self.at(
                        (x * 2 + dx).min(self.width - 1),
                        (y * 2 + dy).min(self.height - 1),
                    );
                    for c in 0..4 {
                        sum[c] += t[c];
                    }
                }
                texels.push(sum.map(|v| v / 4.0));
            }
        }
        Surface {
            width: nw,
            height: nh,
            texels,
        }
    }

    /// Filters one axis down to `new_len` texels with the Kaiser kernel.
    fn resample_axis(&self, new_len: u32, horizontal: bool) -> Surface {
        let old_len = if horizontal { self.width } else { self.height };
        if new_len == old_len {
            return self.clone();
        }
        let scale = old_len as f32 / new_len as f32;
        let taps = kernel_taps(old_len, new_len, scale);
        let (w, h) = if horizontal {
            (new_len, self.height)
        } else {
            (self.width, new_len)
        };
        let mut texels = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let (pos, other) = if horizontal { (x, y) } else { (y, x) };
                let mut sum = [0.0f32; 4];
                for &(i, weight) in &taps[pos as usize] {
                    let t = if horizontal {
                        self.at(i, other)
                    } else {
                        self.at(other, i)
                    };
                    for c in 0..4 {
                        sum[c] += t[c] * weight;
                    }
                }
                texels.push(sum);
            }
        }
        Surface {
            width: w,
            height: h,
            texels,
        }
    }
}

/// Normalised source taps `(index, weight)` for each destination texel.
fn kernel_taps(old_len: u32, new_len: u32, scale: f32) -> Vec<Vec<(u32, f32)>> {
    const RADIUS: f32 = 3.0;
    let reach = (RADIUS * scale).ceil() as i32;
    (0..new_len)
        .map(|d| {
            let centre = (d as f32 + 0.5) * scale;
            let first = (centre - 0.5).floor() as i32 - reach;
            let mut taps: Vec<(u32, f32)> = (first..=first + 2 * reach + 1)
                .map(|i| {
                    let t = ((i as f32 + 0.5) - centre) / scale;
                    (i.clamp(0, old_len as i32 - 1) as u32, kaiser(t, RADIUS))
                })
                .collect();
            let total: f32 = taps.iter().map(|t| t.1).sum();
            if total.abs() > 1e-6 {
                for t in &mut taps {
                    t.1 /= total;
                }
            }
            taps
        })
        .collect()
}

/// A Kaiser-windowed sinc evaluated at `t` destination texels from the
/// centre, zero beyond `radius`.
fn kaiser(t: f32, radius: f32) -> f32 {
    const ALPHA: f32 = 4.0;
    if t.abs() >= radius {
        return 0.0;
    }
    let sinc = if t.abs() < 1e-6 {
        1.0
    } else {
        (PI * t).sin() / (PI * t)
    };
    let r = t / radius;
    sinc * bessel_i0(ALPHA * (1.0 - r * r).sqrt()) / bessel_i0(ALPHA)
}

/// The modified Bessel function of the first kind, order zero, by its series.
fn bessel_i0(x: f32) -> f32 {
    let mut sum = 1.0f32;
    let mut term = 1.0f32;
    let q = x * x / 4.0;
    for k in 1..24 {
        term *= q / (k * k) as f32;
        sum += term;
    }
    sum
}

/// The sRGB electro-optical transfer function: encoded `0..=1` to linear.
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The inverse of [`srgb_to_linear`]: linear `0..=1` to encoded.
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32, v: [f32; 4]) -> Surface {
        Surface {
            width: w,
            height: h,
            texels: vec![v; (w * h) as usize],
        }
    }

    #[test]
    fn dimensions_follow_the_chain() {
        for filter in [MipFilter::Box, MipFilter::Kaiser] {
            let mut s = flat(37, 8, [0.5; 4]);
            let mut sizes = vec![(s.width, s.height)];
            while s.width > 1 || s.height > 1 {
                s = s.downsample(filter);
                sizes.push((s.width, s.height));
            }
            assert_eq!(
                sizes,
                [(37, 8), (18, 4), (9, 2), (4, 1), (2, 1), (1, 1)],
                "{filter:?}"
            );
        }
    }

    #[test]
    fn flat_colour_is_preserved() {
        for filter in [MipFilter::Box, MipFilter::Kaiser] {
            let s = flat(16, 16, [0.2, 0.4, 0.6, 0.8]).downsample(filter);
            for t in &s.texels {
                for (got, want) in t.iter().zip([0.2, 0.4, 0.6, 0.8]) {
                    assert!((got - want).abs() < 1e-4, "{filter:?}: {got} vs {want}");
                }
            }
        }
    }

    #[test]
    fn box_averages_in_the_given_space() {
        let s = Surface {
            width: 2,
            height: 1,
            texels: vec![[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]],
        };
        assert_eq!(s.downsample(MipFilter::Box).texels[0], [0.5, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn srgb_round_trip() {
        for i in 0..=255 {
            let v = i as f32 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(v)) - v).abs() < 1e-4);
        }
    }
}
