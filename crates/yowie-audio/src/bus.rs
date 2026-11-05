//! Buses: groups of voices that share volume, mute, pause, a low-pass and a reverb.

use crate::command::Command;
use crate::dsp::{Biquad, LowPass};
use crate::mixer::Inner;
use crate::reverb::{Reverb, ReverbSettings};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The most buses a mixer holds, including the master bus.
pub const MAX_BUSES: usize = 32;

/// Settings the game thread writes and the audio thread reads every chunk.
pub(crate) struct BusShared {
    pub name: String,
    pub index: usize,
    volume: AtomicU32,
    cutoff: AtomicU32,
    mute: AtomicBool,
    solo: AtomicBool,
    paused: AtomicBool,
}

impl BusShared {
    pub(crate) fn new(name: &str, index: usize) -> Self {
        Self {
            name: name.to_string(),
            index,
            volume: AtomicU32::new(1.0f32.to_bits()),
            cutoff: AtomicU32::new(0.0f32.to_bits()),
            mute: AtomicBool::new(false),
            solo: AtomicBool::new(false),
            paused: AtomicBool::new(false),
        }
    }

    pub(crate) fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
    pub(crate) fn cutoff(&self) -> f32 {
        f32::from_bits(self.cutoff.load(Ordering::Relaxed))
    }
    pub(crate) fn muted(&self) -> bool {
        self.mute.load(Ordering::Relaxed)
    }
    pub(crate) fn soloed(&self) -> bool {
        self.solo.load(Ordering::Relaxed)
    }
    pub(crate) fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
}

/// A handle to a bus. Clones refer to the same bus.
///
/// Settings take effect on the next audio chunk and ramp, so changing them never clicks.
/// A mixer has a master bus, [`Mixer::music`](crate::Mixer::music) and
/// [`Mixer::effects`](crate::Mixer::effects), and any buses made with
/// [`Mixer::new_bus`](crate::Mixer::new_bus).
#[derive(Clone)]
pub struct Bus {
    pub(crate) shared: Arc<BusShared>,
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bus")
            .field("name", &self.shared.name)
            .field("index", &self.shared.index)
            .finish()
    }
}

impl Bus {
    /// The bus name.
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// Sets the gain applied to everything on the bus; 1 is unity.
    pub fn set_volume(&self, v: f32) {
        let v = if v.is_finite() { v.max(0.0) } else { 1.0 };
        self.shared.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    /// The gain set by [`Bus::set_volume`].
    pub fn volume(&self) -> f32 {
        self.shared.volume()
    }

    /// Silences the bus while its voices keep playing.
    pub fn set_mute(&self, mute: bool) {
        self.shared.mute.store(mute, Ordering::Relaxed);
    }

    /// Whether the bus is muted.
    pub fn muted(&self) -> bool {
        self.shared.muted()
    }

    /// Solos the bus. While any bus is soloed, buses that are not soloed are silent.
    pub fn set_solo(&self, solo: bool) {
        self.shared.solo.store(solo, Ordering::Relaxed);
    }

    /// Whether the bus is soloed.
    pub fn soloed(&self) -> bool {
        self.shared.soloed()
    }

    /// Holds every voice on the bus in place, silent, until resumed.
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// Whether the bus is paused.
    pub fn paused(&self) -> bool {
        self.shared.paused()
    }

    /// Sets a low-pass cutoff in Hz over the whole bus; zero or less removes the filter.
    pub fn set_low_pass(&self, cutoff_hz: f32) {
        let c = if cutoff_hz.is_finite() {
            cutoff_hz.max(0.0)
        } else {
            0.0
        };
        self.shared.cutoff.store(c.to_bits(), Ordering::Relaxed);
    }

    /// Gives the bus a reverb of its own, which its voices' reverb sends feed instead of
    /// the mixer's shared one. Settings with `wet` of zero remove it.
    pub fn set_reverb(&self, settings: ReverbSettings) {
        let alloc = (settings.wet > 0.0).then(|| Box::new(Reverb::new(self.inner.rate)));
        self.inner.send(Command::BusReverb {
            bus: self.shared.index,
            reverb: alloc,
            settings,
        });
    }
}

/// The audio thread's state for a bus.
pub(crate) struct BusState {
    pub shared: Arc<BusShared>,
    pub dry: Vec<f32>,
    pub send: Vec<f32>,
    pub gain: f32,
    pub lpc: Biquad,
    pub lps: LowPass,
    pub applied_cutoff: f32,
    pub reverb: Option<Box<Reverb>>,
}

impl BusState {
    pub(crate) fn new(shared: Arc<BusShared>, chunk_samples: usize) -> Self {
        Self {
            shared,
            dry: vec![0.0; chunk_samples],
            send: vec![0.0; chunk_samples],
            gain: 1.0,
            lpc: Biquad::default(),
            lps: LowPass::default(),
            applied_cutoff: 0.0,
            reverb: None,
        }
    }
}
