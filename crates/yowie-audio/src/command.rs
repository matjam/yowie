//! Messages from the game thread to the audio thread.

use crate::bus::BusState;
use crate::dsp::Biquad;
use crate::reverb::{Reverb, ReverbSettings};
use crate::spatial::{Attenuation, Cone, Listener, SpatialSettings};
use crate::voice::VoiceState;
use yowie_core::Vec3;

/// A change to one voice.
pub(crate) enum VoiceCmd {
    Stop,
    Volume(f32),
    Pan(f32),
    Pitch(f32),
    Paused(bool),
    Mute(bool),
    Solo(bool),
    Position(Vec3),
    Velocity(Vec3),
    Occlusion(f32),
    Reverb(f32),
    LowPass(Option<Biquad>),
    Fade { to: f32, seconds: f32, stop: bool },
    Relative(bool),
    Direction(Vec3),
    Cone(Cone),
    Attenuation(Attenuation),
    Distance(f32, f32),
    Seek(f64),
    Looping(bool),
}

/// Everything the game thread can ask of the audio thread. Anything that allocates is
/// built before sending, so the audio thread only moves it.
pub(crate) enum Command {
    Play(Box<VoiceState>),
    Voice(u64, VoiceCmd),
    StopAll,
    Listener(Listener),
    Spatial(SpatialSettings),
    Doppler(f32),
    SpeedOfSound(f32),
    MaxVoices(usize),
    Paused(bool),
    Reverb { settings: ReverbSettings, alloc: Option<Box<Reverb>>, drop: bool },
    Zones(Box<crate::core::ZoneList>),
    AddBus(Box<BusState>),
    BusReverb { bus: usize, reverb: Option<Box<Reverb>>, settings: ReverbSettings },
}

impl VoiceState {
    /// Applies a game-thread change to this voice.
    pub(crate) fn apply(&mut self, cmd: VoiceCmd, rate: u32) {
        use VoiceCmd as C;
        match cmd {
            C::Stop => self.begin_stop(),
            C::Volume(v) => {
                self.vol = if v.is_finite() { v } else { 1.0 };
                self.fade = None;
            }
            C::Pan(p) => self.pan = if p.is_finite() { p } else { 0.0 },
            C::Pitch(p) => self.pitch = crate::voice::valid_pitch(p),
            C::Paused(p) => self.paused = p,
            C::Mute(m) => self.mute = m,
            C::Solo(s) => self.solo = s,
            C::Position(p) => {
                self.position = p;
                self.positional = true;
            }
            C::Velocity(v) => self.velocity = v,
            C::Occlusion(o) => self.set_occlusion(o, rate),
            C::Reverb(r) => self.reverb = r,
            C::LowPass(c) => {
                self.lp_on = c.is_some();
                if let Some(c) = c {
                    self.lpc = c;
                }
            }
            C::Fade { to, seconds, stop } => self.start_fade(to, seconds, stop, rate),
            C::Relative(r) => {
                self.relative = r;
                self.positional = true;
            }
            C::Direction(d) => {
                self.direction = d;
                self.positional = true;
            }
            C::Cone(c) => {
                self.cone = c;
                self.positional = true;
            }
            C::Attenuation(a) => {
                self.attenuation = a;
                self.positional = true;
            }
            C::Distance(a, b) => {
                self.min_dist = a;
                self.max_dist = b;
                self.positional = true;
            }
            C::Seek(s) => self.seek(s, rate),
            C::Looping(l) => self.looping = l,
        }
    }

    fn seek(&mut self, seconds: f64, rate: u32) {
        let seconds = seconds.max(0.0);
        match &mut self.source {
            crate::voice::Source::Sound { data } => {
                self.pos = (seconds * f64::from(rate)).min((data.samples.len() / 2) as f64);
            }
            crate::voice::Source::Stream(s) => {
                s.seek(seconds);
                self.pos = seconds * f64::from(rate);
            }
        }
        self.shared.position.store(self.pos.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}
