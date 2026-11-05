//! Delta encoding of serialized snapshots against a baseline.
//!
//! A delta is built from the postcard bytes of the baseline and the current
//! value: the new length, then runs of (bytes unchanged from the baseline,
//! literal bytes). Snapshots with a fixed layout (numbers, fixed arrays,
//! structs of those) keep each field at the same offset, so unchanged
//! fields cost nothing. Variable-length fields (vectors, strings) shift the
//! bytes after them and make the delta larger, never incorrect.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// A run of at least this many equal bytes ends a literal.
const MIN_RUN: usize = 6;
/// Largest snapshot a delta may describe, to bound allocation from
/// untrusted input.
const MAX_LEN: usize = 16 << 20;

fn put_varint(out: &mut Vec<u8>, mut v: usize) {
    while v >= 0x80 {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn get_varint(b: &[u8], pos: &mut usize) -> Result<usize> {
    let mut v = 0usize;
    for shift in (0..35).step_by(7) {
        let byte = *b.get(*pos).ok_or(Error::BadDelta("truncated"))?;
        *pos += 1;
        v |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(Error::BadDelta("varint too long"))
}

/// Computes the delta that turns `baseline` into `current`.
pub fn diff_bytes(baseline: &[u8], current: &[u8]) -> Vec<u8> {
    let n = current.len();
    let same = |i: usize| baseline.get(i) == Some(&current[i]);
    let mut out = Vec::new();
    put_varint(&mut out, n);
    let mut i = 0;
    while i < n {
        let skip_start = i;
        while i < n && same(i) {
            i += 1;
        }
        let lit_start = i;
        while i < n {
            if same(i) {
                let mut j = i;
                while j < n && same(j) {
                    j += 1;
                }
                if j - i >= MIN_RUN || j == n {
                    break;
                }
                i = j;
            } else {
                i += 1;
            }
        }
        put_varint(&mut out, lit_start - skip_start);
        put_varint(&mut out, i - lit_start);
        out.extend_from_slice(&current[lit_start..i]);
    }
    out
}

/// Rebuilds the current bytes from `baseline` and a delta from
/// [`diff_bytes`].
pub fn apply_delta(baseline: &[u8], delta: &[u8]) -> Result<Vec<u8>> {
    let mut pos = 0;
    let n = get_varint(delta, &mut pos)?;
    if n > MAX_LEN {
        return Err(Error::BadDelta("snapshot too large"));
    }
    let mut out = Vec::with_capacity(n.min(delta.len().saturating_mul(4) + 64));
    while out.len() < n {
        let skip = get_varint(delta, &mut pos)?;
        let lit = get_varint(delta, &mut pos)?;
        let at = out.len();
        let end = at.checked_add(skip).ok_or(Error::BadDelta("overflow"))?;
        let copied = baseline
            .get(at..end)
            .ok_or(Error::BadDelta("baseline too short"))?;
        out.extend_from_slice(copied);
        let bytes = delta
            .get(pos..pos.saturating_add(lit))
            .ok_or(Error::BadDelta("truncated literal"))?;
        pos += lit;
        out.extend_from_slice(bytes);
        if out.len() > n {
            return Err(Error::BadDelta("longer than announced"));
        }
        if skip == 0 && lit == 0 {
            return Err(Error::BadDelta("empty run"));
        }
    }
    Ok(out)
}

/// Encodes `current` as a delta against `baseline`.
pub fn encode_delta<S: Serialize>(baseline: &S, current: &S) -> Result<Vec<u8>> {
    Ok(diff_bytes(
        &postcard::to_stdvec(baseline)?,
        &postcard::to_stdvec(current)?,
    ))
}

/// Decodes a delta made by [`encode_delta`] against the same `baseline`.
pub fn decode_delta<S: Serialize + DeserializeOwned>(baseline: &S, delta: &[u8]) -> Result<S> {
    let bytes = apply_delta(&postcard::to_stdvec(baseline)?, delta)?;
    Ok(postcard::from_bytes(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct World {
        tick: u32,
        players: [(f32, f32, u16); 16],
        flags: u64,
    }

    fn world() -> World {
        let mut players = [(0.0, 0.0, 0); 16];
        for (i, p) in players.iter_mut().enumerate() {
            *p = (i as f32 * 1.5, i as f32 * -2.0, 100 + i as u16);
        }
        World {
            tick: 1,
            players,
            flags: 0xdead_beef,
        }
    }

    #[test]
    fn delta_round_trips_and_shrinks() {
        let base = world();
        let mut cur = base.clone();
        cur.tick = 2;
        cur.players[3].0 = 99.0;
        cur.players[11].2 = 7;
        let d = encode_delta(&base, &cur).expect("encodes");
        let full = postcard::to_stdvec(&cur).expect("encodes").len();
        assert!(d.len() < full / 2, "delta {} vs full {}", d.len(), full);
        assert_eq!(decode_delta(&base, &d).expect("decodes"), cur);
        let same = encode_delta(&base, &base).expect("encodes");
        assert!(same.len() <= 5);
        assert_eq!(decode_delta(&base, &same).expect("decodes"), base);
    }

    #[test]
    fn byte_diff_cases() {
        let cases: [(&[u8], &[u8]); 6] = [
            (b"", b""),
            (b"", b"hello"),
            (b"hello world", b"hello"),
            (b"hello", b"hello world, longer than before"),
            (b"aaaaaaaaaaaaaaaaaaaa", b"aaaaaaaaabaaaaaaaaaa"),
            (b"abcdef", b"zzzzzz"),
        ];
        for (a, b) in cases {
            let d = diff_bytes(a, b);
            assert_eq!(apply_delta(a, &d).expect("applies"), b, "{a:?} -> {b:?}");
        }
    }

    #[test]
    fn corrupt_deltas_are_errors() {
        for d in [
            &[][..],
            &[200u8][..],
            &[5, 9, 0][..],
            &[3, 0, 9, 1][..],
            &[0xff; 8][..],
        ] {
            assert!(apply_delta(b"abc", d).is_err(), "{d:?}");
        }
    }
}
