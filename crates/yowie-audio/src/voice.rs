//! Playback options and the audio thread's state for one voice.

use crate::binaural::Binaural;
use crate::bus::Bus;
use crate::dsp::{Biquad, LowPass};
use crate::sound::{Sound, SoundData};
use crate::spatial::{Attenuation, Cone, occlusion_cutoff};
use crate::stream::StreamState;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use yowie_core::Vec3;

/// How a voice is heard from a position in the world.
///
/// The voice is full volume within `min_distance` and fades to silence at `max_distance`,
/// panned (or binaurally rendered) by direction relative to the listener.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Positional {
    /// Source position in world units. With `relative` set, an offset in the listener's
    /// frame (x right, y up, -z forward).
    pub position: Vec3,
    /// Source velocity in world units per second; drives Doppler.
    pub velocity: Vec3,
    /// Radius of full volume. Defaults to 1.
    pub min_distance: f32,
    /// Distance of silence. Defaults to 100.
    pub max_distance: f32,
    /// Interpret `position`, `velocity` and `direction` in the listener's frame.
    pub relative: bool,
    /// Direction the source faces, for its cone. Defaults to -Z.
    pub direction: Vec3,
    /// Directional falloff. Defaults to omnidirectional.
    pub cone: Cone,
    /// Distance falloff model. Defaults to inverse distance with a linear cutoff.
    pub attenuation: Attenuation,
}

impl Default for Positional {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            min_distance: 1.0,
            max_distance: 100.0,
            relative: false,
            direction: Vec3::NEG_Z,
            cone: Cone::OMNI,
            attenuation: Attenuation::default(),
        }
    }
}

impl Positional {
    /// A source at `position` with every other field at its default.
    pub fn at(position: Vec3) -> Self {
        Self { position, ..Self::default() }
    }

    fn sanitized(mut self) -> Self {
        if !(self.min_distance.is_finite() && self.min_distance > 0.0) {
            self.min_distance = 1.0;
        }
        if !(self.max_distance.is_finite() && self.max_distance > self.min_distance) {
            self.max_distance = (self.min_distance * 2.0).max(100.0);
        }
        if !self.position.is_finite() {
            self.position = Vec3::ZERO;
        }
        if !self.velocity.is_finite() {
            self.velocity = Vec3::ZERO;
        }
        if !self.direction.is_finite() || self.direction.length() == 0.0 {
            self.direction = Vec3::NEG_Z;
        }
        self.direction = self.direction.normalize();
        self
    }
}

/// Options for a new voice. [`PlayOptions::default`] plays at unity gain, centred,
/// once, through the master bus.
#[derive(Clone, Debug)]
pub struct PlayOptions {
    /// Gain; 1 is unity.
    pub volume: f32,
    /// Stereo position from -1 (left) to +1 (right). Ignored when `positional` is set.
    pub pan: f32,
    /// Repeat a [`Sound`]. A stream controls its own looping.
    pub looping: bool,
    /// Loop region of a sound in seconds from its start, `(start, end)`. `None` loops the whole sound.
    pub loop_points: Option<(f64, f64)>,
    /// Playback rate; 2 plays an octave up. Clamped to 0.01..=64; zero or non-finite means 1.
    pub pitch: f32,
    /// Higher priority survives when the mixer is full.
    pub priority: i32,
    /// Seconds to rise from silence.
    pub fade_in: f32,
    /// Send level 0..=1 into the bus's reverb, or the mixer's shared reverb.
    pub reverb: f32,
    /// Low-pass cutoff in Hz; `None` leaves the sound unfiltered.
    pub low_pass: Option<f32>,
    /// The bus to play through; `None` is the master bus.
    pub bus: Option<Bus>,
    /// Occlusion: 0 clear, 1 fully blocked (20 dB down and muffled to 400 Hz).
    pub occlusion: f32,
    /// Place the voice in the world.
    pub positional: Option<Positional>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            looping: false,
            loop_points: None,
            pitch: 1.0,
            priority: 0,
            fade_in: 0.0,
            reverb: 0.0,
            low_pass: None,
            bus: None,
            occlusion: 0.0,
            positional: None,
        }
    }
}

/// Clamps a pitch to 0.01..=64, mapping zero and non-finite values to 1.
pub(crate) fn valid_pitch(p: f32) -> f32 {
    if p == 0.0 || !p.is_finite() { 1.0 } else { p.clamp(0.01, 64.0) }
}

/// Game-visible flags of a voice, shared with its handle.
#[derive(Default)]
pub(crate) struct VoiceShared {
    pub playing: AtomicBool,
    pub finished: AtomicBool,
    pub position: AtomicU64,
}

impl VoiceShared {
    pub(crate) fn position_frames(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }
}

pub(crate) enum Source {
    Sound { data: Arc<SoundData> },
    Stream(Box<StreamState>),
}

/// A volume change over time.
pub(crate) struct Fade {
    pub from: f32,
    pub to: f32,
    pub left: usize,
    pub total: usize,
    pub stop: bool,
}

impl Fade {
    pub(crate) fn value(&self) -> f32 {
        if self.total == 0 {
            return self.to;
        }
        let t = 1.0 - self.left as f32 / self.total as f32;
        self.from + (self.to - self.from) * t
    }
}

/// Everything the audio thread keeps for one voice.
pub(crate) struct VoiceState {
    pub id: u64,
    pub shared: Arc<VoiceShared>,
    pub source: Source,
    pub bus: usize,
    pub pos: f64,
    pub vol: f32,
    pub pan: f32,
    pub pitch: f32,
    pub looping: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    pub priority: i32,
    pub reverb: f32,
    pub paused: bool,
    pub mute: bool,
    pub solo: bool,
    pub stop: bool,
    pub stop_left: usize,
    pub done: bool,
    pub fade: Option<Fade>,
    pub lp_on: bool,
    pub lpc: Biquad,
    pub lps: LowPass,
    pub occlusion: f32,
    pub occc: Biquad,
    pub occs: LowPass,
    pub positional: bool,
    pub relative: bool,
    pub position: Vec3,
    pub velocity: Vec3,
    pub direction: Vec3,
    pub cone: Cone,
    pub attenuation: Attenuation,
    pub min_dist: f32,
    pub max_dist: f32,
    pub bin: Binaural,
    pub cur_l: f32,
    pub cur_r: f32,
    pub started: bool,
}

impl VoiceState {
    pub(crate) fn new(id: u64, rate: u32, source: Source, opts: &PlayOptions, bus: usize, shared: Arc<VoiceShared>) -> Self {
        let pos = opts.positional.map(Positional::sanitized);
        let p = pos.unwrap_or_default();
        let mut v = Self {
            id,
            shared,
            source,
            bus,
            pos: 0.0,
            vol: if opts.volume.is_finite() { opts.volume } else { 1.0 },
            pan: if opts.pan.is_finite() { opts.pan } else { 0.0 },
            pitch: valid_pitch(opts.pitch),
            looping: opts.looping,
            loop_start: 0.0,
            loop_end: 0.0,
            priority: opts.priority,
            reverb: opts.reverb,
            paused: false,
            mute: false,
            solo: false,
            stop: false,
            stop_left: 0,
            done: false,
            fade: None,
            lp_on: false,
            lpc: Biquad::default(),
            lps: LowPass::default(),
            occlusion: 0.0,
            occc: Biquad::default(),
            occs: LowPass::default(),
            positional: pos.is_some(),
            relative: p.relative,
            position: p.position,
            velocity: p.velocity,
            direction: p.direction,
            cone: p.cone,
            attenuation: p.attenuation,
            min_dist: p.min_distance,
            max_dist: p.max_distance,
            bin: Binaural::new(rate),
            cur_l: 0.0,
            cur_r: 0.0,
            started: false,
        };
        if let Some((a, b)) = opts.loop_points
            && a.is_finite()
            && b.is_finite()
            && a >= 0.0
            && b > a
        {
            v.loop_start = (a * f64::from(rate)).floor();
            v.loop_end = (b * f64::from(rate)).floor();
        }
        if let Some(c) = opts.low_pass.filter(|c| *c > 0.0) {
            v.lp_on = true;
            v.lpc = Biquad::lowpass(c, rate);
        }
        v.set_occlusion(opts.occlusion, rate);
        if opts.fade_in > 0.0 {
            let total = (opts.fade_in * rate as f32) as usize;
            v.fade = Some(Fade { from: 0.0, to: v.vol, left: total, total, stop: false });
        }
        v
    }

    pub(crate) fn set_occlusion(&mut self, o: f32, rate: u32) {
        let o = if o.is_nan() { 0.0 } else { o.clamp(0.0, 1.0) };
        self.occlusion = o;
        if o > 0.0 {
            self.occc = Biquad::lowpass(occlusion_cutoff(o), rate);
        }
    }

    /// Ends the voice at once and tells its handle.
    pub(crate) fn finish(&mut self) {
        self.done = true;
        self.shared.playing.store(false, Ordering::Relaxed);
        self.shared.finished.store(true, Ordering::Release);
    }

    /// Starts the ramp to silence.
    pub(crate) fn begin_stop(&mut self) {
        self.stop = true;
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub(crate) fn start_fade(&mut self, to: f32, seconds: f32, stop: bool, rate: u32) {
        let from = self.fade.as_ref().map_or(self.vol, Fade::value);
        let total = (seconds.max(0.0) * rate as f32) as usize;
        self.fade = Some(Fade { from, to, left: total, total, stop });
    }
}

impl From<&Sound> for Source {
    fn from(s: &Sound) -> Self {
        Source::Sound { data: s.data.clone() }
    }
}
