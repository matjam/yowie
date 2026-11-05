//! zstd frames for pack entries.

/// Compresses `data` into one zstd frame at `level` (1 to 22; higher is
/// smaller and slower). Returns `None` if the encoder fails.
pub(crate) fn compress(data: &[u8], level: i32) -> Option<Vec<u8>> {
    zstd::bulk::compress(data, level).ok()
}

/// Decompresses one zstd frame, failing when the output would exceed
/// `limit` bytes.
pub(crate) fn decompress(data: &[u8], limit: usize) -> std::io::Result<Vec<u8>> {
    zstd::bulk::decompress(data, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_limit() {
        let data = "abc".repeat(1000).into_bytes();
        for level in [1, 3, 19] {
            let packed = compress(&data, level).unwrap();
            assert!(packed.len() < data.len());
            assert_eq!(decompress(&packed, data.len()).unwrap(), data);
        }
        let packed = compress(&data, 3).unwrap();
        assert!(decompress(&packed, 10).is_err());
        assert!(decompress(b"not zstd", 100).is_err());
    }
}
