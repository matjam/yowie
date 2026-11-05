//! FastTracker 2 loader.

use super::bytes::{Bytes, bad};
use super::effects::{VolCmd, xm_effect};
use super::module::{
    AutoVibrato, Cell, EnvPoint, Envelope, Format, Instrument, LoopType, Module, NOTE_NONE,
    NOTE_OFF, Pattern, Sample, empty_row, trim_name,
};
use crate::error::Result;

/// Parses a FastTracker 2 module.
pub fn load_xm(data: &[u8]) -> Result<Module> {
    let d = Bytes(data);
    if d.len() < 336 || d.slice(0, 17)? != b"Extended Module: " {
        return Err(bad("missing XM signature"));
    }
    let header_size = d.u32(60)? as usize;
    if header_size < 20 || 60 + header_size > d.len() {
        return Err(bad(format!("bad XM header size {header_size}")));
    }
    let song_len = usize::from(d.u16(64)?);
    let channels = usize::from(d.u16(68)?);
    let pat_num = usize::from(d.u16(70)?);
    let ins_num = usize::from(d.u16(72)?);
    let flags = d.u16(74)?;
    let mut m = Module {
        title: trim_name(d.slice(17, 20)?),
        channels,
        speed: usize::from(d.u16(76)?),
        tempo: usize::from(d.u16(78)?),
        format: Format::Xm,
        linear_slides: flags & 1 != 0,
        global_volume: 128,
        restart: usize::from(d.u16(66)?),
        ..Module::default()
    };
    if channels == 0 || channels > 64 {
        return Err(bad(format!("XM with {channels} channels")));
    }
    if m.speed == 0 {
        m.speed = 6;
    }
    if m.tempo < 32 {
        m.tempo = 125;
    }
    for i in 0..song_len.min(256) {
        if 80 + i >= 60 + header_size {
            break;
        }
        m.orders.push(usize::from(d.u8(80 + i)?));
    }
    if m.restart >= m.orders.len() {
        m.restart = 0;
    }
    m.pan = vec![0.0; channels];
    let mut off = 60 + header_size;
    for i in 0..pat_num {
        let (pat, next) =
            load_pattern(d, off, channels).map_err(|e| bad(format!("XM pattern {i}: {e}")))?;
        m.patterns.push(pat);
        off = next;
    }
    for i in 0..ins_num {
        off = load_instrument(&mut m, d, off)
            .map_err(|e| bad(format!("XM instrument {}: {e}", i + 1)))?;
    }
    Ok(m)
}

fn load_pattern(d: Bytes, off: usize, channels: usize) -> Result<(Pattern, usize)> {
    let hdr_len = d.u32(off)? as usize;
    let mut rows = usize::from(d.u16(off + 5)?);
    let packed = usize::from(d.u16(off + 7)?);
    if rows == 0 {
        rows = 64;
    }
    if rows > 1024 {
        return Err(bad(format!("pattern has {rows} rows")));
    }
    let mut grid: Vec<Vec<Cell>> = (0..rows).map(|_| empty_row(channels)).collect();
    let mut p = off + hdr_len;
    let end = (p + packed).min(d.len());
    'rows: for row in grid.iter_mut() {
        for cell in row.iter_mut() {
            if p >= end {
                break 'rows;
            }
            let b = d.u8(p)?;
            p += 1;
            let mut f = [0u8; 5]; // note, instrument, volume, effect, parameter
            if b & 0x80 != 0 {
                for (bit, slot) in f.iter_mut().enumerate() {
                    if b & (1 << bit) != 0 && p < end {
                        *slot = d.u8(p)?;
                        p += 1;
                    }
                }
            } else {
                if p + 4 > end {
                    break 'rows;
                }
                f = [b, d.u8(p)?, d.u8(p + 1)?, d.u8(p + 2)?, d.u8(p + 3)?];
                p += 4;
            }
            cell.note = match f[0] {
                97 => NOTE_OFF,
                n @ 1..=96 => i32::from(n) - 1,
                _ => NOTE_NONE,
            };
            cell.instrument = usize::from(f[1]);
            (cell.vol_cmd, cell.vol_param) = volume(f[2]);
            (cell.effect, cell.param) = xm_effect(f[3], f[4]);
        }
    }
    Ok((Pattern { rows: grid }, off + hdr_len + packed))
}

/// Decodes the volume column byte.
fn volume(v: u8) -> (VolCmd, usize) {
    let x = usize::from(v & 0x0F);
    match v {
        0x10..=0x50 => (VolCmd::Set, usize::from(v) - 0x10),
        0x60..=0x6F => (VolCmd::SlideDown, x),
        0x70..=0x7F => (VolCmd::SlideUp, x),
        0x80..=0x8F => (VolCmd::FineDown, x),
        0x90..=0x9F => (VolCmd::FineUp, x),
        0xA0..=0xAF => (VolCmd::VibratoSpeed, x),
        0xB0..=0xBF => (VolCmd::VibratoDepth, x),
        0xC0..=0xCF => (VolCmd::Pan, x * 64 / 15),
        0xD0..=0xDF => (VolCmd::PanSlideLeft, x),
        0xE0..=0xEF => (VolCmd::PanSlideRight, x),
        0xF0..=0xFF => (VolCmd::TonePorta, x * 16),
        _ => (VolCmd::None, 0),
    }
}

/// Reads an envelope whose points start at `at`. `meta` holds the header offsets of the
/// point count, sustain point, loop start, loop end and type flags.
fn read_env(d: Bytes, at: usize, meta: [usize; 5], pan: bool) -> Result<Envelope> {
    let count = usize::from(d.u8(meta[0])?);
    let sustain = usize::from(d.u8(meta[1])?);
    let loop_start = usize::from(d.u8(meta[2])?);
    let loop_end = usize::from(d.u8(meta[3])?);
    let typ = d.u8(meta[4])?;
    let mut e = Envelope {
        enabled: typ & 1 != 0,
        sustain: typ & 2 != 0,
        looping: typ & 4 != 0,
        sustain_start: sustain,
        sustain_end: sustain,
        loop_start,
        loop_end,
        points: Vec::new(),
    };
    for i in 0..count.min(12) {
        let tick = usize::from(d.u16(at + i * 4)?);
        let mut value = f32::from(d.u16(at + i * 4 + 2)?);
        if pan {
            value -= 32.0;
        }
        e.points.push(EnvPoint { tick, value });
    }
    if e.points.is_empty() {
        e.enabled = false;
    }
    Ok(e)
}

fn load_instrument(m: &mut Module, data: Bytes, off: usize) -> Result<usize> {
    let size = data.u32(off)? as usize;
    let mut inst = Instrument {
        name: trim_name(data.slice(off + 4, 22)?),
        ..Instrument::default()
    };
    let num_samples = usize::from(data.u16(off + 27)?);
    if num_samples == 0 || size < 243 {
        m.instruments.push(inst);
        return Ok(off + size);
    }
    let h = Bytes(data.from(off)?);
    let sample_header_size = h.u32(29)? as usize;
    let base = m.samples.len();
    for n in 0..96 {
        let si = usize::from(h.u8(33 + n)?);
        if si < num_samples {
            inst.sample_map[n] = (base + si) as i32;
        }
    }
    inst.vol_env = read_env(h, 129, [225, 227, 228, 229, 233], false)?;
    inst.pan_env = read_env(h, 177, [226, 230, 231, 232, 234], true)?;
    let vib = AutoVibrato {
        kind: usize::from(h.u8(235)?),
        sweep: usize::from(h.u8(236)?),
        depth: usize::from(h.u8(237)?),
        rate: usize::from(h.u8(238)?),
    };
    inst.fadeout = i32::from(h.u16(239)?) * 2;
    m.instruments.push(inst);

    let mut off = off + size;
    struct Sh {
        length: usize,
        loop_start: usize,
        loop_len: usize,
        sixteen: bool,
        ping_pong: bool,
        looped: bool,
    }
    let mut headers = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let s = Bytes(
            data.slice(off, 40)
                .map_err(|_| bad(format!("sample header {i} is truncated")))?,
        );
        let typ = s.u8(14)?;
        headers.push(Sh {
            length: s.u32(0)? as usize,
            loop_start: s.u32(4)? as usize,
            loop_len: s.u32(8)? as usize,
            sixteen: typ & 16 != 0,
            ping_pong: typ & 3 == 2,
            looped: typ & 3 != 0,
        });
        m.samples.push(Sample {
            name: trim_name(s.slice(18, 22)?),
            volume: usize::from(s.u8(12)?).min(64),
            global_volume: 64,
            finetune: f32::from(s.u8(13)? as i8) / 128.0,
            relative_note: i32::from(s.u8(16)? as i8),
            c4_speed: 8363,
            pan: f32::from(s.u8(15)?) / 127.5 - 1.0,
            has_pan: true,
            vibrato: vib,
            ..Sample::default()
        });
        off += sample_header_size;
    }
    for (i, h) in headers.iter().enumerate() {
        let bytes_per = if h.sixteen { 2 } else { 1 };
        let end = (off + h.length).min(data.len());
        let raw = data.0.get(off..end).unwrap_or(&[]);
        let count = (h.length / bytes_per).min(raw.len() / bytes_per);
        let s = &mut m.samples[base + i];
        let mut acc: i32 = 0;
        s.data = (0..count)
            .map(|j| {
                if h.sixteen {
                    acc = acc
                        .wrapping_add(i32::from(i16::from_le_bytes([raw[j * 2], raw[j * 2 + 1]])));
                    f32::from(acc as i16) / 32768.0
                } else {
                    acc = acc.wrapping_add(i32::from(raw[j] as i8));
                    f32::from(acc as i8) / 128.0
                }
            })
            .collect();
        if h.looped && h.loop_len / bytes_per > 1 {
            s.loop_start = (h.loop_start / bytes_per).min(count);
            s.loop_end = ((h.loop_start + h.loop_len) / bytes_per).min(count);
            s.loop_type = if h.ping_pong {
                LoopType::PingPong
            } else {
                LoopType::Forward
            };
        }
        off = end;
    }
    Ok(off)
}
