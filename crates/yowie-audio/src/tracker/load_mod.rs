//! ProTracker MOD loader.

use super::bytes::{Bytes, bad};
use super::effects::mod_effect;
use super::module::{Cell, Format, LoopType, Module, NOTE_NONE, Pattern, Sample, trim_name};
use super::tables::mod_note_from_period;
use crate::error::Result;

fn channel_count(tag: &[u8]) -> Option<usize> {
    match tag {
        b"M.K." | b"M!K!" | b"FLT4" | b"4CHN" => Some(4),
        b"6CHN" => Some(6),
        b"8CHN" | b"FLT8" | b"OCTA" | b"CD81" => Some(8),
        [d @ b'1'..=b'9', b'C', b'H', b'N'] => Some(usize::from(d - b'0')),
        [a @ b'1'..=b'9', b @ b'0'..=b'9', b'C', b'H'] => {
            Some(usize::from(a - b'0') * 10 + usize::from(b - b'0'))
        }
        _ => None,
    }
}

/// Parses a ProTracker-family module with 4 to 32 channels.
pub fn load_mod(data: &[u8]) -> Result<Module> {
    let d = Bytes(data);
    if d.len() < 1084 {
        return Err(bad(format!("MOD file too short ({} bytes)", d.len())));
    }
    let tag = d.slice(1080, 4)?;
    let channels = channel_count(tag).ok_or_else(|| {
        bad(format!(
            "unknown MOD tag {:?}",
            String::from_utf8_lossy(tag)
        ))
    })?;
    let mut m = Module {
        title: trim_name(d.slice(0, 20)?),
        channels,
        speed: 6,
        tempo: 125,
        format: Format::Mod,
        global_volume: 128,
        ..Module::default()
    };
    struct Header {
        length: usize,
        loop_start: usize,
        loop_len: usize,
    }
    let mut headers = Vec::with_capacity(31);
    for i in 0..31 {
        let off = 20 + i * 30;
        let h = d.slice(off, 30)?;
        let mut ft = i32::from(h[24] & 0x0F);
        if ft > 7 {
            ft -= 16;
        }
        headers.push(Header {
            length: usize::from(d.u16_be(off + 22)?) * 2,
            loop_start: usize::from(d.u16_be(off + 26)?) * 2,
            loop_len: usize::from(d.u16_be(off + 28)?) * 2,
        });
        m.samples.push(Sample {
            name: trim_name(&h[..22]),
            volume: usize::from(h[25]).min(64),
            global_volume: 64,
            finetune: ft as f32 / 8.0,
            c4_speed: 8363,
            ..Sample::default()
        });
    }
    let song_len = usize::from(d.u8(950)?);
    m.restart = usize::from(d.u8(951)?);
    if m.restart >= song_len {
        m.restart = 0;
    }
    let mut max_pattern = 0;
    for i in 0..128 {
        let p = usize::from(d.u8(952 + i)?);
        if i < song_len {
            m.orders.push(p);
        }
        max_pattern = max_pattern.max(p);
    }
    let pattern_size = 64 * channels * 4;
    let mut off = 1084;
    for p in 0..=max_pattern {
        let bytes = d
            .slice(off, pattern_size)
            .map_err(|_| bad(format!("MOD pattern {p} is truncated")))?;
        m.patterns.push(decode_pattern(bytes, channels));
        off += pattern_size;
    }
    for (i, h) in headers.iter().enumerate() {
        if h.length == 0 {
            continue;
        }
        let end = (off + h.length).min(d.len());
        let raw = d.0.get(off..end).unwrap_or(&[]);
        let s = &mut m.samples[i];
        s.data = raw.iter().map(|b| f32::from(*b as i8) / 128.0).collect();
        if h.loop_len > 2 {
            s.loop_start = h.loop_start.min(s.data.len());
            s.loop_end = (h.loop_start + h.loop_len).min(s.data.len());
            s.loop_type = LoopType::Forward;
        }
        off = end;
    }
    // Amiga hardware pans channels L R R L; the hard split is softened.
    m.pan = (0..channels)
        .map(|c| if c % 4 == 1 || c % 4 == 2 { 0.5 } else { -0.5 })
        .collect();
    Ok(m)
}

fn decode_pattern(data: &[u8], channels: usize) -> Pattern {
    let mut rows = Vec::with_capacity(64);
    for row in 0..64 {
        let mut cells = Vec::with_capacity(channels);
        for c in 0..channels {
            let at = (row * channels + c) * 4;
            let b = &data[at..at + 4];
            let period = i32::from(b[0] & 0x0F) << 8 | i32::from(b[1]);
            let (effect, param) = mod_effect(b[2] & 0x0F, b[3]);
            cells.push(Cell {
                note: if period > 0 {
                    mod_note_from_period(period)
                } else {
                    NOTE_NONE
                },
                instrument: usize::from(b[0] & 0xF0 | b[2] >> 4),
                effect,
                param,
                ..Cell::default()
            });
        }
        rows.push(cells);
    }
    Pattern { rows }
}
