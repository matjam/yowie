//! Envelopes, fades and auto-vibrato, advanced once per tick.

use super::module::{Envelope, Format, Module};
use super::player::State;
use super::state::{EnvState, Voice};
use super::tables::waveform;

impl Envelope {
    /// Evaluates the envelope at `tick`, interpolating between points.
    pub(crate) fn value(&self, tick: usize) -> f32 {
        let pts = &self.points;
        let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
            return 0.0;
        };
        if tick <= first.tick {
            return first.value;
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if tick <= b.tick {
                if b.tick == a.tick {
                    return b.value;
                }
                let t = (tick - a.tick) as f32 / (b.tick - a.tick) as f32;
                return a.value + (b.value - a.value) * t;
            }
        }
        last.value
    }

    /// Moves the envelope one tick, honouring sustain (while the key is held) and loops.
    pub(crate) fn advance(&self, st: &mut EnvState, key_off: bool) {
        if !self.enabled || self.points.is_empty() {
            st.value = 0.0;
            return;
        }
        st.value = self.value(st.tick);
        let pts = &self.points;
        // Loop and sustain points come from the file and may name points that do not exist.
        let at = |i: usize| pts.get(i).map(|p| p.tick);
        if self.sustain
            && !key_off
            && let (Some(end), Some(start)) = (at(self.sustain_end), at(self.sustain_start))
            && st.tick >= end
        {
            st.tick = start;
            if self.sustain_start == self.sustain_end {
                return; // hold on the sustain point
            }
        }
        if self.looping
            && let (Some(end), Some(start)) = (at(self.loop_end), at(self.loop_start))
            && st.tick >= end
        {
            st.tick = start;
            if self.loop_start == self.loop_end {
                return;
            }
        }
        if pts.last().is_some_and(|p| st.tick >= p.tick) {
            st.done = true;
            return;
        }
        st.tick += 1;
    }
}

impl State {
    /// Runs envelopes, fades and auto-vibrato for every voice after the row or tick
    /// effects have set the channel's output values.
    pub(crate) fn update_voices(&mut self, m: &Module) {
        for i in 0..self.chans.len() {
            let mut v = self.chans[i].v;
            update_voice(m, &mut v);
            self.chans[i].v = v;
        }
        let mut i = 0;
        while i < self.bg.len() {
            update_voice(m, &mut self.bg[i]);
            if self.bg[i].active {
                i += 1;
            } else {
                self.bg.swap_remove(i);
            }
        }
    }
}

fn update_voice(m: &Module, v: &mut Voice) {
    if !v.active {
        return;
    }
    if let Some(inst) = v.inst.and_then(|i| m.instruments.get(i)) {
        inst.vol_env.advance(&mut v.vol_env, v.key_off);
        inst.pan_env.advance(&mut v.pan_env, v.key_off);
        inst.pitch_env.advance(&mut v.pitch_env, v.key_off);
        // Fading starts at key off (IT), or once the volume envelope has finished after key
        // off (XM keeps the note until then).
        if v.key_off && (m.format == Format::It || !inst.vol_env.enabled || v.vol_env.done) {
            v.fading = true;
        }
        if v.fading {
            v.fade -= inst.fadeout;
            if v.fade <= 0 {
                v.fade = 0;
                v.active = false;
            }
        }
        if v.key_off && !inst.vol_env.enabled && m.format == Format::Xm {
            v.active = false; // FastTracker cuts a key-off note that has no envelope
        }
        if inst.vol_env.enabled && v.vol_env.done && v.vol_env.value <= 0.0 {
            v.active = false;
        }
    }
    if let Some(s) = v.sample.and_then(|i| m.samples.get(i))
        && s.vibrato.depth > 0
    {
        if v.auto_vib.sweep < s.vibrato.sweep {
            v.auto_vib.sweep += 1;
        }
        v.auto_vib.pos += s.vibrato.rate as i32;
    }
}

/// The instrument vibrato's period offset this tick.
pub(crate) fn auto_vibrato_delta(m: &Module, v: &Voice) -> f64 {
    let Some(s) = v.sample.and_then(|i| m.samples.get(i)) else {
        return 0.0;
    };
    if s.vibrato.depth == 0 {
        return 0.0;
    }
    let mut depth = s.vibrato.depth as f64;
    if s.vibrato.sweep > 0 {
        depth *= v.auto_vib.sweep as f64 / s.vibrato.sweep as f64;
    }
    let w = f64::from(waveform(s.vibrato.kind, v.auto_vib.pos / 4));
    if m.linear_slides {
        w * depth / 256.0
    } else {
        w * depth / 64.0
    }
}
