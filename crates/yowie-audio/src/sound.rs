//! Decoded audio held in memory.

use crate::error::{Error, Result};
use std::sync::Arc;

/// Interleaved floating-point audio, nominally in `-1.0..=1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    /// Interleaved samples, `channels` per frame.
    pub samples: Vec<f32>,
    /// Samples per frame. [`Sound::new`] accepts 1 or 2.
    pub channels: u16,
    /// Frames per second; must be positive.
    pub rate: u32,
}

impl Pcm {
    /// Number of frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// Length in seconds.
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }

    /// A mono sine tone with 10 ms fades at both ends, for tests and placeholders.
    pub fn sine(freq: f64, seconds: f64, rate: u32) -> Self {
        let n = (seconds * f64::from(rate)) as usize;
        let fade = (rate / 100) as usize;
        let samples = (0..n)
            .map(|i| {
                let env = if i < fade {
                    i as f32 / fade as f32
                } else if n - i < fade {
                    (n - i) as f32 / fade as f32
                } else {
                    1.0
                };
                env * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin() as f32
            })
            .collect();
        Self {
            samples,
            channels: 1,
            rate,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SoundData {
    /// Interleaved stereo at the mixer rate.
    pub samples: Vec<f32>,
    pub mono: bool,
}

/// Audio decoded into memory at the mixer's sample rate.
///
/// A `Sound` is cheap to clone; clones share the samples. To play it, call
/// [`Mixer::play`](crate::Mixer::play).
#[derive(Clone, Debug)]
pub struct Sound {
    pub(crate) data: Arc<SoundData>,
    rate: u32,
}

impl Sound {
    /// Converts PCM to the mixer rate and stereo layout.
    ///
    /// `mixer_rate` is [`Mixer::rate`](crate::Mixer::rate). Fails when the channel count
    /// is not 1 or 2, the rate is zero, or the sample count is not a whole number of frames.
    pub fn new(mixer_rate: u32, pcm: &Pcm) -> Result<Self> {
        let ch = usize::from(pcm.channels);
        if !(1..=2).contains(&ch)
            || pcm.rate == 0
            || mixer_rate == 0
            || !pcm.samples.len().is_multiple_of(ch)
        {
            return Err(Error::Unsupported(format!(
                "{} channels at {} Hz, {} samples",
                pcm.channels,
                pcm.rate,
                pcm.samples.len()
            )));
        }
        let mut stereo: Vec<f32> = if ch == 1 {
            pcm.samples.iter().flat_map(|&s| [s, s]).collect()
        } else {
            pcm.samples.clone()
        };
        if pcm.rate != mixer_rate {
            stereo = resample(&stereo, pcm.rate, mixer_rate);
        }
        let mono = ch == 1
            || stereo
                .chunks_exact(2)
                .all(|f| f[0].to_bits() == f[1].to_bits());
        Ok(Self {
            data: Arc::new(SoundData {
                samples: stereo,
                mono,
            }),
            rate: mixer_rate,
        })
    }

    /// Number of frames at the mixer rate.
    pub fn frames(&self) -> usize {
        self.data.samples.len() / 2
    }

    /// Length in seconds.
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate)
    }
}

/// Resamples interleaved stereo by linear interpolation.
pub(crate) fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let frames = input.len() / 2;
    if frames == 0 {
        return Vec::new();
    }
    let out_frames = (frames as f64 * f64::from(to) / f64::from(from)).round() as usize;
    let step = f64::from(from) / f64::from(to);
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let src = i as f64 * step;
        let j = (src as usize).min(frames - 1);
        let k = (j + 1).min(frames - 1);
        let t = (src - j as f64) as f32;
        out.push(input[j * 2] * (1.0 - t) + input[k * 2] * t);
        out.push(input[j * 2 + 1] * (1.0 - t) + input[k * 2 + 1] * t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_length_and_pitch() {
        let pcm = Pcm::sine(440.0, 0.5, 22_050);
        for to in [44_100, 48_000, 11_025] {
            let s = Sound::new(to, &pcm).unwrap();
            let want = (0.5 * f64::from(to)) as usize;
            assert!(s.frames().abs_diff(want) <= 1, "{to}: {}", s.frames());
            let crossings = s
                .data
                .samples
                .chunks_exact(2)
                .map(|f| f[0])
                .collect::<Vec<_>>();
            let zc = crossings
                .windows(2)
                .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                .count();
            // 440 Hz for half a second crosses zero about 440 times.
            assert!(zc.abs_diff(440) < 8, "{to}: {zc}");
        }
    }

    #[test]
    fn rejects_bad_pcm() {
        let cases = [
            Pcm {
                samples: vec![0.0; 6],
                channels: 3,
                rate: 44_100,
            },
            Pcm {
                samples: vec![0.0; 3],
                channels: 2,
                rate: 44_100,
            },
            Pcm {
                samples: vec![0.0; 4],
                channels: 2,
                rate: 0,
            },
        ];
        for pcm in cases {
            assert!(Sound::new(44_100, &pcm).is_err());
        }
    }

    #[test]
    fn mono_detection() {
        let mono = Sound::new(
            8000,
            &Pcm {
                samples: vec![0.1, 0.2],
                channels: 1,
                rate: 8000,
            },
        )
        .unwrap();
        assert!(mono.data.mono);
        let st = Sound::new(
            8000,
            &Pcm {
                samples: vec![0.1, 0.2],
                channels: 2,
                rate: 8000,
            },
        )
        .unwrap();
        assert!(!st.data.mono);
    }
}
