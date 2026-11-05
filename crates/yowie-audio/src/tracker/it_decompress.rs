//! Impulse Tracker sample decompression.

use crate::error::{Error, Result};

/// Expands IT 2.14 sample compression: blocks of variable-width delta-coded values with
/// in-band width changes. Version 2.15 files integrate the deltas twice.
pub(crate) fn decompress(src: &[u8], count: usize, sixteen: bool, it215: bool) -> Result<Vec<f32>> {
    // The data cannot hold more samples than it has bits, so a header claiming billions
    // of samples does not reserve memory for them.
    let mut out: Vec<f32> = Vec::with_capacity(count.min(src.len().saturating_mul(8)));
    let (block_samples, width_max, border_max): (usize, i32, i64) = if sixteen {
        (0x4000, 17, 0xFFFF)
    } else {
        (0x8000, 9, 0xFF)
    };
    let mut off = 0;
    while out.len() < count {
        let hdr = src
            .get(off..off + 2)
            .ok_or_else(|| Error::Module("compressed sample is truncated".into()))?;
        let block_len = usize::from(hdr[0]) | usize::from(hdr[1]) << 8;
        off += 2;
        let end = (off + block_len).min(src.len());
        let mut r = BitReader {
            data: &src[off..end],
            pos: 0,
        };
        off = end;
        let mut width = width_max;
        let (mut mem1, mut mem2) = (0i32, 0i32);
        let want = block_samples.min(count - out.len());
        let mut n = 0;
        while n < want {
            let Some(v) = r.read(width) else { break };
            let v = i64::from(v);
            if width <= 6 {
                if v == 1 << (width - 1) {
                    let Some(nw) = r.read(3) else { break };
                    width = adjust(nw as i32 + 1, width);
                    continue;
                }
            } else if width < width_max {
                let border = (border_max >> (width_max - width)) - i64::from(width_max / 2);
                if v > border && v < border + i64::from(width_max) {
                    width = adjust((v - border) as i32, width);
                    continue;
                }
            } else if v & (1 << (width_max - 1)) != 0 {
                width = ((v + 1) & 0xFF) as i32;
                if width < 1 || width > width_max {
                    return Err(Error::Module(format!(
                        "compressed sample sets a width of {width} bits"
                    )));
                }
                continue;
            }
            let shift = 32 - width;
            let sv = ((v as u32) << shift) as i32 >> shift;
            mem1 = mem1.wrapping_add(sv);
            mem2 = mem2.wrapping_add(mem1);
            let val = if it215 { mem2 } else { mem1 };
            out.push(if sixteen {
                f32::from(val as i16) / 32768.0
            } else {
                f32::from(val as i8) / 128.0
            });
            n += 1;
        }
        if r.exhausted() && out.len() < count && block_len == 0 {
            return Err(Error::Module("compressed sample ended early".into()));
        }
    }
    Ok(out)
}

/// A new width at or above the current one is stored one less than meant.
fn adjust(new: i32, width: i32) -> i32 {
    if new >= width { new + 1 } else { new }
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn read(&mut self, bits: i32) -> Option<u32> {
        let bits = usize::try_from(bits).ok()?;
        if self.pos + bits > self.data.len() * 8 {
            return None;
        }
        let mut v = 0u32;
        for i in 0..bits {
            let bit = (self.data[self.pos >> 3] >> (self.pos & 7)) & 1;
            v |= u32::from(bit) << i;
            self.pos += 1;
        }
        Some(v)
    }

    fn exhausted(&self) -> bool {
        self.pos >= self.data.len() * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs fixed-width values into a one-block IT stream with no width changes.
    fn pack(values: &[i32], width: u32) -> Vec<u8> {
        let mut bits = Vec::new();
        for v in values {
            for i in 0..width {
                bits.push(((*v as u32) >> i) & 1 == 1);
            }
        }
        let mut bytes = vec![0u8; bits.len().div_ceil(8)];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                bytes[i / 8] |= 1 << (i % 8);
            }
        }
        let mut out = (bytes.len() as u16).to_le_bytes().to_vec();
        out.extend(bytes);
        out
    }

    #[test]
    fn decodes_delta_coded_eight_bit_block() {
        // 9-bit initial width; values without the top bit are plain deltas.
        let deltas = [10, 5, 3, 20, 40];
        let out = decompress(&pack(&deltas, 9), deltas.len(), false, false).expect("decode");
        let mut acc = 0;
        for (i, d) in deltas.iter().enumerate() {
            acc += d;
            assert!((out[i] - acc as f32 / 128.0).abs() < 1e-6, "sample {i}");
        }
    }

    #[test]
    fn truncated_data_is_an_error_not_a_panic() {
        assert!(decompress(&[], 10, false, false).is_err());
        assert!(decompress(&[5, 0, 1], 10, true, true).is_err());
    }
}
