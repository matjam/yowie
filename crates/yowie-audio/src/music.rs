//! Music: a file decoded on a background thread and played as a [`Stream`].

use crate::decode::Decoder;
use crate::error::{Error, Result};
use crate::mixer::Mixer;
use crate::stream::Stream;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Frames per block passed from the decoder thread to the audio thread.
const BLOCK_FRAMES: usize = 2048;
/// Blocks queued ahead of playback: about 0.75 s at 44.1 kHz.
const QUEUE_BLOCKS: usize = 16;
const NO_SEEK: u64 = u64::MAX;

/// Options for opening music.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MusicOptions {
    /// Restart at the loop start when the end is reached.
    pub looping: bool,
    /// Loop region in seconds from the start of the file, `(start, end)`. `None` loops the whole file.
    pub loop_range: Option<(f64, f64)>,
}

struct Block {
    generation: u64,
    data: Vec<f32>,
    last: bool,
}

/// Settings shared between the game thread, the audio thread and the decoder thread.
struct Shared {
    generation: AtomicU64,
    seek_to: AtomicU64,
    looping: AtomicBool,
    loop_start: AtomicU64,
    loop_end: AtomicU64,
    closed: AtomicBool,
    ended: AtomicBool,
    error: Mutex<Option<String>>,
    duration: f64,
}

/// A handle that controls music after it has moved to the mixer.
#[derive(Clone)]
pub struct MusicControl(Arc<Shared>);

impl MusicControl {
    /// Moves playback to `seconds` from the start. Audio already decoded is discarded, so
    /// the new position is heard after a short delay while the decoder catches up.
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() {
            self.0.seek_to.store(seconds.max(0.0).to_bits(), Ordering::Release);
            self.0.generation.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Turns looping on or off from the current decoder position.
    pub fn set_looping(&self, looping: bool) {
        self.0.looping.store(looping, Ordering::Release);
    }

    /// Whether the music loops.
    pub fn looping(&self) -> bool {
        self.0.looping.load(Ordering::Acquire)
    }

    /// Sets the loop region in seconds, `(start, end)`, or `None` for the whole file.
    /// Fails unless `0 <= start < end`.
    pub fn set_loop_range(&self, range: Option<(f64, f64)>) -> Result<()> {
        let (a, b) = match range {
            None => (0.0, 0.0),
            Some((a, b)) if a.is_finite() && b.is_finite() && a >= 0.0 && b > a => (a, b),
            Some(_) => return Err(Error::Invalid("loop range")),
        };
        self.0.loop_start.store(a.to_bits(), Ordering::Release);
        self.0.loop_end.store(b.to_bits(), Ordering::Release);
        Ok(())
    }

    /// Length of the file in seconds, when the container records it.
    pub fn duration(&self) -> Option<f64> {
        (self.0.duration > 0.0).then_some(self.0.duration)
    }

    /// The decoding error that stopped the music, if any.
    pub fn error(&self) -> Option<String> {
        self.0.error.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// A music file streamed from disk or memory.
///
/// A background thread decodes and resamples to the mixer's rate; the audio thread only
/// copies decoded blocks. Pass it to [`Mixer::play_stream`] and keep a [`MusicControl`]
/// from [`Music::control`] to seek or change looping.
pub struct Music {
    shared: Arc<Shared>,
    blocks: Receiver<Block>,
    recycle: Sender<Vec<f32>>,
    cur: Vec<f32>,
    pos: usize,
    cur_gen: u64,
    last: bool,
}

impl Music {
    /// Opens a WAV, Ogg Vorbis, MP3 or FLAC file for streaming.
    pub fn open_file(mixer: &Mixer, path: impl AsRef<Path>, opts: MusicOptions) -> Result<Self> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        Self::start(mixer.rate(), Decoder::open(Box::new(file), ext.as_deref())?, opts)
    }

    /// Opens music held in memory.
    pub fn open_bytes(mixer: &Mixer, data: Vec<u8>, opts: MusicOptions) -> Result<Self> {
        Self::start(mixer.rate(), Decoder::open(Box::new(Cursor::new(data)), None)?, opts)
    }

    /// A handle that controls this music after it has moved to the mixer.
    pub fn control(&self) -> MusicControl {
        MusicControl(self.shared.clone())
    }

    fn start(rate: u32, dec: Decoder, opts: MusicOptions) -> Result<Self> {
        let duration = dec.length.map_or(0.0, |n| n as f64 / f64::from(dec.rate.max(1)));
        let (a, b) = opts.loop_range.unwrap_or((0.0, 0.0));
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            seek_to: AtomicU64::new(NO_SEEK),
            looping: AtomicBool::new(opts.looping),
            loop_start: AtomicU64::new(a.to_bits()),
            loop_end: AtomicU64::new(b.to_bits()),
            closed: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            error: Mutex::new(None),
            duration,
        });
        let (tx, blocks) = bounded(QUEUE_BLOCKS);
        let (recycle, recycle_rx) = bounded(QUEUE_BLOCKS + 2);
        let worker = Worker::new(dec, rate, shared.clone(), tx, recycle_rx);
        std::thread::Builder::new().name("yowie-music".into()).spawn(move || worker.run())?;
        Ok(Self { shared, blocks, recycle, cur: Vec::new(), pos: 0, cur_gen: 0, last: false })
    }

    fn drop_current(&mut self) {
        let v = std::mem::take(&mut self.cur);
        if v.capacity() > 0 {
            let _ = self.recycle.try_send(v);
        }
        self.pos = 0;
    }
}

impl Drop for Music {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}

impl Stream for Music {
    fn read(&mut self, out: &mut [f32]) -> usize {
        let want = out.len();
        let mut written = 0;
        while written < want {
            let generation = self.shared.generation.load(Ordering::Acquire);
            if self.cur_gen != generation {
                self.drop_current();
                self.cur_gen = generation;
                self.last = false;
            }
            if self.pos >= self.cur.len() {
                if self.last {
                    return written / 2;
                }
                self.drop_current();
                match self.blocks.try_recv() {
                    Ok(b) if b.generation == generation => {
                        self.cur = b.data;
                        self.last = b.last;
                    }
                    Ok(b) => {
                        let _ = self.recycle.try_send(b.data);
                    }
                    Err(_) => break,
                }
                continue;
            }
            let n = (want - written).min(self.cur.len() - self.pos);
            out[written..written + n].copy_from_slice(&self.cur[self.pos..self.pos + n]);
            self.pos += n;
            written += n;
        }
        // The decoder is behind: play silence and keep going.
        out[written..].fill(0.0);
        want / 2
    }

    fn seek(&mut self, seconds: f64) -> bool {
        self.control().seek(seconds);
        true
    }
}

/// A streaming linear resampler that carries its phase between calls.
struct Resampler {
    step: f64,
    pos: f64,
    prev: [f32; 2],
    have: bool,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        Self { step: f64::from(from) / f64::from(to), pos: 0.0, prev: [0.0; 2], have: false }
    }

    fn reset(&mut self) {
        self.pos = 0.0;
        self.have = false;
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        for f in input.chunks_exact(2) {
            if !self.have {
                self.prev = [f[0], f[1]];
                self.have = true;
                self.pos = 0.0;
                continue;
            }
            while self.pos < 1.0 {
                let t = self.pos as f32;
                out.push(self.prev[0] + (f[0] - self.prev[0]) * t);
                out.push(self.prev[1] + (f[1] - self.prev[1]) * t);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = [f[0], f[1]];
        }
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        if self.have && self.step != 1.0 {
            out.extend_from_slice(&self.prev);
        }
    }
}

struct Worker {
    dec: Decoder,
    shared: Arc<Shared>,
    tx: Sender<Block>,
    recycle: Receiver<Vec<f32>>,
    rs: Resampler,
    generation: u64,
    src_pos: u64,
    out: Vec<f32>,
    stereo: Vec<f32>,
    packet: Vec<f32>,
}

enum Flow {
    Continue,
    Restarted,
    Stop,
}

impl Worker {
    fn new(dec: Decoder, rate: u32, shared: Arc<Shared>, tx: Sender<Block>, recycle: Receiver<Vec<f32>>) -> Self {
        let rs = Resampler::new(dec.rate, rate);
        Self { dec, shared, tx, recycle, rs, generation: 0, src_pos: 0, out: Vec::new(), stereo: Vec::new(), packet: Vec::new() }
    }

    fn fail(&self, e: &Error) {
        *self.shared.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(e.to_string());
        self.shared.ended.store(true, Ordering::Release);
    }

    fn loop_frames(&self) -> (u64, u64) {
        let rate = f64::from(self.dec.rate);
        let a = f64::from_bits(self.shared.loop_start.load(Ordering::Acquire));
        let b = f64::from_bits(self.shared.loop_end.load(Ordering::Acquire));
        ((a * rate) as u64, (b * rate) as u64)
    }

    /// Applies a pending seek. Returns true when one was applied.
    fn check_seek(&mut self) -> bool {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation == self.generation {
            return false;
        }
        self.generation = generation;
        self.out.clear();
        self.rs.reset();
        self.shared.ended.store(false, Ordering::Release);
        let bits = self.shared.seek_to.swap(NO_SEEK, Ordering::AcqRel);
        if bits != NO_SEEK {
            let s = f64::from_bits(bits);
            match self.dec.seek(s) {
                Ok(()) => self.src_pos = (s * f64::from(self.dec.rate)) as u64,
                Err(e) => log::warn!("music seek failed: {e}"),
            }
        }
        true
    }

    fn run(mut self) {
        loop {
            if self.shared.closed.load(Ordering::Acquire) {
                return;
            }
            self.check_seek();
            if self.shared.ended.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            match self.step() {
                Flow::Stop => return,
                Flow::Continue | Flow::Restarted => {}
            }
        }
    }

    /// Decodes one packet, or handles the end of the file.
    fn step(&mut self) -> Flow {
        self.packet.clear();
        let more = match self.dec.read_more(&mut self.packet) {
            Ok(m) => m,
            Err(e) => {
                self.fail(&e);
                self.flush(true);
                return Flow::Stop;
            }
        };
        if more {
            let (ls, le) = self.loop_frames();
            let looping = self.shared.looping.load(Ordering::Acquire);
            self.to_stereo();
            let mut frames = self.stereo.len() / 2;
            let mut hit_end = false;
            if looping && le > ls && self.src_pos + frames as u64 >= le {
                frames = le.saturating_sub(self.src_pos) as usize;
                self.stereo.truncate(frames * 2);
                hit_end = true;
            }
            self.src_pos += frames as u64;
            self.rs.push(&self.stereo, &mut self.out);
            if !self.drain(false) {
                return Flow::Continue;
            }
            if hit_end {
                return self.rewind();
            }
            return Flow::Continue;
        }
        if self.shared.looping.load(Ordering::Acquire) {
            return self.rewind();
        }
        self.rs.finish(&mut self.out);
        self.drain(true);
        self.shared.ended.store(true, Ordering::Release);
        Flow::Continue
    }

    fn rewind(&mut self) -> Flow {
        let (ls, _) = self.loop_frames();
        let start = ls as f64 / f64::from(self.dec.rate);
        match self.dec.seek(start) {
            Ok(()) => {
                self.src_pos = ls;
                Flow::Restarted
            }
            Err(e) => {
                self.fail(&e);
                self.flush(true);
                Flow::Stop
            }
        }
    }

    fn to_stereo(&mut self) {
        self.stereo.clear();
        if self.dec.channels == 1 {
            self.stereo.extend(self.packet.iter().flat_map(|s| [*s, *s]));
        } else {
            self.stereo.extend_from_slice(&self.packet);
        }
    }

    fn flush(&mut self, last: bool) {
        self.drain(last);
        self.shared.ended.store(true, Ordering::Release);
    }

    /// Sends full blocks (and the remainder when `last`). Returns false when a seek
    /// arrived while waiting for room, in which case the pending audio is discarded.
    fn drain(&mut self, last: bool) -> bool {
        let block = BLOCK_FRAMES * 2;
        while self.out.len() >= block || last {
            let n = self.out.len().min(block);
            let mut data = self.recycle.try_recv().unwrap_or_default();
            data.clear();
            data.extend_from_slice(&self.out[..n]);
            self.out.drain(..n);
            let is_last = last && self.out.is_empty();
            let mut b = Block { generation: self.generation, data, last: is_last };
            loop {
                if self.shared.generation.load(Ordering::Acquire) != self.generation || self.shared.closed.load(Ordering::Acquire) {
                    return false;
                }
                match self.tx.try_send(b) {
                    Ok(()) => break,
                    Err(TrySendError::Full(back)) => {
                        b = back;
                        // Wait for the audio thread to consume a block.
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(TrySendError::Disconnected(_)) => return false,
                }
            }
            if is_last {
                break;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::wav_bytes;
    use crate::{Pcm, PlayOptions};

    fn music(rate: u32, secs: f64, opts: MusicOptions) -> (Mixer, crate::Renderer, Music) {
        let (m, r) = Mixer::offline(48_000).expect("mixer");
        let data = wav_bytes(&Pcm::sine(220.0, secs, rate));
        let music = Music::open_bytes(&m, data, opts).expect("music");
        (m, r, music)
    }

    fn render_until(r: &mut crate::Renderer, out: &mut Vec<f32>, frames: usize) {
        // The decoder thread runs concurrently, so render in small steps with pauses.
        while out.len() < frames * 2 {
            out.extend(r.render_frames(1024));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn plays_a_file_to_its_end() {
        let (m, mut r, music) = music(22_050, 0.5, MusicOptions::default());
        let v = m.play_stream(music, PlayOptions::default());
        let mut out = Vec::new();
        let mut spins = 0;
        while !v.is_finished() && spins < 2000 {
            out.extend(r.render_frames(1024));
            std::thread::sleep(Duration::from_millis(1));
            spins += 1;
        }
        assert!(v.is_finished());
        let peak = out.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.3, "peak {peak}");
        let frames = out.len() / 2;
        assert!(frames >= 24_000 && frames < 24_000 + 8192 + 4096, "{frames} frames for half a second");
    }

    #[test]
    fn loops_until_stopped() {
        let (m, mut r, music) = music(48_000, 0.2, MusicOptions { looping: true, ..Default::default() });
        let v = m.play_stream(music, PlayOptions::default());
        let mut out = Vec::new();
        render_until(&mut r, &mut out, 48_000);
        assert!(v.is_playing());
        let tail = &out[out.len() - 4000..];
        assert!(tail.iter().fold(0.0f32, |a, b| a.max(b.abs())) > 0.2, "still audible after several loops");
    }

    #[test]
    fn seek_discards_stale_audio() {
        let (m, mut r, music) = music(48_000, 2.0, MusicOptions::default());
        let ctl = music.control();
        let v = m.play_stream(music, PlayOptions::default());
        let mut out = Vec::new();
        render_until(&mut r, &mut out, 4096);
        v.seek(1.5);
        out.clear();
        render_until(&mut r, &mut out, 48_000);
        let _ = ctl.duration();
        assert!(v.is_finished(), "only half a second remained after seeking to 1.5 s");
    }

    #[test]
    fn resampler_changes_length_by_the_ratio() {
        for (from, to) in [(22_050u32, 44_100u32), (48_000, 44_100), (44_100, 44_100)] {
            let mut rs = Resampler::new(from, to);
            let input = vec![0.25; from as usize * 2];
            let mut out = Vec::new();
            rs.push(&input, &mut out);
            rs.finish(&mut out);
            let frames = out.len() / 2;
            assert!(frames.abs_diff(to as usize) <= 3, "{from}->{to}: {frames}");
        }
    }

    #[test]
    fn loop_range_is_validated() {
        let (_m, _r, music) = music(48_000, 0.1, MusicOptions::default());
        let c = music.control();
        assert!(c.set_loop_range(Some((1.0, 0.5))).is_err());
        assert!(c.set_loop_range(Some((-1.0, 0.5))).is_err());
        assert!(c.set_loop_range(Some((0.0, 0.05))).is_ok());
        assert!(c.set_loop_range(None).is_ok());
    }
}
