//! The module player: a [`Stream`] that renders a [`Module`].

use super::module::{Format, Module};
use super::state::{Channel, Filter, Voice};
use super::tables::{amiga_period_xm, linear_period, mod_period, s3m_period};
use crate::stream::Stream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const NO_SEEK: u64 = u64::MAX;

/// Options fixed when a player is made.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerOptions {
    /// Restart at the module's restart position when the order list ends.
    pub looping: bool,
    /// Enable the ProTracker LED low-pass filter (E00 and E01). Off by default, since most
    /// players and listeners expect the unfiltered mix.
    pub amiga_filter: bool,
    /// Interpolate samples with a four-point Hermite curve instead of linearly, which
    /// rounds off the aliasing that makes chip samples sound harsh. Off by default to
    /// match the classic sound.
    pub cubic: bool,
}

/// State shared between the audio thread and the game: position, seeks, mute and solo.
struct Control {
    position: AtomicU64,
    seek: AtomicU64,
    mute: AtomicU64,
    solo: AtomicU64,
    finished: AtomicBool,
    channels: usize,
    module: Arc<Module>,
}

/// A handle to control and observe a [`Player`] after it has moved to the mixer.
#[derive(Clone)]
pub struct PlayerControl(Arc<Control>);

impl PlayerControl {
    /// The current song position (an index into the order list) and the row within its pattern.
    pub fn position(&self) -> (usize, usize) {
        let p = self.0.position.load(Ordering::Relaxed);
        ((p >> 32) as usize, (p & 0xFFFF_FFFF) as usize)
    }

    /// The number of song positions, the range [`PlayerControl::seek`] accepts.
    pub fn length(&self) -> usize {
        self.0.module.orders.len()
    }

    /// The number of rows in the pattern at a song position, or 0 outside the song.
    pub fn rows(&self, order: usize) -> usize {
        let m = &self.0.module;
        m.orders
            .get(order)
            .and_then(|&p| m.patterns.get(p))
            .map_or(0, |p| p.rows.len())
    }

    /// The number of pattern channels, the range [`PlayerControl::mute`] accepts.
    pub fn channels(&self) -> usize {
        self.0.channels
    }

    /// Jumps to a song position and row and plays on from there, cutting what was sounding.
    /// Order and row are clamped to the song. Speed, tempo and global volume stay as they
    /// are. Seeking a finished song starts it again.
    pub fn seek(&self, order: usize, row: usize) {
        self.0.seek.store(
            (order.min(u32::MAX as usize) as u64) << 32 | row.min(u32::MAX as usize) as u64,
            Ordering::Release,
        );
    }

    /// Silences a pattern channel while the song plays on. Notes it starts still run
    /// their course, so unmuting is seamless. Channels past 63 are ignored.
    pub fn mute(&self, channel: usize, mute: bool) {
        set_bit(&self.0.mute, channel, mute);
    }

    /// Whether a channel is muted.
    pub fn muted(&self, channel: usize) -> bool {
        get_bit(&self.0.mute, channel)
    }

    /// Auditions a channel: while any channel is soloed, only soloed channels are heard.
    pub fn solo(&self, channel: usize, solo: bool) {
        set_bit(&self.0.solo, channel, solo);
    }

    /// Whether a channel is soloed.
    pub fn soloed(&self, channel: usize) -> bool {
        get_bit(&self.0.solo, channel)
    }

    /// Whether a non-looping song has ended.
    pub fn finished(&self) -> bool {
        self.0.finished.load(Ordering::Acquire)
    }
}

fn set_bit(a: &AtomicU64, i: usize, on: bool) {
    if i < 64 {
        if on {
            a.fetch_or(1 << i, Ordering::Relaxed);
        } else {
            a.fetch_and(!(1 << i), Ordering::Relaxed);
        }
    }
}

fn get_bit(a: &AtomicU64, i: usize) -> bool {
    i < 64 && a.load(Ordering::Relaxed) & (1 << i) != 0
}

/// Renders a [`Module`] to stereo `f32` at a fixed rate.
///
/// A `Player` is a [`Stream`], so it plays through the mixer like any other voice;
/// playback is deterministic, so a module renders the same samples on every platform.
/// Keep a [`PlayerControl`] from [`Player::control`] to seek, mute and read the position.
pub struct Player {
    module: Arc<Module>,
    pub(crate) st: State,
    ctl: Arc<Control>,
}

/// Everything that changes while a song plays.
pub(crate) struct State {
    pub rate: u32,
    pub opts: PlayerOptions,
    pub chans: Vec<Channel>,
    pub bg: Vec<Voice>,
    pub mute_mask: u64,
    pub solo_mask: u64,
    pub order: usize,
    pub row: usize,
    pub speed: usize,
    pub tempo: usize,
    pub tick: usize,
    pub tick_frames: usize,
    pub tick_remaining: usize,
    pub global_vol: i32,
    pub pat_delay: usize,
    pub jump_order: i32,
    pub break_row: i32,
    pub loop_row: usize,
    pub loop_count: i32,
    pub loop_jump: bool,
    pub done: bool,
    pub started: bool,
    pub gain: f32,
    pub amiga_filter: bool,
    pub filter_l: Filter,
    pub filter_r: Filter,
}

impl Player {
    /// Prepares a module for playback at `rate` Hz with default options.
    pub fn new(module: Arc<Module>, rate: u32) -> Self {
        Self::with_options(module, rate, PlayerOptions::default())
    }

    /// Prepares a module for playback at `rate` Hz.
    pub fn with_options(module: Arc<Module>, rate: u32, opts: PlayerOptions) -> Self {
        let st = State::new(&module, rate, opts);
        let ctl = Arc::new(Control {
            position: AtomicU64::new(0),
            seek: AtomicU64::new(NO_SEEK),
            mute: AtomicU64::new(0),
            solo: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            channels: module.channels,
            module: module.clone(),
        });
        Self { module, st, ctl }
    }

    /// A handle that controls this player after it has moved to the mixer.
    pub fn control(&self) -> PlayerControl {
        PlayerControl(self.ctl.clone())
    }

    /// The song being played.
    pub fn module(&self) -> &Module {
        &self.module
    }
}

impl Stream for Player {
    fn read(&mut self, out: &mut [f32]) -> usize {
        let seek = self.ctl.seek.swap(NO_SEEK, Ordering::AcqRel);
        if seek != NO_SEEK {
            self.st.seek(
                &self.module,
                (seek >> 32) as usize,
                (seek & 0xFFFF_FFFF) as usize,
            );
        }
        self.st.mute_mask = self.ctl.mute.load(Ordering::Relaxed);
        self.st.solo_mask = self.ctl.solo.load(Ordering::Relaxed);
        let frames = out.len() / 2;
        let mut written = 0;
        while written < frames && !self.st.done {
            if self.st.tick_remaining == 0 {
                self.st.advance_tick(&self.module);
                if self.st.done {
                    break;
                }
                self.st.tick_remaining = self.st.tick_frames;
            }
            let n = (frames - written).min(self.st.tick_remaining);
            self.st
                .render(&self.module, &mut out[written * 2..(written + n) * 2]);
            written += n;
            self.st.tick_remaining -= n;
        }
        self.ctl.position.store(
            (self.st.order as u64) << 32 | self.st.row as u64,
            Ordering::Relaxed,
        );
        self.ctl.finished.store(self.st.done, Ordering::Release);
        written
    }
}

impl State {
    fn new(m: &Module, rate: u32, opts: PlayerOptions) -> Self {
        let chans = (0..m.channels)
            .map(|i| {
                let pan = m.pan.get(i).copied().unwrap_or(0.0);
                let mut ch = Channel {
                    pan,
                    chan_vol: m.channel_vol.get(i).map_or(64, |v| (*v).min(64) as i32),
                    ..Channel::default()
                };
                ch.v.pan = pan;
                ch.v.index = i;
                ch
            })
            .collect();
        // Per-channel gain follows the file's mixing volume, as the reference players do,
        // rather than the channel count: 48/128 puts four channels at full volume just short
        // of clipping, which is the Amiga's balance.
        let mix = if m.mix_volume == 0 { 48 } else { m.mix_volume };
        let mut gain = 2.0 / 3.0 * mix as f32 / 128.0;
        match m.format {
            Format::It => gain /= 4.0, // Impulse Tracker's mixing volume sits four times lower
            Format::S3m => gain *= 0.8, // ScreamTracker mixes about 2 dB lower
            _ => {}
        }
        let mut st = Self {
            rate,
            opts,
            chans,
            bg: Vec::with_capacity(64),
            mute_mask: 0,
            solo_mask: 0,
            order: 0,
            row: 0,
            speed: m.speed.max(1),
            tempo: m.tempo,
            tick: 0,
            tick_frames: 1,
            tick_remaining: 0,
            global_vol: if m.global_volume == 0 {
                128
            } else {
                m.global_volume as i32
            },
            pat_delay: 0,
            jump_order: -1,
            break_row: -1,
            loop_row: 0,
            loop_count: 0,
            loop_jump: false,
            done: false,
            started: false,
            gain,
            amiga_filter: false,
            filter_l: Filter::default(),
            filter_r: Filter::default(),
        };
        st.set_tempo(m.tempo);
        st.filter_l.set_cutoff(3200.0, 0.7, rate);
        st.filter_r.set_cutoff(3200.0, 0.7, rate);
        st
    }

    pub(crate) fn set_tempo(&mut self, bpm: usize) {
        let bpm = bpm.max(32);
        self.tempo = bpm;
        self.tick_frames = ((f64::from(self.rate) * 2.5 / bpm as f64) as usize).max(1);
    }

    /// Whether a channel is heard under the mute and solo state.
    pub(crate) fn audible(&self, channel: usize) -> bool {
        if channel >= 64 {
            return true;
        }
        let bit = 1u64 << channel;
        self.mute_mask & bit == 0 && (self.solo_mask == 0 || self.solo_mask & bit != 0)
    }

    pub(crate) fn seek(&mut self, m: &Module, order: usize, row: usize) {
        if m.orders.is_empty() {
            return;
        }
        let order = order.min(m.orders.len() - 1);
        let rows = rows_at(m, order);
        self.order = order;
        self.row = row.min(rows.saturating_sub(1));
        self.tick = 0;
        self.tick_remaining = 0;
        self.pat_delay = 0;
        self.jump_order = -1;
        self.break_row = -1;
        self.loop_row = 0;
        self.loop_count = 0;
        self.loop_jump = false;
        self.done = false;
        self.started = false;
        self.bg.clear();
        for ch in &mut self.chans {
            ch.v.active = false;
            ch.pending_sample = None;
            ch.note_cut = -1;
            ch.note_delay = -1;
            ch.key_off_tick = -1;
        }
    }

    /// Moves to the next tick, processing a new row when needed.
    pub(crate) fn advance_tick(&mut self, m: &Module) {
        if !self.started {
            self.started = true;
            if m.orders.is_empty() || m.patterns.is_empty() {
                self.done = true;
                return;
            }
            if !self.valid_position(m) {
                self.next_row(m);
                if self.done {
                    return;
                }
            }
            self.tick = 0;
            self.process_row(m);
            self.update_voices(m);
            return;
        }
        self.tick += 1;
        if self.tick < self.speed {
            self.process_tick(m);
            self.update_voices(m);
            return;
        }
        self.tick = 0;
        if self.pat_delay > 0 {
            // A delayed row repeats its tick effects but not its notes or row effects, or the
            // delay would arm itself again and retrigger every note on each repeat.
            self.pat_delay -= 1;
            self.process_tick(m);
            self.update_voices(m);
            return;
        }
        self.next_row(m);
        if !self.done {
            self.process_row(m);
            self.update_voices(m);
        }
    }

    fn valid_position(&self, m: &Module) -> bool {
        m.orders
            .get(self.order)
            .and_then(|&p| m.patterns.get(p))
            .is_some_and(|p| self.row < p.rows.len())
    }

    /// Applies pending jumps, breaks and loops, then steps the row.
    fn next_row(&mut self, m: &Module) {
        if self.loop_jump {
            self.loop_jump = false;
            self.row = self.loop_row;
        } else if self.jump_order >= 0 || self.break_row >= 0 {
            if self.jump_order >= 0 {
                self.order = self.jump_order as usize;
            } else {
                self.order += 1;
            }
            self.row = self.break_row.max(0) as usize;
            self.jump_order = -1;
            self.break_row = -1;
            self.loop_row = 0;
            self.loop_count = 0;
        } else {
            self.row += 1;
        }
        // A module whose orders name no playable pattern would otherwise be scanned forever.
        for _ in 0..=m.orders.len() {
            if self.order >= m.orders.len() {
                if !self.opts.looping {
                    self.done = true;
                    return;
                }
                self.order = m.restart.min(m.orders.len().saturating_sub(1));
                self.row = 0;
            }
            if self.valid_position(m) {
                return;
            }
            self.order += 1;
            self.row = 0;
        }
        self.done = true;
    }

    /// The cells of the current row.
    pub(crate) fn current_row<'a>(&self, m: &'a Module) -> &'a [super::module::Cell] {
        m.orders
            .get(self.order)
            .and_then(|&p| m.patterns.get(p))
            .and_then(|p| p.rows.get(self.row))
            .map_or(&[], Vec::as_slice)
    }

    /// One unit of portamento in the format's period space.
    pub(crate) fn slide_unit(&self, m: &Module) -> f64 {
        if m.format == Format::Mod { 1.0 } else { 4.0 }
    }

    /// Whether a zero parameter recalls the previous one; ProTracker slides have no memory.
    pub(crate) fn has_memory(&self, m: &Module) -> bool {
        m.format != Format::Mod
    }
}

fn rows_at(m: &Module, order: usize) -> usize {
    m.orders
        .get(order)
        .and_then(|&p| m.patterns.get(p))
        .map_or(0, |p| p.rows.len())
}

/// Converts a note index to the period of a sample.
pub(crate) fn note_period(m: &Module, sample: Option<usize>, note: i32) -> f64 {
    let Some(s) = sample.and_then(|i| m.samples.get(i)) else {
        return 0.0;
    };
    note_period_for(m, s.finetune, s.c4_speed, s.relative_note, note)
}

pub(crate) fn note_period_for(
    m: &Module,
    finetune: f32,
    c4_speed: usize,
    relative_note: i32,
    note: i32,
) -> f64 {
    match m.format {
        Format::Mod => return mod_period(note, finetune),
        Format::S3m => return s3m_period(note, c4_speed),
        _ => {}
    }
    let semis = if m.format == Format::Xm {
        f64::from(note + relative_note) + f64::from(finetune)
    } else {
        let c5 = if c4_speed == 0 {
            8363.0
        } else {
            c4_speed as f64
        };
        f64::from(note - 12) + 12.0 * (c5 / 8363.0).log2()
    };
    if m.linear_slides {
        linear_period(semis)
    } else {
        amiga_period_xm(semis)
    }
}
