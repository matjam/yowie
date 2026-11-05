//! Length-prefixed framing for TCP: a 4-byte little-endian length followed
//! by the postcard-encoded message.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// Bytes in the length prefix of a frame.
pub const PREFIX: usize = 4;

/// Encodes `msg` as a complete frame (prefix included), refusing payloads
/// over `max` bytes.
pub fn encode_frame<T: Serialize>(msg: &T, max: usize) -> Result<Vec<u8>> {
    let mut frame = postcard::to_extend(msg, vec![0u8; PREFIX])?;
    let size = frame.len() - PREFIX;
    if size > max {
        return Err(Error::TooLarge { size, max });
    }
    // `size <= max` and `max` is bounded by u32::MAX in `FrameReader`, but
    // a caller-supplied `max` above u32::MAX is clamped here.
    let len = u32::try_from(size).map_err(|_| Error::TooLarge {
        size,
        max: u32::MAX as usize,
    })?;
    frame[..PREFIX].copy_from_slice(&len.to_le_bytes());
    Ok(frame)
}

/// Decodes one postcard payload.
pub fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T> {
    Ok(postcard::from_bytes(payload)?)
}

/// Incremental parser that turns a byte stream into frames.
#[derive(Debug)]
pub struct FrameReader {
    buf: Vec<u8>,
    max: usize,
}

impl FrameReader {
    /// Creates a reader that rejects payloads larger than `max` bytes.
    pub fn new(max: usize) -> Self {
        Self {
            buf: Vec::new(),
            max,
        }
    }

    /// Appends bytes read from the stream.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Returns the next complete payload, `Ok(None)` when more bytes are
    /// needed, or an error when the announced length exceeds the limit.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        if self.buf.len() < PREFIX {
            return Ok(None);
        }
        let mut p = [0u8; PREFIX];
        p.copy_from_slice(&self.buf[..PREFIX]);
        let size = u32::from_le_bytes(p) as usize;
        if size > self.max {
            return Err(Error::TooLarge {
                size,
                max: self.max,
            });
        }
        if self.buf.len() < PREFIX + size {
            return Ok(None);
        }
        let payload = self.buf[PREFIX..PREFIX + size].to_vec();
        self.buf.drain(..PREFIX + size);
        Ok(Some(payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_split_across_reads() {
        let a = encode_frame(&"hello".to_string(), 1024).unwrap();
        let b = encode_frame(&42u32, 1024).unwrap();
        let mut all = a.clone();
        all.extend_from_slice(&b);
        for chunk in [1usize, 2, 3, 7, all.len()] {
            let mut r = FrameReader::new(1024);
            let mut got = Vec::new();
            for piece in all.chunks(chunk) {
                r.push(piece);
                while let Some(f) = r.next_frame().unwrap() {
                    got.push(f);
                }
            }
            assert_eq!(got.len(), 2, "chunk {chunk}");
            assert_eq!(decode_payload::<String>(&got[0]).unwrap(), "hello");
            assert_eq!(decode_payload::<u32>(&got[1]).unwrap(), 42);
        }
    }

    #[test]
    fn oversize_frames_are_rejected() {
        let big = vec![7u8; 100];
        assert!(matches!(
            encode_frame(&big, 50),
            Err(Error::TooLarge { max: 50, .. })
        ));
        let frame = encode_frame(&big, 1000).unwrap();
        let mut r = FrameReader::new(50);
        r.push(&frame);
        assert!(matches!(r.next_frame(), Err(Error::TooLarge { .. })));
    }
}
