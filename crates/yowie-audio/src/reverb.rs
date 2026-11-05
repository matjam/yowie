//! A Freeverb-style reverb (Jezar at Dreampoint, public domain) and reverb zones.
//!
//! Eight parallel feedback combs with damping feed four series allpasses per channel,
//! with the right channel's delays offset for stereo width.

use crate::dsp::{clamp01, flush_tiny};
use yowie_core::Vec3;

/// Settings of a reverb. Every field runs from 0 to 1; values outside are clamped.
///
/// A zero `room_size`, `damping` or `width` takes the default (0.5, 0.5 and 1). A zero
/// `wet` turns the reverb off, so [`ReverbSettings::default`] is no reverb.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReverbSettings {
    /// How long the tail rings, 0 to 1.
    pub room_size: f32,
    /// How quickly high frequencies die away, 0 to 1.
    pub damping: f32,
    /// Stereo spread, 0 to 1.
    pub width: f32,
    /// Level of the reverb in the mix, 0 to 1.
    pub wet: f32,
}

impl ReverbSettings {
    /// Fills in the zero fields so settings compare and blend on what the reverb uses.
    pub fn with_defaults(mut self) -> Self {
        if self.room_size == 0.0 {
            self.room_size = 0.5;
        }
        if self.damping == 0.0 {
            self.damping = 0.5;
        }
        if self.width == 0.0 {
            self.width = 1.0;
        }
        self.wet = self.wet.max(0.0);
        self
    }

    fn lerp(self, o: Self, t: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self {
            room_size: l(self.room_size, o.room_size),
            damping: l(self.damping, o.damping),
            width: l(self.width, o.width),
            wet: l(self.wet, o.wet),
        }
    }
}

/// A sphere of the world with its own reverb, such as a cave or a hall.
///
/// While the listener is inside, the zone's settings replace the mixer's shared reverb,
/// blended in over `fade` units from the edge. Where zones overlap, the one the listener
/// is furthest inside wins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbZone {
    /// Sphere centre in world units.
    pub center: Vec3,
    /// Sphere radius in world units; a zone with a nonpositive radius is ignored.
    pub radius: f32,
    /// Inward blend distance in world units; nonpositive means the whole radius.
    pub fade: f32,
    /// Settings at full zone strength.
    pub settings: ReverbSettings,
}

/// The shared reverb settings in force at `listener`: `base`, or the best zone blended over it.
pub(crate) fn effective(base: ReverbSettings, zones: &[ReverbZone], listener: Vec3) -> ReverbSettings {
    let b = base.with_defaults();
    let mut best: Option<(&ReverbZone, f32)> = None;
    for z in zones {
        if z.radius <= 0.0 {
            continue;
        }
        let dist = listener.distance(z.center);
        if dist >= z.radius {
            continue;
        }
        let fade = if z.fade <= 0.0 || z.fade > z.radius { z.radius } else { z.fade };
        let w = ((z.radius - dist) / fade).min(1.0);
        if best.is_none_or(|(_, bw)| w > bw) {
            best = Some((z, w));
        }
    }
    let Some((zone, weight)) = best else { return b };
    let mut s = zone.settings.with_defaults();
    if base.wet <= 0.0 {
        // No room outside the zone: keep the zone's character and bring only its level up.
        s.wet *= weight;
        return s;
    }
    b.lerp(s, weight)
}

#[derive(Default)]
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    filt: f32,
    feedback: f32,
    damp: f32,
}

impl Comb {
    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let out = self.buf[self.idx];
        self.filt = flush_tiny(out * (1.0 - self.damp) + self.filt * self.damp);
        self.buf[self.idx] = flush_tiny(input + self.filt * self.feedback);
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

#[derive(Default)]
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let b = self.buf[self.idx];
        self.buf[self.idx] = flush_tiny(input + b * 0.5);
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        b - input
    }
}

/// The reverb engine. Allocated on the game thread; processed on the audio thread.
pub(crate) struct Reverb {
    comb_l: [Comb; 8],
    comb_r: [Comb; 8],
    ap_l: [Allpass; 4],
    ap_r: [Allpass; 4],
    wet1: f32,
    wet2: f32,
}

impl Reverb {
    pub(crate) fn new(rate: u32) -> Self {
        const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
        const SPREAD: usize = 23;
        let scale = f64::from(rate) / 44100.0;
        let len = |n: usize| ((n as f64 * scale) as usize).max(1);
        let mut r = Self {
            comb_l: Default::default(),
            comb_r: Default::default(),
            ap_l: Default::default(),
            ap_r: Default::default(),
            wet1: 0.0,
            wet2: 0.0,
        };
        for (i, n) in COMBS.iter().enumerate() {
            r.comb_l[i].buf = vec![0.0; len(*n)];
            r.comb_r[i].buf = vec![0.0; len(n + SPREAD)];
        }
        for (i, n) in ALLPASSES.iter().enumerate() {
            r.ap_l[i].buf = vec![0.0; len(*n)];
            r.ap_r[i].buf = vec![0.0; len(n + SPREAD)];
        }
        r
    }

    /// Applies settings. Feedback reaches 0.98 at a room size of 1; larger values would
    /// never decay, so sizes are clamped here.
    pub(crate) fn set(&mut self, s: ReverbSettings) {
        let s = s.with_defaults();
        let feedback = clamp01(s.room_size) * 0.28 + 0.7;
        let damp = clamp01(s.damping) * 0.4;
        let width = clamp01(s.width);
        for c in self.comb_l.iter_mut().chain(self.comb_r.iter_mut()) {
            c.feedback = feedback;
            c.damp = damp;
        }
        let wet = s.wet.max(0.0);
        self.wet1 = wet * 3.0 * (width / 2.0 + 0.5);
        self.wet2 = wet * 3.0 * ((1.0 - width) / 2.0);
    }

    /// Runs the interleaved stereo send buffer through the reverb and adds the tail to `out`.
    pub(crate) fn process(&mut self, send: &[f32], out: &mut [f32]) {
        const FIXED_GAIN: f32 = 0.015;
        for (s, o) in send.chunks_exact(2).zip(out.chunks_exact_mut(2)) {
            let input = (s[0] + s[1]) * FIXED_GAIN;
            let (mut l, mut r) = (0.0, 0.0);
            for c in 0..8 {
                l += self.comb_l[c].process(input);
                r += self.comb_r[c].process(input);
            }
            for a in 0..4 {
                l = self.ap_l[a].process(l);
                r = self.ap_r[a].process(r);
            }
            o[0] += l * self.wet1 + r * self.wet2;
            o[1] += r * self.wet1 + l * self.wet2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(buf: &[f32]) -> f32 {
        buf.iter().map(|v| v * v).sum()
    }

    #[test]
    fn tail_decays() {
        let rate = 44_100;
        let mut r = Reverb::new(rate);
        r.set(ReverbSettings { room_size: 0.5, wet: 0.5, ..Default::default() });
        let frames = rate as usize / 2;
        let mut send = vec![0.0; frames * 2];
        send[0] = 1.0;
        send[1] = 1.0;
        let mut out = vec![0.0; frames * 2];
        r.process(&send, &mut out);
        let early = energy(&out[..frames / 2]);
        let late = energy(&out[frames + frames / 2..]);
        assert!(early > 0.0);
        assert!(late < early * 0.5, "early {early} late {late}");
        let mut quiet = vec![0.0; frames * 2];
        for _ in 0..20 {
            quiet.fill(0.0);
            r.process(&vec![0.0; frames * 2], &mut quiet);
        }
        assert!(energy(&quiet) < 1e-6);
    }

    #[test]
    fn bigger_rooms_ring_longer() {
        let tail = |room| {
            let mut r = Reverb::new(44_100);
            r.set(ReverbSettings { room_size: room, wet: 1.0, ..Default::default() });
            let mut send = vec![0.0; 88_200];
            send[0] = 1.0;
            let mut out = vec![0.0; 88_200];
            r.process(&send, &mut out);
            energy(&out[44_100..])
        };
        assert!(tail(0.9) > tail(0.2));
    }

    #[test]
    fn zones_blend_by_depth() {
        let zone = ReverbZone {
            center: Vec3::ZERO,
            radius: 10.0,
            fade: 5.0,
            settings: ReverbSettings { room_size: 0.9, wet: 0.8, ..Default::default() },
        };
        let base = ReverbSettings::default();
        let cases = [(Vec3::new(20.0, 0.0, 0.0), 0.0), (Vec3::new(7.5, 0.0, 0.0), 0.4), (Vec3::ZERO, 0.8)];
        for (pos, wet) in cases {
            let s = effective(base, &[zone], pos);
            assert!((s.wet - wet).abs() < 1e-5, "{pos:?}: {}", s.wet);
        }
    }
}
