//! Mixing the voices of one tick span into stereo.

use super::envelope::auto_vibrato_delta;
use super::module::{LoopType, Module};
use super::player::State;
use super::state::{Voice, clamp_pan};
use super::tables::{LINEAR_PER_SEMI, frequency, it_cutoff_hz};

/// Channel-level values a voice reads while rendering.
pub(crate) struct Extra {
    pub chan_vol: i32,
    /// MOD: a sample change that takes effect when the loop wraps.
    pub pending: Option<usize>,
}

/// The pitch, volume and pan a voice is rendered with this span.
pub(crate) struct Params {
    pub period: f64,
    pub volume: i32,
    pub pan: f32,
    pub global: f32,
}

/// Four-point Hermite interpolation between `y1` and `y2` at `t`, using `y0` and `y3` for slope.
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// The loop in force for a voice: start, exclusive end, kind.
fn active_loop(m: &Module, v: &Voice, sample: usize) -> (usize, usize, LoopType) {
    let Some(s) = m.samples.get(sample) else {
        return (0, 0, LoopType::None);
    };
    if v.sustain_on
        && !v.key_off
        && s.sus_loop != LoopType::None
        && s.sus_loop_end > s.sus_loop_start + 1
    {
        (s.sus_loop_start, s.sus_loop_end, s.sus_loop)
    } else {
        (s.loop_start, s.loop_end, s.loop_type)
    }
}

impl State {
    /// Mixes every voice into `out` for one span of a tick.
    pub(crate) fn render(&mut self, m: &Module, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let global = self.global_vol as f32 / 128.0;
        for i in 0..self.chans.len() {
            let mut ch = std::mem::take(&mut self.chans[i]);
            let mut ex = Extra {
                chan_vol: ch.chan_vol,
                pending: ch.pending_sample,
            };
            let p = Params {
                period: ch.out_period,
                volume: ch.out_volume,
                pan: ch.out_pan,
                global,
            };
            self.render_voice(m, out, &mut ch.v, Some(&mut ex), &p);
            ch.pending_sample = ex.pending;
            if ch.v.filter_on {
                ch.v.filter.flush();
            }
            self.chans[i] = ch;
        }
        for i in 0..self.bg.len() {
            let mut v = self.bg[i];
            let p = Params {
                period: v.period,
                volume: v.volume,
                pan: v.pan,
                global,
            };
            self.render_voice(m, out, &mut v, None, &p);
            if v.filter_on {
                v.filter.flush();
            }
            self.bg[i] = v;
        }
        if self.amiga_filter && self.opts.amiga_filter {
            for f in 0..frames {
                out[f * 2] = self.filter_l.process(out[f * 2]);
                out[f * 2 + 1] = self.filter_r.process(out[f * 2 + 1]);
            }
            self.filter_l.flush();
            self.filter_r.flush();
        }
    }

    fn render_voice(
        &self,
        m: &Module,
        out: &mut [f32],
        v: &mut Voice,
        mut ex: Option<&mut Extra>,
        p: &Params,
    ) {
        let frames = out.len() / 2;
        let Some(si) = v.sample.filter(|_| v.active && p.period > 0.0) else {
            return;
        };
        let Some(s) = m.samples.get(si) else { return };
        let inst = v.inst.and_then(|i| m.instruments.get(i));
        let mut period = p.period;
        let mut pan = p.pan;
        // Pitch: the envelope (in semitones) and auto-vibrato sit on top of the channel's period.
        if inst.is_some_and(|i| i.pitch_env.enabled && !i.pitch_is_filter) {
            let semis = f64::from(v.pitch_env.value) / 8.0;
            if m.linear_slides {
                period -= semis * LINEAR_PER_SEMI;
            } else {
                period *= 2f64.powf(-semis / 12.0);
            }
        }
        period += auto_vibrato_delta(m, v);
        let step = frequency(m.format, m.linear_slides, period) / f64::from(self.rate);
        if step <= 0.0 || step > 64.0 {
            return;
        }
        let mut vol =
            p.volume as f32 / 64.0 * p.global * self.gain * (s.global_volume as f32 / 64.0);
        if let Some(i) = inst {
            vol *= i.global_volume as f32 / 128.0;
            if i.vol_env.enabled {
                vol *= v.vol_env.value / 64.0;
            }
            vol *= v.fade as f32 / 65536.0;
            if i.pan_env.enabled {
                pan = clamp_pan(pan + v.pan_env.value / 32.0 * (1.0 - pan.abs()));
            }
        }
        if let Some(e) = &ex {
            vol *= e.chan_vol as f32 / 64.0;
        }
        if vol <= 0.0 {
            return;
        }
        if !self.audible(v.index) {
            // Muted: the sample advances as if heard, so unmuting is seamless.
            advance_silent(m, v, si, step, frames);
            return;
        }
        let pan = clamp_pan(pan);
        // Linear panning, as the trackers themselves mix: centre is unity on both sides,
        // hard left is double on one side and silent on the other.
        let (left, right) = (vol * (1.0 - pan), vol * (1.0 + pan));
        let use_filter = v.filter_on && v.cutoff < 127;
        if use_filter {
            let mut cutoff = v.cutoff;
            if inst.is_some_and(|i| i.pitch_is_filter && i.pitch_env.enabled) {
                cutoff = (cutoff as f32 * (v.pitch_env.value + 32.0) / 64.0) as i32;
            }
            let q = 0.707 + f64::from(v.resonance) / 128.0 * 6.0;
            v.filter
                .set_cutoff(it_cutoff_hz(cutoff.clamp(0, 127)), q, self.rate);
        }
        let data = &s.data;
        let n = data.len();
        let (loop_start, loop_end, loop_type) = active_loop(m, v, si);
        let looping = loop_type != LoopType::None && loop_end.min(n) > loop_start + 1;
        let end = if looping { loop_end.min(n) } else { n };
        let (mut pos, mut dir) = (v.pos, v.dir);
        for f in 0..frames {
            if dir > 0 && pos >= end as f64 {
                if !looping {
                    v.active = false;
                    break;
                }
                if let Some(pending) = ex.as_deref_mut().and_then(|e| e.pending.take()) {
                    // ProTracker: an instrument change without a note takes effect when the loop wraps.
                    v.sample = Some(pending);
                    v.pos = m.samples.get(pending).map_or(0.0, |s| s.loop_start as f64);
                    v.dir = 1;
                    return;
                }
                if loop_type == LoopType::PingPong {
                    dir = -1;
                    pos = end as f64 - (pos - end as f64) - 1.0;
                } else {
                    let span = (end - loop_start) as f64;
                    while pos >= end as f64 {
                        pos -= span;
                    }
                }
            } else if dir < 0 && pos < loop_start as f64 {
                dir = 1;
                pos = loop_start as f64 + (loop_start as f64 - pos);
            }
            let idx = pos.max(0.0) as usize;
            if idx >= n {
                v.active = false;
                break;
            }
            let frac = (pos - idx as f64) as f32;
            let next = idx as i64 + i64::from(dir);
            let next = if next >= end as i64 || next < 0 {
                idx
            } else {
                next as usize
            };
            let mut sample = if self.opts.cubic {
                // Neighbours on either side, clamped at the sample's edges.
                let prev = idx as i64 - i64::from(dir);
                let prev = if prev < 0 || prev >= end as i64 {
                    idx
                } else {
                    prev as usize
                };
                let after = next as i64 + i64::from(dir);
                let after = if after < 0 || after >= end as i64 {
                    next
                } else {
                    after as usize
                };
                hermite(data[prev], data[idx], data[next], data[after], frac)
            } else {
                data[idx] * (1.0 - frac) + data[next] * frac
            };
            if use_filter {
                sample = v.filter.process(sample);
            }
            out[f * 2] += sample * left;
            out[f * 2 + 1] += sample * right;
            pos += step * f64::from(dir);
        }
        v.pos = pos;
        v.dir = dir;
    }
}

/// Moves a muted voice through its sample without mixing it, ending it where a one-shot
/// runs out and wrapping a loop the way [`State::render_voice`] does.
fn advance_silent(m: &Module, v: &mut Voice, si: usize, step: f64, frames: usize) {
    let n = m.samples.get(si).map_or(0, |s| s.data.len());
    let (loop_start, loop_end, loop_type) = active_loop(m, v, si);
    let looping = loop_type != LoopType::None && loop_end.min(n) > loop_start + 1;
    let end = if looping { loop_end.min(n) } else { n };
    let mut pos = v.pos + step * f64::from(v.dir) * frames as f64;
    if !looping {
        if pos >= end as f64 {
            v.active = false;
            return;
        }
    } else if loop_type == LoopType::PingPong {
        let span = (end.saturating_sub(loop_start)).max(1) as f64;
        // Fold the position into a forward pass and a backward pass.
        let off = (pos - loop_start as f64).rem_euclid(2.0 * span);
        let off = if off >= span {
            v.dir = -1;
            2.0 * span - off - 1.0
        } else {
            v.dir = 1;
            off
        };
        pos = loop_start as f64 + off;
    } else {
        let span = (end.saturating_sub(loop_start)).max(1) as f64;
        while pos >= end as f64 {
            pos -= span;
        }
    }
    v.pos = pos;
}
