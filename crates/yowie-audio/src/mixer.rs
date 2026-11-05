//! The game-facing mixer handle.

use crate::bus::{Bus, BusShared, BusState, MAX_BUSES};
use crate::command::Command;
use crate::core::{CHUNK, Core, MAX_VOICES, ZoneList};
use crate::error::{Error, Result};
use crate::handle::Voice;
use crate::reverb::{Reverb, ReverbSettings, ReverbZone};
use crate::sound::{Pcm, Sound};
use crate::spatial::{Listener, SpatialSettings};
use crate::stream::{Stream, StreamState};
use crate::voice::{PlayOptions, Source, VoiceShared, VoiceState};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

const QUEUE: usize = 4096;
const TRASH: usize = 1024;

type DoneCallback = Box<dyn FnOnce() + Send>;

/// State only the game thread touches.
struct GameState {
    buses: Vec<Arc<BusShared>>,
    reverb_alloc: bool,
    base_reverb: ReverbSettings,
    zones: Vec<ReverbZone>,
    watched: Vec<(Arc<VoiceShared>, DoneCallback)>,
}

/// Shared by the mixer, its buses and its voices.
pub(crate) struct Inner {
    pub rate: u32,
    tx: Sender<Command>,
    trash_rx: Receiver<Box<dyn Any + Send>>,
    next_id: AtomicU64,
    state: Mutex<GameState>,
}

impl Inner {
    /// Queues a command. A full queue drops it with a warning, since the audio thread
    /// must never wait for the game and the game must never wait for the audio thread.
    pub(crate) fn send(&self, cmd: Command) {
        if self.tx.try_send(cmd).is_err() {
            log::warn!("audio command queue is full; a change was dropped");
        }
    }

    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn watch(&self, shared: Arc<VoiceShared>, f: DoneCallback) {
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        st.watched.push((shared, f));
    }
}

/// The audio mixer: play sounds and streams, group them on buses, place them in the world.
///
/// `Mixer` is a cheap handle; clones control the same mixer, and it can be used from any
/// thread. Every call queues a message that the audio thread applies at the start of its
/// next chunk, so calls never wait for audio and gain changes ramp instead of clicking.
/// The mixer produces sound only while something drives its audio thread: an
/// [`Output`](crate::Output) for a device, or a [`Renderer`] for offline rendering.
#[derive(Clone)]
pub struct Mixer {
    pub(crate) inner: Arc<Inner>,
}

/// Renders a mixer into memory with no audio device.
///
/// Created by [`Mixer::offline`]. Playback advances only when [`Renderer::render`] is called.
pub struct Renderer {
    core: Core,
}

impl Renderer {
    /// Mixes interleaved stereo frames into `out`. An odd trailing sample is left untouched.
    pub fn render(&mut self, out: &mut [f32]) {
        let even = out.len() & !1;
        self.core.render(&mut out[..even]);
    }

    /// Mixes `frames` stereo frames into a new buffer.
    pub fn render_frames(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        self.render(&mut out);
        out
    }
}

impl Mixer {
    /// Makes a mixer and the [`Core`] that mixes for it.
    pub(crate) fn with_core(rate: u32) -> Result<(Mixer, Core)> {
        if rate == 0 {
            return Err(Error::Invalid("sample rate must be positive"));
        }
        let (tx, rx) = bounded(QUEUE);
        let (trash_tx, trash_rx) = bounded(TRASH);
        let names = ["master", "music", "effects"];
        let shared: Vec<Arc<BusShared>> = names
            .iter()
            .enumerate()
            .map(|(i, n)| Arc::new(BusShared::new(n, i)))
            .collect();
        let states = shared
            .iter()
            .map(|s| BusState::new(s.clone(), CHUNK * 2))
            .collect();
        let inner = Arc::new(Inner {
            rate,
            tx,
            trash_rx,
            next_id: AtomicU64::new(1),
            state: Mutex::new(GameState {
                buses: shared,
                reverb_alloc: false,
                base_reverb: ReverbSettings::default(),
                zones: Vec::new(),
                watched: Vec::new(),
            }),
        });
        Ok((Mixer { inner }, Core::new(rate, rx, trash_tx, states)))
    }

    /// Makes a mixer at `rate` Hz that renders into memory through the returned [`Renderer`].
    pub fn offline(rate: u32) -> Result<(Mixer, Renderer)> {
        let (m, core) = Self::with_core(rate)?;
        Ok((m, Renderer { core }))
    }

    /// The sample rate sounds are stored and mixed at.
    pub fn rate(&self) -> u32 {
        self.inner.rate
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GameState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs end-of-voice callbacks and frees what the audio thread has finished with.
    /// The engine calls this every frame; a program driving the mixer by hand should too.
    pub fn update(&self) {
        while self.inner.trash_rx.try_recv().is_ok() {}
        let ready: Vec<DoneCallback> = {
            let mut st = self.state();
            let mut ready = Vec::new();
            let mut i = 0;
            while i < st.watched.len() {
                if st.watched[i].0.finished.load(Ordering::Acquire) {
                    ready.push(st.watched.swap_remove(i).1);
                } else {
                    i += 1;
                }
            }
            ready
        };
        for f in ready {
            f();
        }
    }

    /// Converts PCM to a [`Sound`] at this mixer's rate.
    pub fn sound(&self, pcm: &Pcm) -> Result<Sound> {
        Sound::new(self.inner.rate, pcm)
    }

    /// Starts a sound and returns its voice.
    ///
    /// When the mixer is full of voices of higher priority, the returned voice is already finished.
    pub fn play(&self, sound: &Sound, opts: PlayOptions) -> Voice {
        self.start(Source::from(sound), opts)
    }

    /// Starts a stream and returns its voice. The stream moves to the audio thread.
    pub fn play_stream(&self, stream: impl Stream, opts: PlayOptions) -> Voice {
        self.start(
            Source::Stream(Box::new(StreamState::new(Box::new(stream)))),
            opts,
        )
    }

    fn start(&self, source: Source, opts: PlayOptions) -> Voice {
        self.update_trash();
        let id = self.inner.next_id();
        let shared = Arc::new(VoiceShared::default());
        shared.playing.store(true, Ordering::Relaxed);
        let bus = opts.bus.as_ref().map_or(0, |b| b.shared.index);
        let state = Box::new(VoiceState::new(
            id,
            self.inner.rate,
            source,
            &opts,
            bus,
            shared.clone(),
        ));
        if let Err(
            TrySendError::Full(Command::Play(mut v))
            | TrySendError::Disconnected(Command::Play(mut v)),
        ) = self.inner.tx.try_send(Command::Play(state))
        {
            v.finish();
        }
        Voice::new(id, shared, self.inner.clone())
    }

    fn update_trash(&self) {
        while self.inner.trash_rx.try_recv().is_ok() {}
    }

    /// The master bus, which every other bus feeds.
    pub fn master(&self) -> Bus {
        self.bus_at(0)
    }

    /// The built-in bus for music.
    pub fn music(&self) -> Bus {
        self.bus_at(1)
    }

    /// The built-in bus for sound effects.
    pub fn effects(&self) -> Bus {
        self.bus_at(2)
    }

    fn bus_at(&self, i: usize) -> Bus {
        let shared = self.state().buses[i].clone();
        Bus {
            shared,
            inner: self.inner.clone(),
        }
    }

    /// Finds a bus by name.
    pub fn bus(&self, name: &str) -> Option<Bus> {
        let shared = self
            .state()
            .buses
            .iter()
            .find(|b| b.name == name)
            .cloned()?;
        Some(Bus {
            shared,
            inner: self.inner.clone(),
        })
    }

    /// Makes a bus, or returns the existing one with that name. Fails past [`MAX_BUSES`].
    pub fn new_bus(&self, name: &str) -> Result<Bus> {
        if let Some(b) = self.bus(name) {
            return Ok(b);
        }
        let mut st = self.state();
        if st.buses.len() >= MAX_BUSES {
            return Err(Error::TooManyBuses(MAX_BUSES));
        }
        let shared = Arc::new(BusShared::new(name, st.buses.len()));
        st.buses.push(shared.clone());
        drop(st);
        self.inner.send(Command::AddBus(Box::new(BusState::new(
            shared.clone(),
            CHUNK * 2,
        ))));
        Ok(Bus {
            shared,
            inner: self.inner.clone(),
        })
    }

    /// Sets the master bus volume; 1 is unity.
    pub fn set_master_volume(&self, v: f32) {
        self.master().set_volume(v);
    }

    /// Caps how many voices play at once (default 64, at most 128). When full, a new voice
    /// replaces the quietest voice of the lowest priority no higher than its own, or is refused.
    pub fn set_max_voices(&self, n: usize) {
        self.inner.send(Command::MaxVoices(n.min(MAX_VOICES)));
    }

    /// Stops every voice with a short ramp to silence.
    pub fn stop_all(&self) {
        self.inner.send(Command::StopAll);
    }

    /// Holds every voice in place, silent, until resumed.
    pub fn set_paused(&self, paused: bool) {
        self.inner.send(Command::Paused(paused));
    }

    /// Moves the listener. Call each frame for a moving camera.
    pub fn set_listener(&self, l: Listener) {
        self.inner.send(Command::Listener(l.sanitized()));
    }

    /// Chooses stereo panning or the binaural head model for positional voices.
    pub fn set_spatial(&self, s: SpatialSettings) {
        self.inner.send(Command::Spatial(s));
    }

    /// Sets the Doppler factor; 0 turns Doppler off and 1 is physical.
    pub fn set_doppler(&self, factor: f32) {
        self.inner.send(Command::Doppler(if factor.is_finite() {
            factor.max(0.0)
        } else {
            0.0
        }));
    }

    /// Sets the speed of sound in world units per second (default 343). Ignored unless positive.
    pub fn set_speed_of_sound(&self, c: f32) {
        if c.is_finite() && c > 0.0 {
            self.inner.send(Command::SpeedOfSound(c));
        }
    }

    /// Configures the shared reverb that voices feed through their reverb send. The zero
    /// value turns it off.
    pub fn set_reverb(&self, s: ReverbSettings) {
        let mut st = self.state();
        st.base_reverb = s;
        self.sync_reverb(&mut st);
    }

    /// Replaces the set of reverb zones. Pass an empty slice to clear them.
    pub fn set_reverb_zones(&self, zones: &[ReverbZone]) {
        let mut st = self.state();
        st.zones = zones.to_vec();
        self.inner
            .send(Command::Zones(Box::new(ZoneList(st.zones.clone()))));
        self.sync_reverb(&mut st);
    }

    fn sync_reverb(&self, st: &mut GameState) {
        let needed = st.base_reverb.wet > 0.0 || !st.zones.is_empty();
        let alloc = (needed && !st.reverb_alloc).then(|| Box::new(Reverb::new(self.inner.rate)));
        let drop = !needed && st.reverb_alloc;
        st.reverb_alloc = needed;
        self.inner.send(Command::Reverb {
            settings: st.base_reverb,
            alloc,
            drop,
        });
    }
}

impl std::fmt::Debug for Mixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mixer")
            .field("rate", &self.inner.rate)
            .finish()
    }
}
