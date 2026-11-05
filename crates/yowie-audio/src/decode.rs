//! Decoding of WAV, Ogg Vorbis, MP3 and FLAC through Symphonia.

use crate::error::{Error, Result};
use crate::sound::Pcm;
use std::io::Cursor;
use std::path::Path;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

fn err(e: SymError) -> Error {
    Error::Decode(e.to_string())
}

/// Decodes a complete WAV, Ogg Vorbis, MP3 or FLAC file held in memory.
///
/// The format is detected from the data. The result is mono or stereo; sources with
/// more channels are mixed down to stereo. Pass the result to [`Mixer::sound`](crate::Mixer::sound).
pub fn decode(data: Vec<u8>) -> Result<Pcm> {
    decode_source(Box::new(Cursor::new(data)), None)
}

/// Decodes a WAV, Ogg Vorbis, MP3 or FLAC file from disk. See [`decode`].
pub fn decode_file(path: impl AsRef<Path>) -> Result<Pcm> {
    let path = path.as_ref();
    let file = std::fs::File::open(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    decode_source(Box::new(file), ext.as_deref())
}

fn decode_source(src: Box<dyn MediaSource>, ext: Option<&str>) -> Result<Pcm> {
    let mut dec = Decoder::open(src, ext)?;
    let mut samples = Vec::new();
    while dec.read_more(&mut samples)? {}
    Ok(Pcm { samples, channels: dec.channels, rate: dec.rate })
}

/// A streaming decoder that yields mono or stereo interleaved `f32`.
pub(crate) struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    pub rate: u32,
    pub channels: u16,
    /// Length in frames at the source rate, when the container says.
    pub length: Option<u64>,
    first: Vec<f32>,
    tmp: Vec<f32>,
}

impl Decoder {
    pub(crate) fn open(src: Box<dyn MediaSource>, ext: Option<&str>) -> Result<Self> {
        let mss = MediaSourceStream::new(src, Default::default());
        let mut hint = Hint::new();
        if let Some(e) = ext {
            hint.with_extension(e);
        }
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(err)?;
        let track = format.default_track(TrackType::Audio).ok_or_else(|| Error::Decode("no audio track".into()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| Error::Decode("no audio parameters".into()))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(err)?;
        let (track_id, length) = (track.id, track.num_frames);
        let mut d = Self {
            format,
            decoder,
            track_id,
            rate: 0,
            channels: 2,
            length,
            first: Vec::new(),
            tmp: Vec::new(),
        };
        // Decode one buffer so the rate and channel count are known before any playback.
        let mut first = Vec::new();
        if !d.decode_next(&mut first)? {
            return Err(Error::Decode("the file holds no audio".into()));
        }
        d.first = first;
        Ok(d)
    }

    /// Appends the next decoded packet to `out`. Returns false at the end of the stream.
    pub(crate) fn read_more(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        if !self.first.is_empty() {
            out.append(&mut self.first);
            return Ok(true);
        }
        self.decode_next(out)
    }

    fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(SymError::ResetRequired) => return Ok(false),
                Err(e) => return Err(err(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    if buf.frames() == 0 {
                        continue;
                    }
                    let spec = buf.spec();
                    let n = spec.channels().count().max(1);
                    self.rate = spec.rate();
                    self.channels = if n == 1 { 1 } else { 2 };
                    self.tmp.clear();
                    buf.copy_to_vec_interleaved::<f32>(&mut self.tmp);
                    fold_channels(&self.tmp, n, out);
                    return Ok(true);
                }
                Err(SymError::DecodeError(msg)) => log::debug!("skipping a bad packet: {msg}"),
                Err(e) => return Err(err(e)),
            }
        }
    }

    /// Moves to `seconds` from the start and discards buffered audio.
    pub(crate) fn seek(&mut self, seconds: f64) -> Result<()> {
        let time = Time::try_from_secs_f64(seconds.max(0.0)).unwrap_or(Time::ZERO);
        self.format
            .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.track_id) })
            .map_err(err)?;
        self.decoder.reset();
        self.first.clear();
        Ok(())
    }
}

/// Appends `n`-channel interleaved samples to `out` as mono (n = 1) or stereo.
fn fold_channels(input: &[f32], n: usize, out: &mut Vec<f32>) {
    match n {
        1 | 2 => out.extend_from_slice(input),
        _ => {
            for frame in input.chunks_exact(n) {
                let (mut l, mut r, mut nl, mut nr) = (0.0, 0.0, 0.0, 0.0);
                for (i, s) in frame.iter().enumerate() {
                    if i % 2 == 0 {
                        l += s;
                        nl += 1.0;
                    } else {
                        r += s;
                        nr += 1.0;
                    }
                }
                out.push(l / f32::max(nl, 1.0));
                out.push(r / f32::max(nr, 1.0));
            }
        }
    }
}

/// Builds a 16-bit PCM WAV file in memory, for tests.
#[cfg(test)]
pub(crate) fn wav_bytes(pcm: &Pcm) -> Vec<u8> {
    crate::wav::encode_wav(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(channels: u16, rate: u32) -> Pcm {
        let mono = Pcm::sine(440.0, 0.25, rate);
        let samples = if channels == 2 { mono.samples.iter().flat_map(|s| [*s, s * 0.5]).collect() } else { mono.samples };
        Pcm { samples, channels, rate }
    }

    #[test]
    fn generated_wav_round_trips() {
        for (channels, rate) in [(1, 22_050), (2, 44_100), (1, 8_000)] {
            let src = tone(channels, rate);
            let out = decode(wav_bytes(&src)).expect("decode");
            assert_eq!((out.channels, out.rate), (channels, rate));
            assert_eq!(out.samples.len(), src.samples.len());
            let worst = out.samples.iter().zip(&src.samples).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(worst < 1.0 / 16_000.0, "{channels} ch at {rate}: error {worst}");
        }
    }

    #[test]
    fn garbage_is_rejected() {
        for data in [vec![], vec![1, 2, 3, 4, 5, 6, 7, 8], b"RIFFxxxxWAVEjunk".to_vec()] {
            assert!(decode(data).is_err());
        }
    }

    #[test]
    fn many_channels_fold_to_stereo() {
        let mut out = Vec::new();
        fold_channels(&[1.0, 0.0, 1.0, 0.0, 1.0, 0.0], 6, &mut out);
        assert_eq!(out, vec![1.0, 0.0]);
    }

    #[test]
    fn seek_restarts_decoding() {
        let src = tone(1, 22_050);
        let mut d = Decoder::open(Box::new(Cursor::new(wav_bytes(&src))), Some("wav")).expect("open");
        let mut a = Vec::new();
        while d.read_more(&mut a).expect("read") {}
        d.seek(0.0).expect("seek");
        let mut b = Vec::new();
        while d.read_more(&mut b).expect("read") {}
        assert_eq!(a.len(), b.len());
    }
}
