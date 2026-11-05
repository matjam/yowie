//! ScreamTracker 3 loader.

use super::bytes::{Bytes, bad};
use super::effects::{VolCmd, s3m_effect};
use super::module::{
    Cell, Format, LoopType, Module, NOTE_NONE, NOTE_OFF, Pattern, Sample, empty_row, trim_name,
};
use crate::error::Result;

/// Parses a ScreamTracker 3 module. Adlib instruments are ignored.
pub fn load_s3m(data: &[u8]) -> Result<Module> {
    let d = Bytes(data);
    if d.len() < 96 || d.slice(44, 4)? != b"SCRM" {
        return Err(bad("missing SCRM signature"));
    }
    let ord_num = usize::from(d.u16(32)?);
    let ins_num = usize::from(d.u16(34)?);
    let pat_num = usize::from(d.u16(36)?);
    let ffi = d.u16(42)?;
    let mut m = Module {
        title: trim_name(d.slice(0, 28)?),
        global_volume: usize::from(d.u8(48)?).min(64) * 2,
        speed: usize::from(d.u8(49)?),
        tempo: usize::from(d.u8(50)?),
        format: Format::S3m,
        ..Module::default()
    };
    if m.speed == 0 {
        m.speed = 6;
    }
    if m.tempo < 33 {
        m.tempo = 125;
    }
    let stereo = d.u8(51)? & 0x80 != 0;
    m.mix_volume = usize::from(d.u8(51)? & 0x7F);
    let default_pan = d.u8(53)? == 252;
    let off = 96 + ord_num + ins_num * 2 + pat_num * 2;
    if off > d.len() {
        return Err(bad("S3M header is truncated"));
    }
    // The channel settings decide which of the 32 slots exist and their side.
    let mut slots = Vec::new();
    for c in 0..32 {
        let set = d.u8(64 + c)?;
        if set & 0x80 != 0 || set == 255 || set & 0x7F >= 16 {
            continue; // disabled, or an Adlib channel
        }
        slots.push((c, set));
    }
    m.channels = slots.len();
    m.pan = vec![0.0; m.channels];
    if stereo {
        for (i, (_, set)) in slots.iter().enumerate() {
            m.pan[i] = if set & 0x7F < 8 { -0.5 } else { 0.5 };
        }
    }
    for i in 0..ord_num {
        let o = usize::from(d.u8(96 + i)?);
        if o < 254 {
            m.orders.push(o);
        }
    }
    let ins_ptr = 96 + ord_num;
    let pat_ptr = ins_ptr + ins_num * 2;
    if default_pan && stereo && off + 32 <= d.len() {
        for (i, (slot, _)) in slots.iter().enumerate() {
            let p = d.u8(off + slot)?;
            if p & 0x20 != 0 {
                m.pan[i] = f32::from(p & 0x0F) / 7.5 - 1.0;
            }
        }
    }
    for i in 0..ins_num {
        let p = usize::from(d.u16(ins_ptr + i * 2)?) * 16;
        m.samples.push(
            load_instrument(d, p, ffi)
                .map_err(|e| bad(format!("S3M instrument {}: {e}", i + 1)))?,
        );
    }
    let slot_to_channel = |slot: usize| slots.iter().position(|(s, _)| *s == slot);
    for i in 0..pat_num {
        let p = usize::from(d.u16(pat_ptr + i * 2)?) * 16;
        m.patterns.push(
            load_pattern(d, p, m.channels, &slot_to_channel)
                .map_err(|e| bad(format!("S3M pattern {i}: {e}")))?,
        );
    }
    Ok(m)
}

fn load_instrument(d: Bytes, p: usize, ffi: u16) -> Result<Sample> {
    let empty = Sample {
        volume: 64,
        global_volume: 64,
        c4_speed: 8363,
        ..Sample::default()
    };
    if p == 0 || p + 80 > d.len() {
        return Ok(empty);
    }
    let h = d.slice(p, 80)?;
    let mut s = Sample {
        name: trim_name(&h[48..76]),
        volume: usize::from(h[28]).min(64),
        global_volume: 64,
        c4_speed: d.u32(p + 32)? as usize,
        ..Sample::default()
    };
    if h[0] != 1 {
        return Ok(s);
    }
    let memseg = usize::from(h[13]) << 16 | usize::from(d.u16(p + 14)?);
    let start = memseg * 16;
    let mut length = d.u32(p + 16)? as usize;
    let loop_begin = d.u32(p + 20)? as usize;
    let loop_end = d.u32(p + 24)? as usize;
    let flags = h[31];
    let sixteen = flags & 4 != 0;
    let bytes_per = if sixteen { 2 } else { 1 };
    if start.saturating_add(length.saturating_mul(bytes_per)) > d.len() {
        length = d.len().saturating_sub(start) / bytes_per;
    }
    let raw = d.0.get(start..start + length * bytes_per).unwrap_or(&[]);
    let unsigned = ffi == 2;
    s.data = if sixteen {
        raw.chunks_exact(2)
            .map(|c| {
                let v = u16::from_le_bytes([c[0], c[1]]);
                if unsigned {
                    (f32::from(v) - 32768.0) / 32768.0
                } else {
                    f32::from(v as i16) / 32768.0
                }
            })
            .collect()
    } else {
        raw.iter()
            .map(|&v| {
                if unsigned {
                    (f32::from(v) - 128.0) / 128.0
                } else {
                    f32::from(v as i8) / 128.0
                }
            })
            .collect()
    };
    if flags & 1 != 0 && loop_end > loop_begin {
        s.loop_start = loop_begin.min(length);
        s.loop_end = loop_end.min(length);
        s.loop_type = LoopType::Forward;
    }
    Ok(s)
}

fn load_pattern(
    d: Bytes,
    p: usize,
    channels: usize,
    slot_to_channel: &dyn Fn(usize) -> Option<usize>,
) -> Result<Pattern> {
    let mut rows: Vec<Vec<Cell>> = (0..64).map(|_| empty_row(channels)).collect();
    if p == 0 {
        return Ok(Pattern { rows });
    }
    let end = (p + 2 + usize::from(d.u16(p)?)).min(d.len());
    let mut off = p + 2;
    let mut row = 0;
    while off < end && row < 64 {
        let b = d.u8(off)?;
        off += 1;
        if b == 0 {
            row += 1;
            continue;
        }
        let mut cell = Cell {
            note: NOTE_NONE,
            ..Cell::default()
        };
        if b & 0x20 != 0 {
            if off + 2 > end {
                break;
            }
            let n = d.u8(off)?;
            cell.instrument = usize::from(d.u8(off + 1)?);
            off += 2;
            cell.note = match n {
                255 => NOTE_NONE,
                254 => NOTE_OFF,
                _ => i32::from(n >> 4) * 12 + i32::from(n & 0x0F),
            };
        }
        if b & 0x40 != 0 {
            if off + 1 > end {
                break;
            }
            cell.vol_cmd = VolCmd::Set;
            cell.vol_param = usize::from(d.u8(off)?).min(64);
            off += 1;
        }
        if b & 0x80 != 0 {
            if off + 2 > end {
                break;
            }
            (cell.effect, cell.param) = s3m_effect(d.u8(off)?, d.u8(off + 1)?);
            off += 2;
        }
        if let Some(ch) = slot_to_channel(usize::from(b & 0x1F)) {
            rows[row][ch] = cell;
        }
    }
    Ok(Pattern { rows })
}
