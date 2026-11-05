//! The audio thread's mixer.
//!
//! `Core` owns every voice and bus. It is driven by a device callback or by an offline
//! renderer, applies the commands the game thread queued, and mixes in chunks of
//! [`CHUNK`] frames. It never blocks, and allocates nothing in steady state: new voices
//! and buses arrive already built, and finished ones leave through the trash channel to
//! be dropped on the game thread.

use crate::binaural::head_radius;
use crate::bus::BusState;
use crate::command::Command;
use crate::dsp::Biquad;
use crate::mix::{Ctx, mix_voice};
use crate::reverb::{self, Reverb, ReverbSettings, ReverbZone};
use crate::spatial::{Listener, SpatialSettings};
use crate::voice::VoiceState;
use crossbeam_channel::{Receiver, Sender};
use std::any::Any;

/// Frames mixed per step; gain changes ramp across one step.
pub(crate) const CHUNK: usize = 256;
/// Capacity of the voice list; also the upper bound on the voice limit.
pub(crate) const MAX_VOICES: usize = 128;

/// A boxed list of reverb zones, boxed so it can leave the audio thread as `dyn Any`.
pub(crate) struct ZoneList(pub Vec<ReverbZone>);

pub(crate) type Trash = Sender<Box<dyn Any + Send>>;

pub(crate) struct Core {
    rate: u32,
    rx: Receiver<Command>,
    trash: Trash,
    // Boxed because voices arrive boxed and leave boxed through the trash channel; moving
    // the large state in and out would copy it on the audio thread.
    #[allow(clippy::vec_box)]
    voices: Vec<Box<VoiceState>>,
    max_voices: usize,
    buses: Vec<BusState>,
    listener: Listener,
    spatial: SpatialSettings,
    doppler: f32,
    speed_of_sound: f32,
    paused: bool,
    reverb: Option<Box<Reverb>>,
    base_reverb: ReverbSettings,
    applied_reverb: ReverbSettings,
    reverb_dirty: bool,
    zones: Box<ZoneList>,
    scratch: Vec<f32>,
    shared_send: Vec<f32>,
    stop_frames: usize,
}

impl Core {
    pub(crate) fn new(
        rate: u32,
        rx: Receiver<Command>,
        trash: Trash,
        mut buses: Vec<BusState>,
    ) -> Self {
        buses.reserve(crate::bus::MAX_BUSES);
        Self {
            rate,
            rx,
            trash,
            voices: Vec::with_capacity(MAX_VOICES * 2),
            max_voices: 64,
            buses,
            listener: Listener::default(),
            spatial: SpatialSettings::default(),
            doppler: 0.0,
            speed_of_sound: 343.0,
            paused: false,
            reverb: None,
            base_reverb: ReverbSettings::default(),
            applied_reverb: ReverbSettings::default(),
            reverb_dirty: false,
            zones: Box::new(ZoneList(Vec::new())),
            scratch: vec![0.0; CHUNK * 2],
            shared_send: vec![0.0; CHUNK * 2],
            stop_frames: (rate as usize / 1000).max(1),
        }
    }

    fn discard(&self, item: Box<dyn Any + Send>) {
        // When the channel is full the item is dropped here, which is rare and harmless.
        let _ = self.trash.try_send(item);
    }

    /// Renders interleaved stereo into `out`, applying queued commands between chunks.
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        for chunk in out.chunks_mut(CHUNK * 2) {
            while let Ok(cmd) = self.rx.try_recv() {
                self.apply(cmd);
            }
            self.mix_chunk(chunk);
        }
    }

    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Play(v) => self.add_voice(v),
            Command::Voice(id, c) => {
                let rate = self.rate;
                if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
                    v.apply(c, rate);
                }
            }
            Command::StopAll => {
                for v in &mut self.voices {
                    v.begin_stop();
                }
            }
            Command::Listener(l) => self.listener = l,
            Command::Spatial(s) => self.spatial = s,
            Command::Doppler(d) => self.doppler = d,
            Command::SpeedOfSound(c) => self.speed_of_sound = c,
            Command::MaxVoices(n) => self.max_voices = n.clamp(1, MAX_VOICES),
            Command::Paused(p) => self.paused = p,
            Command::Reverb {
                settings,
                alloc,
                drop,
            } => {
                self.base_reverb = settings;
                let old = if drop {
                    self.reverb.take()
                } else {
                    alloc.and_then(|r| self.reverb.replace(r))
                };
                if let Some(old) = old {
                    self.discard(old);
                }
                self.reverb_dirty = true;
            }
            Command::Zones(z) => {
                let old = std::mem::replace(&mut self.zones, z);
                self.discard(old);
                self.reverb_dirty = true;
            }
            Command::AddBus(b) => {
                if self.buses.len() < self.buses.capacity() {
                    self.buses.push(*b);
                }
            }
            Command::BusReverb {
                bus,
                reverb,
                settings,
            } => {
                if let Some(b) = self.buses.get_mut(bus) {
                    let old = match reverb {
                        Some(mut r) => {
                            r.set(settings);
                            b.reverb.replace(r)
                        }
                        None if settings.wet > 0.0 => {
                            if let Some(r) = b.reverb.as_mut() {
                                r.set(settings);
                            }
                            None
                        }
                        None => b.reverb.take(),
                    };
                    if let Some(old) = old {
                        self.discard(old);
                    }
                }
            }
        }
    }

    /// Adds a voice, stealing a lower-priority one when the mixer is full.
    fn add_voice(&mut self, v: Box<VoiceState>) {
        let mut v = v;
        let active = self.voices.iter().filter(|o| !o.stop && !o.done).count();
        let mut victim: Option<usize> = None;
        if active >= self.max_voices {
            for (i, o) in self.voices.iter().enumerate() {
                if o.stop || o.done || o.priority > v.priority {
                    continue;
                }
                let better = victim.is_none_or(|b| {
                    let w = &self.voices[b];
                    o.priority < w.priority
                        || (o.priority == w.priority && o.cur_l + o.cur_r < w.cur_l + w.cur_r)
                });
                if better {
                    victim = Some(i);
                }
            }
            if victim.is_none() {
                v.finish();
                self.discard(v);
                return;
            }
        }
        if self.voices.len() >= self.voices.capacity() {
            // Already ramping out a full set of stolen voices: cut the victim.
            let i = victim.unwrap_or(0);
            self.voices[i].finish();
            let old = std::mem::replace(&mut self.voices[i], v);
            self.discard(old);
            return;
        }
        if let Some(i) = victim {
            self.voices[i].begin_stop();
        }
        self.voices.push(v);
    }

    fn update_reverb(&mut self) {
        let Some(rv) = self.reverb.as_mut() else {
            return;
        };
        let s = reverb::effective(self.base_reverb, &self.zones.0, self.listener.position);
        if self.reverb_dirty || s != self.applied_reverb {
            rv.set(s);
            self.applied_reverb = s;
            self.reverb_dirty = false;
        }
    }

    fn mix_chunk(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        let n2 = n * 2;
        for b in &mut self.buses {
            b.dry[..n2].fill(0.0);
            b.send[..n2].fill(0.0);
        }
        self.shared_send[..n2].fill(0.0);
        self.update_reverb();
        let cx = Ctx {
            rate: self.rate,
            listener: &self.listener,
            doppler: self.doppler,
            speed_of_sound: self.speed_of_sound,
            binaural: self.spatial.binaural,
            head_radius: head_radius(self.spatial.head_radius),
            paused: self.paused,
            solo_voices: self.voices.iter().any(|v| v.solo),
            stop_frames: self.stop_frames,
        };
        let mut i = 0;
        while i < self.voices.len() {
            if mix_voice(
                &mut self.voices[i],
                &cx,
                &mut self.buses,
                &mut self.scratch,
                n,
            ) {
                i += 1;
            } else {
                let v = self.voices.swap_remove(i);
                self.discard(v);
            }
        }
        self.finish_buses(out);
    }

    /// Applies bus gains and filters, adds reverb tails, and writes the master bus to `out`.
    fn finish_buses(&mut self, out: &mut [f32]) {
        let n2 = out.len();
        let solo_buses = self.buses.iter().any(|b| b.shared.soloed());
        let rate = self.rate;
        let (master, rest) = self.buses.split_at_mut(1);
        let master = &mut master[0];
        for b in rest.iter_mut() {
            bus_stage(b, n2, rate, solo_buses);
            for (m, d) in master.dry[..n2].iter_mut().zip(&b.dry[..n2]) {
                *m += d;
            }
            match b.reverb.as_mut() {
                Some(rv) => rv.process(&b.send[..n2], &mut master.dry[..n2]),
                None => {
                    for (s, d) in self.shared_send[..n2].iter_mut().zip(&b.send[..n2]) {
                        *s += d;
                    }
                }
            }
        }
        for (s, d) in self.shared_send[..n2].iter_mut().zip(&master.send[..n2]) {
            *s += d;
        }
        if let Some(rv) = self.reverb.as_mut() {
            rv.process(&self.shared_send[..n2], &mut master.dry[..n2]);
        }
        bus_stage(master, n2, rate, false);
        for (o, s) in out.iter_mut().zip(&master.dry[..n2]) {
            // A NaN or infinity from a decoder or game stream would reach the device as
            // a click or worse, so it becomes silence; clamp alone would pass a NaN on.
            *o = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
}

/// Gain ramp, mute and low-pass for one bus over `n2` samples.
fn bus_stage(b: &mut BusState, n2: usize, rate: u32, solo_buses: bool) {
    let silent = b.shared.muted() || (solo_buses && !b.shared.soloed());
    let target = if silent { 0.0 } else { b.shared.volume() };
    let n = n2 / 2;
    let inv = 1.0 / n.max(1) as f32;
    let d = (target - b.gain) * inv;
    let mut g = b.gain;
    for i in 0..n {
        g += d;
        for c in 0..2 {
            b.dry[i * 2 + c] *= g;
            b.send[i * 2 + c] *= g;
        }
    }
    b.gain = target;
    let cutoff = b.shared.cutoff();
    if cutoff != b.applied_cutoff {
        b.applied_cutoff = cutoff;
        if cutoff > 0.0 {
            b.lpc = Biquad::lowpass(cutoff, rate);
        }
    }
    if cutoff > 0.0 {
        b.lps.process(&b.lpc, &mut b.dry[..n2]);
    }
}
