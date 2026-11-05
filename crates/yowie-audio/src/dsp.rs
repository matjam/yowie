//! Small filters and helpers shared by the mixer, the head model and the reverb.

use std::f32::consts::PI;

/// Returns zero for a value whose magnitude is below 1e-15, and the value otherwise.
///
/// Filter and reverb state that decays on silence would reach the subnormal range and
/// stay there, and operations on subnormals are slow on x86. A value below 1e-15 is
/// 300 dB down, so flushing it changes nothing audible.
#[inline]
pub(crate) fn flush_tiny(x: f32) -> f32 {
    if x.abs() < 1e-15 { 0.0 } else { x }
}

/// Flushes every value of a slice with [`flush_tiny`].
pub(crate) fn flush_slice(s: &mut [f32]) {
    for v in s {
        *v = flush_tiny(*v);
    }
}

/// Coefficients of a second-order Butterworth low-pass filter.
///
/// Coefficients are kept apart from [`LowPass`] state so a filter can be retuned while
/// it runs without a reset and so without a click.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    /// Coefficients for a cutoff in Hz at the given sample rate.
    pub(crate) fn lowpass(cutoff: f32, rate: u32) -> Self {
        let cutoff = cutoff.min(rate as f32 * 0.49).max(1.0);
        let w0 = 2.0 * std::f64::consts::PI * f64::from(cutoff) / f64::from(rate);
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let b0 = ((1.0 - cs) / 2.0 / a0) as f32;
        Self {
            b0,
            b1: ((1.0 - cs) / a0) as f32,
            b2: b0,
            a1: (-2.0 * cs / a0) as f32,
            a2: ((1.0 - alpha) / a0) as f32,
        }
    }
}

/// Running state of a [`Biquad`] over stereo frames, in transposed direct form II.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LowPass {
    l0: f32,
    l1: f32,
    r0: f32,
    r1: f32,
}

impl LowPass {
    /// Filters interleaved stereo in place.
    pub(crate) fn process(&mut self, c: &Biquad, buf: &mut [f32]) {
        let (mut l0, mut l1, mut r0, mut r1) = (self.l0, self.l1, self.r0, self.r1);
        for f in buf.chunks_exact_mut(2) {
            let x = f[0];
            let y = c.b0 * x + l0;
            l0 = c.b1 * x - c.a1 * y + l1;
            l1 = c.b2 * x - c.a2 * y;
            f[0] = y;
            let x = f[1];
            let y = c.b0 * x + r0;
            r0 = c.b1 * x - c.a1 * y + r1;
            r1 = c.b2 * x - c.a2 * y;
            f[1] = y;
        }
        self.l0 = flush_tiny(l0);
        self.l1 = flush_tiny(l1);
        self.r0 = flush_tiny(r0);
        self.r1 = flush_tiny(r1);
    }

    /// Filters mono samples in place with the left channel's state, then copies that
    /// state to the right channel so a later switch to stereo continues from it.
    pub(crate) fn process_mono(&mut self, c: &Biquad, buf: &mut [f32]) {
        let (mut l0, mut l1) = (self.l0, self.l1);
        for v in buf {
            let x = *v;
            let y = c.b0 * x + l0;
            l0 = c.b1 * x - c.a1 * y + l1;
            l1 = c.b2 * x - c.a2 * y;
            *v = y;
        }
        self.l0 = flush_tiny(l0);
        self.l1 = flush_tiny(l1);
        self.r0 = self.l0;
        self.r1 = self.l1;
    }
}

/// Coefficient of a one-pole low-pass at `cutoff` Hz; 1 means no filtering.
pub(crate) fn one_pole(cutoff: f32, rate: u32) -> f32 {
    (1.0 - (-2.0 * PI * cutoff / rate as f32).exp()).min(1.0)
}

/// Clamps to `0..=1`, mapping NaN to 0.
pub(crate) fn clamp01(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|v| v * v).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn sine(freq: f32, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames * 2)
            .map(|i| (2.0 * PI * freq * (i / 2) as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn lowpass_passes_low_and_cuts_high() {
        let rate = 48_000;
        let c = Biquad::lowpass(1000.0, rate);
        for (freq, lo, hi) in [(100.0, 0.6, 0.8), (1000.0, 0.45, 0.6), (8000.0, 0.0, 0.02)] {
            let mut s = LowPass::default();
            let mut buf = sine(freq, rate, 4800);
            s.process(&c, &mut buf);
            let level = rms(&buf[2000..]);
            assert!(level >= lo && level <= hi, "{freq} Hz gave {level}");
        }
    }

    #[test]
    fn flush_tiny_clears_denormal_range() {
        for (input, want) in [(1e-20, 0.0), (-1e-20, 0.0), (0.5, 0.5), (-1e-3, -1e-3)] {
            assert_eq!(flush_tiny(input), want);
        }
    }
}
