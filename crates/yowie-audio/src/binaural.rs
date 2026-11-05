//! The binaural spatialiser.
//!
//! A positional voice is rendered for each ear from a parametric head model rather
//! than a measured head-related transfer function. The ears hear the same signal at
//! different times (the interaural time difference, from Woodworth's formula) and at
//! different levels. The far ear is dulled by a one-pole low-pass whose cutoff falls
//! with the angle, and a shelf lifts or drops the high band as the source rises or
//! falls, which is the model's only elevation cue.
//!
//! Every parameter is worked out per chunk and interpolated across it, so a moving
//! source glides.

use crate::dsp::{flush_slice, flush_tiny, one_pole};
use crate::spatial::Listener;
use yowie_core::Vec3;

pub(crate) const DEFAULT_HEAD_RADIUS: f32 = 0.0875;
const MAX_HEAD_RADIUS: f32 = 0.3;
const AIR_SPEED: f32 = 343.0;
const SHADOW_OPEN_HZ: f32 = 22_000.0;
const SHADOW_CLOSED_RATIO: f32 = 0.07;
const SHELF_HZ: f32 = 4000.0;
const ELEVATION_DEPTH: f32 = 0.6;
const SHADOW_LEVEL_DB: f32 = 6.0;

/// The head model for one chunk: what the mixer interpolates toward.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EarParams {
    pub delay_l: f32,
    pub delay_r: f32,
    pub shadow_l: f32,
    pub shadow_r: f32,
    pub tilt: f32,
    pub gain_l: f32,
    pub gain_r: f32,
}

/// Per-voice head-model state: the delay line both ears tap.
pub(crate) struct Binaural {
    ring: Vec<f32>,
    w: usize,
    lp_l: f32,
    lp_r: f32,
    low: f32,
    shelf: f32,
    cur: EarParams,
    pub(crate) started: bool,
}

/// Head radius in metres, clamped to what the delay line has room for.
pub(crate) fn head_radius(r: f32) -> f32 {
    if r <= 0.0 {
        DEFAULT_HEAD_RADIUS
    } else {
        r.min(MAX_HEAD_RADIUS)
    }
}

/// Works out the head model for a source at `p`.
///
/// The lateral angle is measured from the plane through both ears, so a source overhead
/// reaches both ears together; elevation is measured against the listener's up.
pub(crate) fn head_model(l: &Listener, p: Vec3, radius: f32, rate: u32) -> EarParams {
    let fwd = l.forward.normalize_or_zero();
    let right = fwd.cross(l.up).normalize_or_zero();
    let up = right.cross(fwd);
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let d = p - l.position;
    let len = d.length();
    if len > 1e-6 {
        let dir = d / len;
        x = dir.dot(right).clamp(-1.0, 1.0);
        y = dir.dot(up).clamp(-1.0, 1.0);
    }
    // Woodworth: the extra path around a sphere, about 0.66 ms at the ear for an average head.
    let theta = f64::from(x).asin();
    let itd =
        (f64::from(radius) / f64::from(AIR_SPEED) * (theta + theta.sin()) * f64::from(rate)) as f32;
    let far = 10f32.powf(-SHADOW_LEVEL_DB / 20.0 * x.abs());
    let centre = std::f32::consts::FRAC_1_SQRT_2;
    let (mut gain_l, mut gain_r) = (centre, centre);
    if x >= 0.0 {
        gain_l *= far;
    } else {
        gain_r *= far;
    }
    EarParams {
        delay_l: itd.max(0.0),
        delay_r: (-itd).max(0.0),
        shadow_l: one_pole(SHADOW_OPEN_HZ * SHADOW_CLOSED_RATIO.powf(x.max(0.0)), rate),
        shadow_r: one_pole(
            SHADOW_OPEN_HZ * SHADOW_CLOSED_RATIO.powf((-x).max(0.0)),
            rate,
        ),
        tilt: 1.0 + ELEVATION_DEPTH * y,
        gain_l,
        gain_r,
    }
}

/// Reads the delay line `d` frames back from `w`, interpolating between neighbours.
#[inline]
fn tap(ring: &[f32], w: usize, d: f32) -> f32 {
    let n = ring.len() as isize;
    let p = w as f32 - d;
    let mut i = p.floor() as isize;
    let frac = p - i as f32;
    i = i.rem_euclid(n);
    let j = if i + 1 == n { 0 } else { i + 1 };
    ring[i as usize] * (1.0 - frac) + ring[j as usize] * frac
}

impl Binaural {
    pub(crate) fn new(rate: u32) -> Self {
        let frames = (MAX_HEAD_RADIUS / AIR_SPEED
            * (std::f32::consts::FRAC_PI_2 + 1.0)
            * rate as f32) as usize
            + 4;
        Self {
            ring: vec![0.0; frames],
            w: 0,
            lp_l: 0.0,
            lp_r: 0.0,
            low: 0.0,
            shelf: one_pole(SHELF_HZ, rate),
            cur: EarParams::default(),
            started: false,
        }
    }

    /// Holds the current model for a voice that is ramping out.
    pub(crate) fn current(&self) -> Option<EarParams> {
        self.started.then_some(self.cur)
    }

    /// Mixes a mono chunk into `out` through the head model, ramping the gains from
    /// `from` to `to` and gliding each head parameter from where the last chunk ended.
    /// When `send_rev` is above zero, the signal is also added to `send`.
    pub(crate) fn render(
        &mut self,
        mono: &[f32],
        target: EarParams,
        ramp: [(f32, f32); 2],
        out: &mut [f32],
        send: &mut [f32],
        send_rev: f32,
    ) {
        let n = mono.len();
        if n == 0 {
            return;
        }
        if !self.started {
            self.ring.fill(0.0);
            self.cur = target;
            self.started = true;
        }
        let t = target;
        let mut p = self.cur;
        let inv = 1.0 / n as f32;
        let dd_l = (t.delay_l - p.delay_l) * inv;
        let dd_r = (t.delay_r - p.delay_r) * inv;
        let ds_l = (t.shadow_l - p.shadow_l) * inv;
        let ds_r = (t.shadow_r - p.shadow_r) * inv;
        let dt = (t.tilt - p.tilt) * inv;
        let [from, to] = ramp;
        let (mut gl, mut gr) = from;
        let (dl, dr) = ((to.0 - gl) * inv, (to.1 - gr) * inv);
        let (mut w, shelf) = (self.w, self.shelf);
        let (mut low, mut lp_l, mut lp_r) = (self.low, self.lp_l, self.lp_r);
        let len = self.ring.len();
        for (i, &x) in mono.iter().enumerate() {
            p.delay_l += dd_l;
            p.delay_r += dd_r;
            p.shadow_l += ds_l;
            p.shadow_r += ds_r;
            p.tilt += dt;
            gl += dl;
            gr += dr;
            low += shelf * (x - low);
            self.ring[w] = low + p.tilt * (x - low);
            let l = tap(&self.ring, w, p.delay_l);
            let r = tap(&self.ring, w, p.delay_r);
            w += 1;
            if w == len {
                w = 0;
            }
            lp_l += p.shadow_l * (l - lp_l);
            lp_r += p.shadow_r * (r - lp_r);
            let (sl, sr) = (lp_l * gl, lp_r * gr);
            out[i * 2] += sl;
            out[i * 2 + 1] += sr;
            if send_rev > 0.0 {
                send[i * 2] += sl * send_rev;
                send[i * 2 + 1] += sr * send_rev;
            }
        }
        // Once the shelf's low band has died away the input has been silent for longer
        // than the delay line, so what the line holds is the same decay: flush it too.
        low = flush_tiny(low);
        if low == 0.0 {
            flush_slice(&mut self.ring);
        }
        self.w = w;
        self.low = low;
        self.lp_l = flush_tiny(lp_l);
        self.lp_r = flush_tiny(lp_r);
        self.cur = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    #[test]
    fn delay_sign_follows_side() {
        let l = Listener::default();
        let right = head_model(&l, Vec3::new(5.0, 0.0, 0.0), DEFAULT_HEAD_RADIUS, RATE);
        assert!(right.delay_l > 0.0 && right.delay_r == 0.0, "{right:?}");
        let left = head_model(&l, Vec3::new(-5.0, 0.0, 0.0), DEFAULT_HEAD_RADIUS, RATE);
        assert!(left.delay_r > 0.0 && left.delay_l == 0.0);
        let front = head_model(&l, Vec3::new(0.0, 0.0, -5.0), DEFAULT_HEAD_RADIUS, RATE);
        assert!(front.delay_l == 0.0 && front.delay_r == 0.0);
    }

    #[test]
    fn side_delay_is_about_two_thirds_millisecond() {
        let l = Listener::default();
        let e = head_model(&l, Vec3::new(5.0, 0.0, 0.0), DEFAULT_HEAD_RADIUS, RATE);
        let ms = e.delay_l / RATE as f32 * 1000.0;
        assert!((ms - 0.66).abs() < 0.08, "{ms}");
    }

    #[test]
    fn far_ear_is_quieter_and_duller() {
        let l = Listener::default();
        let e = head_model(&l, Vec3::new(5.0, 0.0, 0.0), DEFAULT_HEAD_RADIUS, RATE);
        assert!(e.gain_l < e.gain_r);
        assert!(e.shadow_l < e.shadow_r);
    }

    #[test]
    fn rendered_impulse_arrives_later_in_far_ear() {
        let l = Listener::default();
        let target = head_model(&l, Vec3::new(5.0, 0.0, 0.0), DEFAULT_HEAD_RADIUS, RATE);
        let mut b = Binaural::new(RATE);
        let mut mono = vec![0.0; 256];
        mono[10] = 1.0;
        let mut out = vec![0.0; 512];
        let mut send = vec![0.0; 512];
        b.render(&mono, target, [(1.0, 1.0); 2], &mut out, &mut send, 0.0);
        let peak = |ch: usize| {
            (0..256)
                .max_by(|a, c| out[a * 2 + ch].abs().total_cmp(&out[c * 2 + ch].abs()))
                .unwrap_or(0)
        };
        assert!(peak(0) > peak(1), "left {} right {}", peak(0), peak(1));
    }
}
