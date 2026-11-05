//! The player's mutable state: one voice per channel plus background voices.

use super::module::{Cell, Nna};
use crate::dsp::flush_tiny;

/// A two-pole resonant low-pass filter (RBJ biquad) for Impulse Tracker's filter effects.
#[derive(Clone, Copy, Default)]
pub(crate) struct Filter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Filter {
    pub(crate) fn set_cutoff(&mut self, freq: f64, q: f64, rate: u32) {
        let freq = freq.min(f64::from(rate) * 0.45);
        let w = 2.0 * std::f64::consts::PI * freq / f64::from(rate);
        let alpha = w.sin() / (2.0 * q);
        let cosw = w.cos();
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cosw) / 2.0 / a0) as f32;
        self.b1 = ((1.0 - cosw) / a0) as f32;
        self.b2 = self.b0;
        self.a1 = (-2.0 * cosw / a0) as f32;
        self.a2 = ((1.0 - alpha) / a0) as f32;
    }

    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    /// Zeroes state below 1e-15 so a filter running on silence never reaches subnormals.
    pub(crate) fn flush(&mut self) {
        self.x1 = flush_tiny(self.x1);
        self.x2 = flush_tiny(self.x2);
        self.y1 = flush_tiny(self.y1);
        self.y2 = flush_tiny(self.y2);
    }
}

/// Position of one envelope.
#[derive(Clone, Copy, Default)]
pub(crate) struct EnvState {
    pub tick: usize,
    pub value: f32,
    pub done: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct AutoVibState {
    pub pos: i32,
    pub sweep: usize,
}

/// One sounding sample with its own envelopes and fade.
#[derive(Clone, Copy)]
pub(crate) struct Voice {
    pub sample: Option<usize>,
    pub inst: Option<usize>,
    pub pos: f64,
    /// +1, or -1 while a ping-pong loop runs backwards.
    pub dir: i32,
    pub active: bool,
    /// The channel it started on, for mute and solo.
    pub index: usize,
    /// Base pitch after slides.
    pub period: f64,
    pub note: i32,
    pub volume: i32,
    pub pan: f32,
    pub key_off: bool,
    pub fading: bool,
    /// 65536 is full.
    pub fade: i32,
    pub vol_env: EnvState,
    pub pan_env: EnvState,
    pub pitch_env: EnvState,
    pub auto_vib: AutoVibState,
    /// IT filter cutoff 0..=127; 127 means off.
    pub cutoff: i32,
    pub resonance: i32,
    pub filter: Filter,
    pub filter_on: bool,
    /// The IT sustain loop is still engaged.
    pub sustain_on: bool,
    /// IT S73 to S76: replaces the new-note action for the next note on this channel.
    pub nna_override: Option<Nna>,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            sample: None,
            inst: None,
            pos: 0.0,
            dir: 1,
            active: false,
            index: 0,
            period: 0.0,
            note: 0,
            volume: 0,
            pan: 0.0,
            key_off: false,
            fading: false,
            fade: 65536,
            vol_env: EnvState::default(),
            pan_env: EnvState::default(),
            pitch_env: EnvState::default(),
            auto_vib: AutoVibState::default(),
            cutoff: 127,
            resonance: 0,
            filter: Filter::default(),
            filter_on: false,
            sustain_on: false,
            nna_override: None,
        }
    }
}

/// A pattern channel: its voice plus the effect memory a channel carries between rows.
#[derive(Clone, Copy)]
pub(crate) struct Channel {
    pub v: Voice,
    /// The channel's default pan.
    pub pan: f32,
    /// IT channel volume 0..=64.
    pub chan_vol: i32,
    pub last_inst: usize,
    pub last_note: i32,
    pub out_period: f64,
    pub out_volume: i32,
    pub out_pan: f32,
    pub porta_target: f64,
    pub porta_speed: i32,
    pub vib_pos: i32,
    pub vib_speed: i32,
    pub vib_depth: i32,
    pub vib_wave: usize,
    pub trem_pos: i32,
    pub trem_speed: i32,
    pub trem_depth: i32,
    pub trem_wave: usize,
    pub panbr_pos: i32,
    pub panbr_speed: i32,
    pub panbr_depth: i32,
    pub arp_param: u8,
    pub vol_slide: u8,
    pub pan_slide: u8,
    pub chan_vol_slide: u8,
    pub global_slide: u8,
    pub offset: u8,
    pub offset_high: i32,
    pub retrig: u8,
    pub retrig_count: i32,
    pub tremor_param: u8,
    pub tremor_count: i32,
    pub tremor_off: bool,
    pub note_cut: i32,
    pub note_delay: i32,
    pub delayed: Cell,
    pub key_off_tick: i32,
    /// Shared effect memory (S3M, IT).
    pub mem: u8,
    pub porta_mem: u8,
    pub vib_mem: u8,
    /// MOD: an instrument change waiting for the loop to wrap.
    pub pending_sample: Option<usize>,
    pub finetune: f32,
    pub glissando: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            v: Voice::default(),
            pan: 0.0,
            chan_vol: 64,
            last_inst: 0,
            last_note: 0,
            out_period: 0.0,
            out_volume: 0,
            out_pan: 0.0,
            porta_target: 0.0,
            porta_speed: 0,
            vib_pos: 0,
            vib_speed: 0,
            vib_depth: 0,
            vib_wave: 0,
            trem_pos: 0,
            trem_speed: 0,
            trem_depth: 0,
            trem_wave: 0,
            panbr_pos: 0,
            panbr_speed: 0,
            panbr_depth: 0,
            arp_param: 0,
            vol_slide: 0,
            pan_slide: 0,
            chan_vol_slide: 0,
            global_slide: 0,
            offset: 0,
            offset_high: 0,
            retrig: 0,
            retrig_count: 0,
            tremor_param: 0,
            tremor_count: 0,
            tremor_off: false,
            note_cut: -1,
            note_delay: -1,
            delayed: Cell::default(),
            key_off_tick: -1,
            mem: 0,
            porta_mem: 0,
            vib_mem: 0,
            pending_sample: None,
            finetune: 0.0,
            glissando: false,
        }
    }
}

pub(crate) fn clamp_pan(v: f32) -> f32 {
    v.clamp(-1.0, 1.0)
}
