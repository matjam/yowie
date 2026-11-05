//! WAV writing.

use crate::error::{Error, Result};
use crate::sound::Pcm;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

fn header(rate: u32, channels: u16, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    let block = u32::from(channels) * 2;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * block).to_le_bytes());
    h[32..34].copy_from_slice(&(block as u16).to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Converts a sample to 16-bit with clipping; NaN becomes silence.
pub(crate) fn to_i16(s: f32) -> i16 {
    if s.is_nan() {
        0
    } else {
        (s.clamp(-1.0, 1.0) * 32767.0).round() as i16
    }
}

/// Encodes PCM as a 16-bit WAV file in memory. Clips samples outside `-1..=1`.
pub(crate) fn encode_wav(pcm: &Pcm) -> Vec<u8> {
    let bytes = (pcm.samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.samples.len() * 2);
    out.extend_from_slice(&header(pcm.rate, pcm.channels, bytes));
    for s in &pcm.samples {
        out.extend_from_slice(&to_i16(*s).to_le_bytes());
    }
    out
}

impl Pcm {
    /// Writes the audio as a 16-bit PCM WAV file. Samples outside `-1..=1` are clipped.
    pub fn write_wav(&self, w: &mut impl Write) -> Result<()> {
        if self.channels == 0 || self.rate == 0 {
            return Err(Error::Invalid("PCM needs channels and a rate"));
        }
        let bytes = self.samples.len() as u64 * 2;
        if bytes > u64::from(u32::MAX) - 36 {
            return Err(Error::Invalid("audio is too long for a WAV file"));
        }
        w.write_all(&encode_wav(self))?;
        Ok(())
    }

    /// Saves the audio to a 16-bit PCM WAV file.
    pub fn save_wav(&self, path: impl AsRef<Path>) -> Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        self.write_wav(&mut f)?;
        f.flush()?;
        Ok(())
    }
}

/// Streams 16-bit PCM into a seekable writer, patching the header when finished.
pub(crate) struct WavWriter<W: Write + Seek> {
    w: W,
    rate: u32,
    channels: u16,
    bytes: u64,
}

impl<W: Write + Seek> WavWriter<W> {
    pub(crate) fn new(mut w: W, rate: u32, channels: u16) -> Result<Self> {
        w.write_all(&header(rate, channels, 0))?;
        Ok(Self {
            w,
            rate,
            channels,
            bytes: 0,
        })
    }

    pub(crate) fn write(&mut self, samples: &[f32]) -> Result<()> {
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            buf.extend_from_slice(&to_i16(*s).to_le_bytes());
        }
        self.bytes += buf.len() as u64;
        if self.bytes > u64::from(u32::MAX) - 36 {
            return Err(Error::Invalid("audio is too long for a WAV file"));
        }
        self.w.write_all(&buf)?;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<W> {
        self.w.seek(SeekFrom::Start(0))?;
        self.w
            .write_all(&header(self.rate, self.channels, self.bytes as u32))?;
        self.w.seek(SeekFrom::End(0))?;
        self.w.flush()?;
        Ok(self.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn header_fields() {
        let pcm = Pcm {
            samples: vec![0.0, 1.0, -1.0, 0.5],
            channels: 2,
            rate: 44_100,
        };
        let bytes = encode_wav(&pcm);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            36 + 8
        );
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            44_100
        );
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), 32767);
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), -32767);
    }

    #[test]
    fn clipping_and_nan() {
        for (input, want) in [(2.0, 32767), (-2.0, -32767), (f32::NAN, 0), (0.0, 0)] {
            assert_eq!(to_i16(input), want);
        }
    }

    #[test]
    fn streaming_writer_patches_header() {
        let mut w = WavWriter::new(Cursor::new(Vec::new()), 8000, 1).expect("writer");
        w.write(&[0.5; 100]).expect("write");
        let bytes = w.finish().expect("finish").into_inner();
        assert_eq!(bytes.len(), 44 + 200);
        assert_eq!(
            u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
            200
        );
    }
}
