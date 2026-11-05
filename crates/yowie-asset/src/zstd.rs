//! zstd frames through the pure-Rust `ruzstd` crate, so the crate builds on
//! every target without a C compiler.

use std::io::Read;

use ruzstd::decoding::StreamingDecoder;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};

/// Compresses `data` into one zstd frame. The encoder implements one
/// speed setting, so there is no level to choose.
pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    compress_to_vec(data, CompressionLevel::Fastest)
}

/// Decompresses one zstd frame, failing when the output would exceed
/// `limit` bytes.
pub(crate) fn decompress(data: &[u8], limit: usize) -> std::io::Result<Vec<u8>> {
    let decoder = StreamingDecoder::new(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut out = Vec::new();
    // Read one byte past the limit so an oversize frame is detected.
    decoder.take(limit as u64 + 1).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "decompressed data exceeds its declared size",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_limit() {
        let data = "abc".repeat(1000).into_bytes();
        let packed = compress(&data);
        assert!(packed.len() < data.len());
        assert_eq!(decompress(&packed, data.len()).unwrap(), data);
        assert!(decompress(&packed, 10).is_err());
        assert!(decompress(b"not zstd", 100).is_err());
    }
}
