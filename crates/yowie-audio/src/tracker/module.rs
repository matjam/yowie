//! The song model shared by every loader.

use super::effects::{Effect, VolCmd};

/// A song in memory, in a form shared by every loader.
#[derive(Clone, Debug, Default)]
pub struct Module {
    /// Display title from the file.
    pub title: String,
    /// Pattern channels, excluding background voices started by new-note actions.
    pub channels: usize,
    /// Decoded samples referenced by instruments and cells.
    pub samples: Vec<Sample>,
    /// Instruments; empty for sample-only formats (MOD, S3M).
    pub instruments: Vec<Instrument>,
    /// Pattern data, indexed by `orders`.
    pub patterns: Vec<Pattern>,
    /// Pattern index per song position.
    pub orders: Vec<usize>,
    /// Song position to loop back to.
    pub restart: usize,
    /// Initial ticks per row.
    pub speed: usize,
    /// Initial tempo in beats per minute.
    pub tempo: usize,
    /// Initial pan per channel, -1 left to +1 right. Must hold `channels` entries.
    pub pan: Vec<f32>,
    /// Channel volume 0..=64 per channel; empty means 64.
    pub channel_vol: Vec<usize>,
    /// Selects the source format's playback semantics.
    pub format: Format,
    /// Global volume 0..=128.
    pub global_volume: usize,
    /// The file's mixing volume 0..=128; per-channel gain follows it.
    pub mix_volume: usize,
    /// XM and IT: slides move pitch by fractions of a semitone.
    pub linear_slides: bool,
    /// IT "old effects" flag.
    pub old_effects: bool,
    /// IT: Gxx shares memory with Exx and Fxx.
    pub compat_gxx: bool,
}

/// The format a module was loaded from, which selects period tables and effect semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// ProTracker MOD, 4 to 32 channels.
    #[default]
    Mod,
    /// ScreamTracker 3.
    S3m,
    /// FastTracker 2.
    Xm,
    /// Impulse Tracker.
    It,
}

impl Format {
    /// The usual file extension, as in "MOD" or "IT".
    pub fn name(self) -> &'static str {
        match self {
            Format::Mod => "MOD",
            Format::S3m => "S3M",
            Format::Xm => "XM",
            Format::It => "IT",
        }
    }
}

/// How a sample repeats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopType {
    /// Plays once.
    #[default]
    None,
    /// Wraps from the loop end to the loop start.
    Forward,
    /// Alternates direction at each end.
    PingPong,
}

/// One sample's PCM data, mono, nominally -1..1.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    /// Display name.
    pub name: String,
    /// Mono sample frames.
    pub data: Vec<f32>,
    /// First frame of the normal loop.
    pub loop_start: usize,
    /// Exclusive end frame of the normal loop.
    pub loop_end: usize,
    /// Normal loop behaviour.
    pub loop_type: LoopType,
    /// IT sustain loop start, active until key off.
    pub sus_loop_start: usize,
    /// IT sustain loop exclusive end.
    pub sus_loop_end: usize,
    /// IT sustain loop behaviour.
    pub sus_loop: LoopType,
    /// Default volume 0..=64.
    pub volume: usize,
    /// Sample global volume 0..=64 (IT); 64 elsewhere.
    pub global_volume: usize,
    /// Semitones added to every note.
    pub finetune: f32,
    /// Playback rate of the reference note (S3M, IT).
    pub c4_speed: usize,
    /// XM: semitones added to every note.
    pub relative_note: i32,
    /// Default pan, -1..=1.
    pub pan: f32,
    /// Whether `pan` applies when a note starts.
    pub has_pan: bool,
    /// Sample auto-vibrato.
    pub vibrato: AutoVibrato,
}

impl Sample {
    /// True when the normal loop is long enough to play.
    pub(crate) fn loops(&self) -> bool {
        self.loop_type != LoopType::None && self.loop_end > self.loop_start + 1
    }
}

/// Instrument-level vibrato (XM instruments, IT samples).
#[derive(Clone, Copy, Debug, Default)]
pub struct AutoVibrato {
    /// 0 sine, 1 ramp, 2 square, 3 random.
    pub kind: usize,
    /// Ticks to reach full depth.
    pub sweep: usize,
    /// Depth in the source format's units.
    pub depth: usize,
    /// Oscillator increment per tick in the source format's units.
    pub rate: usize,
}

/// A volume, panning or pitch envelope.
#[derive(Clone, Debug, Default)]
pub struct Envelope {
    /// Nodes in increasing tick order.
    pub points: Vec<EnvPoint>,
    /// Apply the envelope during playback.
    pub enabled: bool,
    /// Hold the sustain points until key off.
    pub sustain: bool,
    /// Loop between `loop_start` and `loop_end`.
    pub looping: bool,
    /// First sustain point index.
    pub sustain_start: usize,
    /// Last sustain point index.
    pub sustain_end: usize,
    /// First loop point index.
    pub loop_start: usize,
    /// Last loop point index.
    pub loop_end: usize,
}

/// One envelope node: a tick and a value, 0..=64 for volume and -32..=32 for pan and pitch.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnvPoint {
    /// Time in tracker ticks.
    pub tick: usize,
    /// Value in the range described above.
    pub value: f32,
}

/// Impulse Tracker's new-note action: what happens to the note a new note replaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nna {
    /// Cut the old note.
    #[default]
    Cut,
    /// Let the old note continue unchanged.
    Continue,
    /// Release the old note's envelopes.
    Off,
    /// Fade the old note out.
    Fade,
}

/// Maps notes to samples and shapes them with envelopes.
#[derive(Clone, Debug)]
pub struct Instrument {
    /// Display name.
    pub name: String,
    /// Sample index per note, or -1 for none.
    pub sample_map: [i32; 120],
    /// Note actually played per note (IT); identity elsewhere.
    pub note_map: [i32; 120],
    /// Volume envelope.
    pub vol_env: Envelope,
    /// Panning envelope.
    pub pan_env: Envelope,
    /// Pitch envelope, or filter envelope when `pitch_is_filter` is set.
    pub pitch_env: Envelope,
    /// IT: the pitch envelope drives the filter cutoff.
    pub pitch_is_filter: bool,
    /// Subtracted from a 65536-scale fade each tick after key off.
    pub fadeout: i32,
    /// Action for the previous note when a new note begins.
    pub nna: Nna,
    /// Duplicate check type: 0 off, 1 note, 2 sample, 3 instrument.
    pub dct: u8,
    /// Duplicate check action: 0 cut, 1 note off, 2 fade.
    pub dca: u8,
    /// Global volume 0..=128 (IT); 128 elsewhere.
    pub global_volume: usize,
    /// Default pan.
    pub pan: f32,
    /// Apply `pan` when this instrument starts a note.
    pub has_pan: bool,
    /// IT filter cutoff 0..=127, or -1 for unset.
    pub filter_cutoff: i32,
    /// IT filter resonance 0..=127, or -1 for unset.
    pub filter_resonance: i32,
}

impl Default for Instrument {
    fn default() -> Self {
        let mut note_map = [0; 120];
        for (i, n) in note_map.iter_mut().enumerate() {
            *n = i as i32;
        }
        Self {
            name: String::new(),
            sample_map: [-1; 120],
            note_map,
            vol_env: Envelope::default(),
            pan_env: Envelope::default(),
            pitch_env: Envelope::default(),
            pitch_is_filter: false,
            fadeout: 0,
            nna: Nna::Cut,
            dct: 0,
            dca: 0,
            global_volume: 128,
            pan: 0.0,
            has_pan: false,
            filter_cutoff: -1,
            filter_resonance: -1,
        }
    }
}

/// Rows of cells, one cell per channel.
#[derive(Clone, Debug, Default)]
pub struct Pattern {
    /// Row index, then channel index.
    pub rows: Vec<Vec<Cell>>,
}

/// One channel's entry in a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    /// [`NOTE_NONE`], [`NOTE_OFF`], [`NOTE_CUT`], [`NOTE_FADE`], or a note index (0 is C-0).
    pub note: i32,
    /// Instrument number, 0 for none.
    pub instrument: usize,
    /// Volume-column command.
    pub vol_cmd: VolCmd,
    /// Volume-column argument.
    pub vol_param: usize,
    /// Effect command.
    pub effect: Effect,
    /// Effect argument.
    pub param: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            note: NOTE_NONE,
            instrument: 0,
            vol_cmd: VolCmd::None,
            vol_param: 0,
            effect: Effect::None,
            param: 0,
        }
    }
}

/// A cell with no note.
pub const NOTE_NONE: i32 = -1;
/// Key off: release envelopes, then fade.
pub const NOTE_OFF: i32 = 1000;
/// Stop immediately.
pub const NOTE_CUT: i32 = 1001;
/// Start fading without releasing sustain.
pub const NOTE_FADE: i32 = 1002;

/// A row of `n` empty cells.
pub(crate) fn empty_row(n: usize) -> Vec<Cell> {
    vec![Cell::default(); n]
}

/// Trims trailing NULs and spaces from a fixed-width name field.
pub(crate) fn trim_name(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    s.trim_end_matches(['\0', ' ']).to_string()
}
