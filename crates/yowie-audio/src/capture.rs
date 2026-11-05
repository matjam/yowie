//! Microphone capture, a level meter and bounded recording.

use crate::error::{Error, Result};
use crate::output::{describe, DeviceInfo, find_device};
use crate::sound::Pcm;
use crate::wav::WavWriter;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::io::BufWriter;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Samples per block handed from the device callback to the game.
const BLOCK_SAMPLES: usize = 4096;
/// Blocks in flight between the device and the game: about 1.5 s at 44.1 kHz mono.
const BLOCKS: usize = 16;

/// Lists the input devices of the default host.
pub fn list_inputs() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok()).map(|i| i.to_string());
    Ok(host.input_devices()?.map(|d| describe(&d, default_id.as_deref())).collect())
}

/// Loudness of the most recent audio block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    /// Largest absolute sample, 0 to 1.
    pub peak: f32,
    /// Root-mean-square amplitude, 0 to 1.
    pub rms: f32,
}

#[derive(Default)]
struct Shared {
    peak: AtomicU32,
    rms: AtomicU32,
    dropped: AtomicU64,
}

impl Shared {
    fn update(&self, block: &[f32]) {
        let peak = block.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        let rms = (block.iter().map(|s| s * s).sum::<f32>() / block.len().max(1) as f32).sqrt();
        self.peak.store(peak.to_bits(), Ordering::Relaxed);
        self.rms.store(rms.to_bits(), Ordering::Relaxed);
    }
}

/// An open input device delivering interleaved `f32` samples.
///
/// The device callback copies into pre-allocated blocks and never blocks. If the game
/// does not call [`Capture::read`] often enough, blocks are dropped and counted by
/// [`Capture::dropped_samples`].
pub struct Capture {
    _stream: Option<Stream>,
    rx: Receiver<Vec<f32>>,
    free: Sender<Vec<f32>>,
    shared: Arc<Shared>,
    rate: u32,
    channels: u16,
    pending: Vec<f32>,
    pos: usize,
}

impl Capture {
    /// Opens the default input device.
    pub fn open_default() -> Result<Self> {
        let device = cpal::default_host().default_input_device().ok_or(Error::NoDevice)?;
        Self::build(&device)
    }

    /// Opens an input device chosen from [`list_inputs`] by id or name.
    pub fn open(device: &str) -> Result<Self> {
        Self::build(&find_device(true, device)?)
    }

    fn build(device: &Device) -> Result<Self> {
        let supported = device.default_input_config()?;
        let config = supported.config();
        let (mut c, tx, free_rx) = Self::detached(config.sample_rate, config.channels);
        let shared = c.shared.clone();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => input::<f32>(device, config, tx, free_rx, shared)?,
            SampleFormat::I16 => input::<i16>(device, config, tx, free_rx, shared)?,
            SampleFormat::U16 => input::<u16>(device, config, tx, free_rx, shared)?,
            SampleFormat::I32 => input::<i32>(device, config, tx, free_rx, shared)?,
            SampleFormat::F64 => input::<f64>(device, config, tx, free_rx, shared)?,
            other => return Err(Error::Unsupported(format!("sample format {other}"))),
        };
        stream.play()?;
        c._stream = Some(stream);
        Ok(c)
    }

    /// A capture with no device, fed through the returned sender. Used by tests and by
    /// programs that produce input some other way.
    pub(crate) fn detached(rate: u32, channels: u16) -> (Self, Sender<Vec<f32>>, Receiver<Vec<f32>>) {
        let (tx, rx) = bounded(BLOCKS);
        let (free, free_rx) = bounded(BLOCKS + 2);
        for _ in 0..BLOCKS {
            let _ = free.send(Vec::with_capacity(BLOCK_SAMPLES));
        }
        let c = Self {
            _stream: None,
            rx,
            free,
            shared: Arc::new(Shared::default()),
            rate,
            channels,
            pending: Vec::new(),
            pos: 0,
        };
        (c, tx, free_rx)
    }

    /// Sample rate in Hz.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Interleaved channels per frame.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Copies captured samples into `out` and returns how many it wrote; zero when none are waiting.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let mut written = 0;
        while written < out.len() {
            if self.pos >= self.pending.len() {
                let old = std::mem::take(&mut self.pending);
                if old.capacity() > 0 {
                    let _ = self.free.try_send(old);
                }
                self.pos = 0;
                match self.rx.try_recv() {
                    Ok(b) => self.pending = b,
                    Err(_) => break,
                }
                continue;
            }
            let n = (out.len() - written).min(self.pending.len() - self.pos);
            out[written..written + n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
            self.pos += n;
            written += n;
        }
        written
    }

    /// The loudness of the most recent block the device delivered.
    pub fn level(&self) -> Level {
        Level {
            peak: f32::from_bits(self.shared.peak.load(Ordering::Relaxed)),
            rms: f32::from_bits(self.shared.rms.load(Ordering::Relaxed)),
        }
    }

    /// Samples lost because the game did not read fast enough.
    pub fn dropped_samples(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
}

fn input<T>(
    device: &Device,
    config: StreamConfig,
    tx: Sender<Vec<f32>>,
    free: Receiver<Vec<f32>>,
    shared: Arc<Shared>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            for piece in data.chunks(BLOCK_SAMPLES) {
                let Ok(mut block) = free.try_recv() else {
                    shared.dropped.fetch_add(piece.len() as u64, Ordering::Relaxed);
                    continue;
                };
                block.clear();
                block.extend(piece.iter().map(|s| f32::from_sample(*s)));
                shared.update(&block);
                if let Err(e) = tx.try_send(block) {
                    shared.dropped.fetch_add(piece.len() as u64, Ordering::Relaxed);
                    let _ = e;
                }
            }
        },
        |e| log::warn!("audio capture error: {e}"),
        None,
    )?;
    Ok(stream)
}

enum Sink {
    Memory(Vec<f32>),
    Wav(WavWriter<BufWriter<std::fs::File>>),
}

/// Records from a [`Capture`] into memory or a WAV file, up to a fixed length.
///
/// Call [`Recorder::update`] regularly (once per frame is enough) to move audio from the
/// device into the recording. Recording stops by itself at the limit, so memory use and
/// file size are bounded.
pub struct Recorder {
    capture: Capture,
    sink: Sink,
    limit: usize,
    recorded: usize,
    scratch: Vec<f32>,
}

impl Recorder {
    /// Records into memory for at most `max_seconds`.
    pub fn to_memory(capture: Capture, max_seconds: f32) -> Result<Self> {
        Self::new(capture, max_seconds, Sink::Memory(Vec::new()))
    }

    /// Records into a 16-bit WAV file for at most `max_seconds`.
    pub fn to_wav_file(capture: Capture, path: impl AsRef<Path>, max_seconds: f32) -> Result<Self> {
        let file = BufWriter::new(std::fs::File::create(path)?);
        let w = WavWriter::new(file, capture.rate, capture.channels)?;
        Self::new(capture, max_seconds, Sink::Wav(w))
    }

    fn new(capture: Capture, max_seconds: f32, sink: Sink) -> Result<Self> {
        if !(max_seconds.is_finite() && max_seconds > 0.0) {
            return Err(Error::Invalid("recording length must be positive"));
        }
        let limit = (f64::from(max_seconds) * f64::from(capture.rate)).round() as usize * usize::from(capture.channels);
        if limit == 0 || limit as u64 * 2 > u64::from(u32::MAX) - 36 {
            return Err(Error::Invalid("recording length is outside the supported range"));
        }
        Ok(Self { capture, sink, limit, recorded: 0, scratch: vec![0.0; BLOCK_SAMPLES] })
    }

    /// Moves waiting audio into the recording. Returns the samples added.
    pub fn update(&mut self) -> Result<usize> {
        let mut added = 0;
        while self.recorded < self.limit {
            let want = self.scratch.len().min(self.limit - self.recorded);
            let n = self.capture.read(&mut self.scratch[..want]);
            if n == 0 {
                break;
            }
            match &mut self.sink {
                Sink::Memory(v) => v.extend_from_slice(&self.scratch[..n]),
                Sink::Wav(w) => w.write(&self.scratch[..n])?,
            }
            self.recorded += n;
            added += n;
        }
        Ok(added)
    }

    /// True once the length limit has been reached.
    pub fn is_full(&self) -> bool {
        self.recorded >= self.limit
    }

    /// Seconds recorded so far.
    pub fn seconds(&self) -> f64 {
        self.recorded as f64 / (f64::from(self.capture.rate) * f64::from(self.capture.channels))
    }

    /// The loudness of the most recent audio, for a recording meter.
    pub fn level(&self) -> Level {
        self.capture.level()
    }

    /// Stops recording. A memory recording returns its audio; a file recording is
    /// finished (the header is patched) and returns `None`.
    pub fn finish(mut self) -> Result<Option<Pcm>> {
        self.update()?;
        let (rate, channels) = (self.capture.rate, self.capture.channels);
        match self.sink {
            Sink::Memory(samples) => Ok(Some(Pcm { samples, channels, rate })),
            Sink::Wav(w) => {
                w.finish()?;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(tx: &Sender<Vec<f32>>, free: &Receiver<Vec<f32>>, samples: &[f32]) {
        let mut b = free.try_recv().expect("free block");
        b.clear();
        b.extend_from_slice(samples);
        tx.send(b).expect("send");
    }

    #[test]
    fn read_returns_what_was_fed_in_order() {
        let (mut c, tx, free) = Capture::detached(8000, 1);
        feed(&tx, &free, &[0.1, 0.2, 0.3]);
        feed(&tx, &free, &[0.4, 0.5]);
        let mut out = [0.0; 4];
        assert_eq!(c.read(&mut out), 4);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(c.read(&mut out), 1);
        assert_eq!(out[0], 0.5);
        assert_eq!(c.read(&mut out), 0);
    }

    #[test]
    fn recording_is_bounded_and_returns_pcm() {
        let (c, tx, free) = Capture::detached(1000, 1);
        let mut r = Recorder::to_memory(c, 0.01).expect("recorder");
        feed(&tx, &free, &[0.5; 8]);
        feed(&tx, &free, &[0.25; 8]);
        r.update().expect("update");
        assert!(r.is_full());
        let pcm = r.finish().expect("finish").expect("memory");
        assert_eq!(pcm.samples.len(), 10);
        assert_eq!(pcm.samples[9], 0.25);
        assert_eq!((pcm.rate, pcm.channels), (1000, 1));
    }

    #[test]
    fn recorder_rejects_bad_lengths() {
        for secs in [0.0, -1.0, f32::NAN] {
            let (c, _, _) = Capture::detached(8000, 1);
            assert!(Recorder::to_memory(c, secs).is_err());
        }
    }

    #[test]
    fn level_meter_tracks_peak_and_rms() {
        let (c, ..) = Capture::detached(8000, 1);
        c.shared.update(&[0.5, -0.5, 0.5, -0.5]);
        let l = c.level();
        assert!((l.peak - 0.5).abs() < 1e-6 && (l.rms - 0.5).abs() < 1e-6);
    }

    #[test]
    fn wav_file_recording_writes_a_valid_header() {
        let dir = std::env::temp_dir().join(format!("yowie-audio-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("rec.wav");
        let (c, tx, free) = Capture::detached(8000, 1);
        let mut r = Recorder::to_wav_file(c, &path, 1.0).expect("recorder");
        feed(&tx, &free, &[0.5; 100]);
        r.update().expect("update");
        assert!(r.finish().expect("finish").is_none());
        let pcm = crate::decode::decode_file(&path).expect("decode");
        assert_eq!(pcm.samples.len(), 100);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
