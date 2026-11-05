//! Decoding tile layer cells: CSV, base64, and zlib, gzip or zstd.

use std::io::Read;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::error::{Error, Result};
use crate::gid::Gid;

/// Most bytes a layer's compressed data may expand to (sixteen million
/// cells). It bounds a file that claims far more than it holds.
const MAX_LAYER_BYTES: u64 = 64 << 20;

/// Parses comma-separated cell ids.
pub(crate) fn decode_csv(text: &str) -> Result<Vec<Gid>> {
    let text = text.trim();
    let text = text.strip_suffix(',').unwrap_or(text);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|f| {
            f.trim()
                .parse::<u32>()
                .map(Gid)
                .map_err(|_| Error::Data(format!("bad CSV cell {:?}", f.trim())))
        })
        .collect()
}

/// Decodes base64 text holding little-endian `u32` cells, optionally
/// compressed with `compression` (`""`, `"zlib"`, `"gzip"` or `"zstd"`).
pub(crate) fn decode_base64(text: &str, compression: &str) -> Result<Vec<Gid>> {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let raw = STANDARD
        .decode(clean)
        .map_err(|e| Error::Data(format!("base64: {e}")))?;
    let bytes = match compression {
        "" => raw,
        "zlib" => inflate(flate2::read::ZlibDecoder::new(&raw[..]), "zlib")?,
        "gzip" => inflate(flate2::read::GzDecoder::new(&raw[..]), "gzip")?,
        "zstd" => {
            let dec = zstd::stream::read::Decoder::new(&raw[..])
                .map_err(|e| Error::Data(format!("zstd: {e}")))?;
            inflate(dec, "zstd")?
        }
        other => return Err(Error::Unsupported(format!("compression {other:?}"))),
    };
    if bytes.len() % 4 != 0 {
        return Err(Error::Data(format!(
            "{} bytes is not whole cells",
            bytes.len()
        )));
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| Gid(u32::from_le_bytes(*c)))
        .collect())
}

fn inflate(reader: impl Read, name: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader
        .take(MAX_LAYER_BYTES + 1)
        .read_to_end(&mut out)
        .map_err(|e| Error::Data(format!("{name}: {e}")))?;
    if out.len() as u64 > MAX_LAYER_BYTES {
        return Err(Error::Data(format!(
            "{name}: data expands past the size limit"
        )));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    /// Encodes cells as Tiled does, for tests: little-endian bytes,
    /// compressed, then base64.
    pub(crate) fn encode(cells: &[u32], compression: &str) -> String {
        let raw: Vec<u8> = cells.iter().flat_map(|c| c.to_le_bytes()).collect();
        let packed = match compression {
            "zlib" => {
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&raw).unwrap();
                e.finish().unwrap()
            }
            "gzip" => {
                let mut e =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&raw).unwrap();
                e.finish().unwrap()
            }
            "zstd" => zstd::bulk::compress(&raw, 3).unwrap(),
            _ => raw,
        };
        STANDARD.encode(packed)
    }

    #[test]
    fn every_base64_compression_round_trips() {
        let cells = [1, 2, 0x8000_0003, 0, 0x1000_0007, 99];
        for compression in ["", "zlib", "gzip", "zstd"] {
            let text = encode(&cells, compression);
            let got: Vec<u32> = decode_base64(&format!("\n  {text}\n"), compression)
                .unwrap()
                .iter()
                .map(|g| g.0)
                .collect();
            assert_eq!(got, cells, "{compression:?}");
        }
    }

    #[test]
    fn csv_cases() {
        assert_eq!(decode_csv(" 1, 2,\n3 ").unwrap(), [Gid(1), Gid(2), Gid(3)]);
        assert!(decode_csv("").unwrap().is_empty());
        assert!(decode_csv("1,x").is_err());
        assert_eq!(
            decode_csv("1,2,").unwrap(),
            [Gid(1), Gid(2)],
            "one trailing comma is allowed"
        );
        assert!(decode_csv("1,,2").is_err(), "an empty cell is an error");
    }

    #[test]
    fn bad_data_is_rejected() {
        assert!(matches!(decode_base64("!!!", ""), Err(Error::Data(_))));
        assert!(matches!(
            decode_base64(&encode(&[1], ""), "lz4"),
            Err(Error::Unsupported(_))
        ));
        assert!(
            matches!(decode_base64("AQID", ""), Err(Error::Data(_))),
            "3 bytes"
        );
        for c in ["zlib", "gzip", "zstd"] {
            assert!(
                matches!(decode_base64("AQIDBAUG", c), Err(Error::Data(_))),
                "{c}"
            );
        }
    }
}
