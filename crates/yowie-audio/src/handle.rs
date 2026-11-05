//! The game-facing handle to a playing voice.

use crate::command::{Command, VoiceCmd};
use crate::dsp::Biquad;
use crate::mixer::Inner;
use crate::spatial::{Attenuation, Cone};
use crate::voice::VoiceShared;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use yowie_core::Vec3;

/// A handle to one playing sound or stream.
///
/// Dropping the handle does not stop the voice. Setters queue a change that the audio
/// thread applies at the start of its next chunk, and gain changes ramp across that
/// chunk so nothing clicks. Setters on a finished voice do nothing.
#[derive(Clone)]
pub struct Voice {
    id: u64,
    shared: Arc<VoiceShared>,
    inner: Arc<Inner>,
}

impl Voice {
    pub(crate) fn new(id: u64, shared: Arc<VoiceShared>, inner: Arc<Inner>) -> Self {
        Self { id, shared, inner }
    }

    fn cmd(&self, c: VoiceCmd) {
        self.inner.send(Command::Voice(self.id, c));
    }

    /// Stops the voice. Its gain ramps to silence over about a millisecond first, so
    /// stopping mid-cycle does not click. [`Voice::is_playing`] reports false at once.
    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.cmd(VoiceCmd::Stop);
    }

    /// Whether the voice is active, including while paused, muted or out of range.
    /// False as soon as it is stopped or has played out.
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// Whether the voice has ended and left the mixer, including its stop ramp.
    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    /// How far into the sound the voice is, in seconds, as of the last chunk mixed.
    ///
    /// For a stream this counts source time since the voice started or last sought,
    /// including pitch and underrun silence, and does not wrap at loops.
    pub fn position(&self) -> f64 {
        self.shared.position_frames() / f64::from(self.inner.rate)
    }

    /// Registers `f` to run when the voice ends, whether it played out, was stopped or
    /// lost its slot. It runs on the thread that calls [`Mixer::update`](crate::Mixer::update).
    /// Every registered callback runs once.
    pub fn on_done(&self, f: impl FnOnce() + Send + 'static) {
        self.inner.watch(self.shared.clone(), Box::new(f));
    }

    /// Sets the gain; 1 is unity. Cancels a fade in progress.
    pub fn set_volume(&self, v: f32) {
        self.cmd(VoiceCmd::Volume(v));
    }

    /// Sets the stereo position from -1 (left) to +1 (right).
    pub fn set_pan(&self, pan: f32) {
        self.cmd(VoiceCmd::Pan(pan));
    }

    /// Sets the playback rate; 2 plays an octave up. Clamped to 0.01..=64.
    pub fn set_pitch(&self, p: f32) {
        self.cmd(VoiceCmd::Pitch(p));
    }

    /// Holds the voice in place, silent, until resumed. The chunk it lands in fades out.
    pub fn set_paused(&self, paused: bool) {
        self.cmd(VoiceCmd::Paused(paused));
    }

    /// Silences the voice while it keeps playing. To stop it advancing, pause it instead.
    pub fn set_mute(&self, mute: bool) {
        self.cmd(VoiceCmd::Mute(mute));
    }

    /// Solos the voice. While any voice is soloed, voices that are not soloed are silent.
    pub fn set_solo(&self, solo: bool) {
        self.cmd(VoiceCmd::Solo(solo));
    }

    /// Moves the voice to `p` in world units and makes it positional.
    pub fn set_position(&self, p: Vec3) {
        self.cmd(VoiceCmd::Position(p));
    }

    /// Sets the velocity in world units per second, used for Doppler. It changes only
    /// the pitch; move the voice with [`Voice::set_position`].
    pub fn set_velocity(&self, v: Vec3) {
        self.cmd(VoiceCmd::Velocity(v));
    }

    /// Sets how blocked the path from the source is: 0 clear, 1 fully blocked, which
    /// drops the voice 20 dB and muffles it to 400 Hz.
    pub fn set_occlusion(&self, o: f32) {
        self.cmd(VoiceCmd::Occlusion(o));
    }

    /// Sets the send level into the bus's reverb, or the mixer's shared reverb.
    pub fn set_reverb(&self, send: f32) {
        self.cmd(VoiceCmd::Reverb(send));
    }

    /// Sets the low-pass cutoff in Hz; `None` removes the filter.
    pub fn set_low_pass(&self, cutoff: Option<f32>) {
        let c = cutoff
            .filter(|c| *c > 0.0)
            .map(|c| Biquad::lowpass(c, self.inner.rate));
        self.cmd(VoiceCmd::LowPass(c));
    }

    /// Moves the volume to `volume` over `seconds`.
    pub fn fade_to(&self, volume: f32, seconds: f32) {
        self.cmd(VoiceCmd::Fade {
            to: volume,
            seconds,
            stop: false,
        });
    }

    /// Fades to silence over `seconds`, then stops the voice.
    pub fn fade_out(&self, seconds: f32) {
        self.cmd(VoiceCmd::Fade {
            to: 0.0,
            seconds,
            stop: true,
        });
    }

    /// Interprets the voice's position, velocity and direction in the listener's frame.
    pub fn set_relative_to_listener(&self, relative: bool) {
        self.cmd(VoiceCmd::Relative(relative));
    }

    /// Sets the direction the source faces, for its cone. Ignored when zero or not finite.
    pub fn set_direction(&self, d: Vec3) {
        if d.is_finite() && d.length() > 0.0 {
            self.cmd(VoiceCmd::Direction(d.normalize()));
        }
    }

    /// Sets the source cone.
    pub fn set_cone(&self, cone: Cone) {
        self.cmd(VoiceCmd::Cone(cone));
    }

    /// Sets the distance attenuation model.
    pub fn set_attenuation(&self, a: Attenuation) {
        self.cmd(VoiceCmd::Attenuation(a));
    }

    /// Sets the full-volume radius and the distance of silence. Ignored unless
    /// `0 < min < max` and both are finite.
    pub fn set_distance_range(&self, min: f32, max: f32) {
        if min.is_finite() && max.is_finite() && min > 0.0 && max > min {
            self.cmd(VoiceCmd::Distance(min, max));
        }
    }

    /// Moves a sound to `seconds` from its start, or asks a stream to seek. A stream
    /// that cannot seek stays where it is.
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() {
            self.cmd(VoiceCmd::Seek(seconds));
        }
    }

    /// Turns looping of a sound on or off.
    pub fn set_looping(&self, looping: bool) {
        self.cmd(VoiceCmd::Looping(looping));
    }
}

impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice").field("id", &self.id).finish()
    }
}
