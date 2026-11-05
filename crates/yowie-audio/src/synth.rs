//! Tone synthesis for game sound effects.
//!
//! A [`Tone`] is an oscillator shaped by an ADSR envelope. [`Tone::render`] produces
//! mono [`Pcm`] that [`Mixer::sound`](crate::Mixer::sound) turns into a [`Sound`](crate::Sound).

use crate::sound::Pcm;
use std::f32::consts::TAU;

/// The oscillator shape of a [`Tone`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Waveform {
    /// A pure sine.
    #[default]
    Sine,
    /// A square wave, band-limited with PolyBLEP.
    Square,
    /// A rising sawtooth, band-limited with PolyBLEP.
    Saw,
    /// A triangle.
    Triangle,
    /// White noise from a fixed-seed generator, so renders are repeatable.
    Noise,
}

/// An attack, decay, sustain, release envelope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adsr {
    /// Seconds to rise from silence to full level.
    pub attack: f32,
    /// Seconds to fall from full level to `sustain`.
    pub decay: f32,
    /// Level held while the note lasts, 0 to 1.
    pub sustain: f32,
    /// Seconds to fall from the level at the end of the note to silence.
    pub release: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self { attack: 0.005, decay: 0.05, sustain: 0.7, release: 0.1 }
    }
}

impl Adsr {
    /// The envelope level `t` seconds after the note started, for a note held `hold` seconds.
    pub fn level(&self, t: f32, hold: f32) -> f32 {
        let sustain = self.sustain.clamp(0.0, 1.0);
        let held = |t: f32| {
            if t < self.attack {
                t / self.attack.max(1e-9)
            } else if t < self.attack + self.decay {
                1.0 + (sustain - 1.0) * (t - self.attack) / self.decay.max(1e-9)
            } else {
                sustain
            }
        };
        if t <= hold {
            held(t)
        } else if self.release > 0.0 && t < hold + self.release {
            held(hold) * (1.0 - (t - hold) / self.release)
        } else {
            0.0
        }
    }
}

/// A note: one oscillator through an envelope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    /// Oscillator shape.
    pub waveform: Waveform,
    /// Frequency in Hz at the start of the note.
    pub frequency: f32,
    /// Frequency in Hz at the end of the held note; the pitch glides linearly. `None` holds the pitch.
    pub sweep_to: Option<f32>,
    /// Seconds the note is held before its release begins.
    pub duration: f32,
    /// Peak amplitude, 0 to 1.
    pub volume: f32,
    /// Envelope.
    pub adsr: Adsr,
}

impl Default for Tone {
    fn default() -> Self {
        Self { waveform: Waveform::Sine, frequency: 440.0, sweep_to: None, duration: 0.25, volume: 0.5, adsr: Adsr::default() }
    }
}

/// PolyBLEP correction for a discontinuity at phase 0 with phase increment `dt`.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

impl Tone {
    /// Renders the note, including its release, as mono PCM at `rate` Hz.
    pub fn render(&self, rate: u32) -> Pcm {
        let rate_f = rate as f32;
        let hold = self.duration.max(0.0);
        let total = ((hold + self.adsr.release.max(0.0)) * rate_f).ceil() as usize;
        let mut samples = Vec::with_capacity(total);
        let mut phase = 0.0f32;
        let mut rng = 0x9E37_79B9u32;
        for i in 0..total {
            let t = i as f32 / rate_f;
            let f = match self.sweep_to {
                Some(end) if hold > 0.0 => self.frequency + (end - self.frequency) * (t / hold).min(1.0),
                _ => self.frequency,
            };
            let dt = (f / rate_f).clamp(0.0, 0.5);
            let raw = match self.waveform {
                Waveform::Sine => (phase * TAU).sin(),
                Waveform::Square => {
                    let naive = if phase < 0.5 { 1.0 } else { -1.0 };
                    naive + poly_blep(phase, dt) - poly_blep((phase + 0.5).fract(), dt)
                }
                Waveform::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
                Waveform::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
                Waveform::Noise => {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    (rng as f32 / u32::MAX as f32) * 2.0 - 1.0
                }
            };
            phase = (phase + dt).fract();
            samples.push(raw * self.volume * self.adsr.level(t, hold));
        }
        Pcm { samples, channels: 1, rate }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
    }

    #[test]
    fn envelope_stages() {
        let e = Adsr { attack: 0.1, decay: 0.1, sustain: 0.5, release: 0.2 };
        let cases = [(0.0, 0.0), (0.05, 0.5), (0.1, 1.0), (0.2, 0.5), (0.5, 0.5), (1.1, 0.25), (1.3, 0.0)];
        for (t, want) in cases {
            let got = e.level(t, 1.0);
            assert!((got - want).abs() < 1e-5, "t={t}: {got} want {want}");
        }
    }

    #[test]
    fn frequency_is_correct_for_each_pitched_waveform() {
        for w in [Waveform::Sine, Waveform::Square, Waveform::Saw, Waveform::Triangle] {
            let tone = Tone { waveform: w, frequency: 500.0, duration: 1.0, adsr: Adsr { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.0 }, volume: 1.0, ..Default::default() };
            let pcm = tone.render(48_000);
            assert_eq!(pcm.samples.len(), 48_000);
            let zc = crossings(&pcm.samples);
            // Every pitched shape crosses zero twice per cycle (a saw once on its ramp, once at its jump).
            assert!(zc.abs_diff(500 * 2) <= 4, "{w:?}: {zc}");
        }
    }

    #[test]
    fn noise_is_repeatable_and_bounded() {
        let tone = Tone { waveform: Waveform::Noise, volume: 1.0, ..Default::default() };
        let a = tone.render(22_050);
        let b = tone.render(22_050);
        assert_eq!(a, b);
        assert!(a.samples.iter().all(|s| s.abs() <= 1.0));
        assert!(crossings(&a.samples) > a.samples.len() / 4);
    }

    #[test]
    fn length_includes_release_and_ends_silent() {
        let tone = Tone { duration: 0.1, adsr: Adsr { release: 0.05, ..Default::default() }, ..Default::default() };
        let pcm = tone.render(8000);
        assert_eq!(pcm.samples.len(), 1200);
        assert!(pcm.samples.last().copied().unwrap_or(1.0).abs() < 0.01);
    }

    #[test]
    fn sweep_raises_pitch() {
        let up = Tone { sweep_to: Some(2000.0), frequency: 200.0, duration: 0.5, volume: 1.0, adsr: Adsr { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.0 }, ..Default::default() }.render(48_000);
        let half = up.samples.len() / 2;
        assert!(crossings(&up.samples[half..]) > crossings(&up.samples[..half]) * 2);
    }
}
