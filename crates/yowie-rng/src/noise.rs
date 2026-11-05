//! Seeded gradient noise in two dimensions, and fractal sums of it, for
//! terrain heights, cloud cover and other smooth randomness.

use crate::Rng;

/// Two-dimensional gradient (Perlin) noise from a seed.
///
/// [`get`](Self::get) returns a smooth value in about -1 to 1 that is zero
/// at whole-number coordinates and repeats every 256 units. The same seed
/// gives the same field on every platform.
///
/// ```
/// use yowie_rng::Noise;
///
/// let n = Noise::new(7);
/// let h = n.fbm(12.3, 4.5, 5, 2.0, 0.5);
/// assert!(h.abs() <= 1.0);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Noise {
    perm: [u8; 512],
}

impl Noise {
    /// The noise field of `seed`.
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut p: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in (1..256).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for (i, v) in perm.iter_mut().enumerate() {
            *v = p[i & 255];
        }
        Self { perm }
    }

    fn gradient(&self, ix: i32, iy: i32, dx: f32, dy: f32) -> f32 {
        let a = self.perm[(ix & 255) as usize] as usize;
        let h = self.perm[a + (iy & 255) as usize] & 7;
        // Eight directions: the axes and the diagonals.
        const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
        let (gx, gy) = match h {
            0 => (1.0, 0.0),
            1 => (-1.0, 0.0),
            2 => (0.0, 1.0),
            3 => (0.0, -1.0),
            4 => (D, D),
            5 => (-D, D),
            6 => (D, -D),
            _ => (-D, -D),
        };
        gx * dx + gy * dy
    }

    /// The noise at `(x, y)`, in about -1 to 1.
    pub fn get(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = (x.floor(), y.floor());
        let (ix, iy) = (fx as i32, fy as i32);
        let (dx, dy) = (x - fx, y - fy);
        let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, v) = (fade(dx), fade(dy));
        let n00 = self.gradient(ix, iy, dx, dy);
        let n10 = self.gradient(ix + 1, iy, dx - 1.0, dy);
        let n01 = self.gradient(ix, iy + 1, dx, dy - 1.0);
        let n11 = self.gradient(ix + 1, iy + 1, dx - 1.0, dy - 1.0);
        let a = n00 + (n10 - n00) * u;
        let b = n01 + (n11 - n01) * u;
        // The largest value eight unit gradients reach is about 0.71;
        // scale it to about 1.
        ((a + (b - a) * v) * 1.4).clamp(-1.0, 1.0)
    }

    /// Fractal noise: `octaves` layers of [`get`](Self::get), each at
    /// `lacunarity` times the frequency and `gain` times the amplitude of
    /// the one before, normalised to about -1 to 1. Typical values are 5
    /// octaves, a lacunarity of 2 and a gain of 0.5.
    pub fn fbm(&self, x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
        let (mut sum, mut amp, mut freq, mut total) = (0.0, 1.0, 1.0, 0.0);
        for i in 0..octaves.max(1) {
            // Each octave is shifted so their zeros do not line up.
            let shift = i as f32 * 17.31;
            sum += self.get(x * freq + shift, y * freq - shift) * amp;
            total += amp;
            amp *= gain;
            freq *= lacunarity;
        }
        sum / total
    }

    /// Ridged fractal noise in 0 to 1: sharp crests where the noise
    /// crosses zero, for mountain ridges.
    pub fn ridged(&self, x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
        let (mut sum, mut amp, mut freq, mut total) = (0.0, 1.0, 1.0, 0.0);
        for i in 0..octaves.max(1) {
            let shift = i as f32 * 23.17;
            let r = 1.0 - self.get(x * freq + shift, y * freq + shift).abs();
            sum += r * r * amp;
            total += amp;
            amp *= gain;
            freq *= lacunarity;
        }
        sum / total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_seeded_smooth_and_bounded() {
        let a = Noise::new(1);
        let b = Noise::new(1);
        let c = Noise::new(2);
        let mut differs = false;
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.21);
            assert_eq!(a.get(x, y), b.get(x, y));
            differs |= a.get(x, y) != c.get(x, y);
            let v = a.fbm(x, y, 5, 2.0, 0.5);
            assert!((-1.0..=1.0).contains(&v), "{v}");
            let r = a.ridged(x, y, 4, 2.0, 0.5);
            assert!((0.0..=1.0).contains(&r), "{r}");
            // Nearby points have nearby values.
            assert!((a.get(x, y) - a.get(x + 0.01, y)).abs() < 0.05);
        }
        assert!(differs);
        assert_eq!(a.get(3.0, 4.0), 0.0);
    }
}
