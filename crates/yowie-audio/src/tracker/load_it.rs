//! Impulse Tracker loader.

use super::bytes::{Bytes, bad};
use super::effects::{VolCmd, s3m_effect};
use super::it_decompress::decompress;
use super::module::{
    AutoVibrato, Cell, EnvPoint, Envelope, Format, Instrument, LoopType, Module, NOTE_CUT,
    NOTE_FADE, NOTE_OFF, Nna, Pattern, Sample, empty_row, trim_name,
};
use crate::error::Result;

/// Parses an Impulse Tracker module, including compressed samples.
pub fn load_it(data: &[u8]) -> Result<Module> {
    let d = Bytes(data);
    if d.len() < 192 || d.slice(0, 4)? != b"IMPM" {
        return Err(bad("missing IMPM signature"));
    }
    let ord_num = usize::from(d.u16(32)?);
    let ins_num = usize::from(d.u16(34)?);
    let smp_num = usize::from(d.u16(36)?);
    let pat_num = usize::from(d.u16(38)?);
    let cwtv = d.u16(40)?;
    let flags = d.u16(44)?;
    let mut m = Module {
        title: trim_name(d.slice(4, 26)?),
        format: Format::It,
        global_volume: usize::from(d.u8(48)?).min(128),
        mix_volume: usize::from(d.u8(49)?).min(128),
        speed: usize::from(d.u8(50)?),
        tempo: usize::from(d.u8(51)?),
        linear_slides: flags & 8 != 0,
        old_effects: flags & 16 != 0,
        compat_gxx: flags & 32 != 0,
        ..Module::default()
    };
    if m.speed == 0 {
        m.speed = 6;
    }
    if m.tempo < 32 {
        m.tempo = 125;
    }
    let stereo = flags & 1 != 0;
    let use_instruments = flags & 4 != 0;
    m.pan = (0..64)
        .map(|c| {
            let pan = d.u8(64 + c).unwrap_or(32);
            if pan >= 128 || pan == 100 || !stereo {
                0.0
            } else {
                f32::from(pan.min(64)) / 32.0 - 1.0
            }
        })
        .collect();
    m.channel_vol = (0..64)
        .map(|c| usize::from(d.u8(128 + c).unwrap_or(64)).min(64))
        .collect();
    let mut off = 192;
    for i in 0..ord_num {
        let Ok(o) = d.u8(off + i) else { break };
        if o < 254 {
            m.orders.push(usize::from(o));
        }
    }
    off += ord_num;
    let mut read_ptrs = |n: usize| -> Vec<usize> {
        let ptrs = (0..n)
            .map_while(|i| d.u32(off + 4 * i).ok().map(|p| p as usize))
            .collect();
        off += 4 * n;
        ptrs
    };
    let ins_ptrs = read_ptrs(ins_num);
    let smp_ptrs = read_ptrs(smp_num);
    let pat_ptrs = read_ptrs(pat_num);
    for (i, &p) in smp_ptrs.iter().enumerate() {
        m.samples
            .push(load_sample(d, p, cwtv).map_err(|e| bad(format!("IT sample {}: {e}", i + 1)))?);
    }
    if use_instruments {
        for (i, &p) in ins_ptrs.iter().enumerate() {
            m.instruments.push(
                load_instrument(d, p).map_err(|e| bad(format!("IT instrument {}: {e}", i + 1)))?,
            );
        }
    }
    let mut max_chan = 0;
    for (i, &p) in pat_ptrs.iter().enumerate() {
        let (pat, used) = load_pattern(d, p).map_err(|e| bad(format!("IT pattern {i}: {e}")))?;
        m.patterns.push(pat);
        max_chan = max_chan.max(used);
    }
    // Patterns are stored 64 channels wide; keep only the channels the song uses.
    m.channels = max_chan.max(1);
    m.pan.truncate(m.channels);
    m.channel_vol.truncate(m.channels);
    for pat in &mut m.patterns {
        for row in &mut pat.rows {
            row.truncate(m.channels);
        }
    }
    Ok(m)
}

fn load_sample(d: Bytes, p: usize, cwtv: u16) -> Result<Sample> {
    let empty = Sample {
        volume: 64,
        global_volume: 64,
        c4_speed: 8363,
        ..Sample::default()
    };
    if p == 0 || p + 80 > d.len() || d.slice(p, 4)? != b"IMPS" {
        return Ok(empty);
    }
    let h = Bytes(d.from(p)?);
    let flags = h.u8(18)?;
    let mut s = Sample {
        name: trim_name(h.slice(20, 26)?),
        global_volume: usize::from(h.u8(17)?).min(64),
        volume: usize::from(h.u8(19)?).min(64),
        c4_speed: h.u32(60)? as usize,
        vibrato: AutoVibrato {
            rate: usize::from(h.u8(76)?),
            depth: usize::from(h.u8(77)?),
            sweep: usize::from(h.u8(78)?),
            kind: usize::from(h.u8(79)?),
        },
        ..Sample::default()
    };
    let dfp = h.u8(47)?;
    if dfp & 0x80 != 0 {
        s.has_pan = true;
        s.pan = f32::from((dfp & 0x7F).min(64)) / 32.0 - 1.0;
    }
    let length = h.u32(48)? as usize;
    let (loop_begin, loop_end) = (h.u32(52)? as usize, h.u32(56)? as usize);
    let (sus_begin, sus_end) = (h.u32(64)? as usize, h.u32(68)? as usize);
    let ptr = h.u32(72)? as usize;
    if flags & 1 == 0 || length == 0 || ptr == 0 || ptr >= d.len() {
        return Ok(s);
    }
    let sixteen = flags & 2 != 0;
    let stereo = flags & 4 != 0;
    let signed = h.u8(46)? & 1 != 0;
    let channels = if stereo { 2 } else { 1 };
    let total = length
        .checked_mul(channels)
        .ok_or_else(|| bad("sample length overflows"))?;
    let mut samples = if flags & 8 != 0 {
        decompress(d.from(ptr)?, total, sixteen, cwtv >= 0x215)?
    } else {
        let bytes_per = if sixteen { 2 } else { 1 };
        let raw = d.from(ptr)?;
        let n = total.min(raw.len() / bytes_per);
        (0..n)
            .map(|i| {
                if sixteen {
                    let v = u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
                    if signed {
                        f32::from(v as i16) / 32768.0
                    } else {
                        (f32::from(v) - 32768.0) / 32768.0
                    }
                } else if signed {
                    f32::from(raw[i] as i8) / 128.0
                } else {
                    (f32::from(raw[i]) - 128.0) / 128.0
                }
            })
            .collect()
    };
    if stereo {
        // IT stores stereo as two mono blocks; they are averaged.
        let half = samples.len() / 2;
        samples = (0..half)
            .map(|i| (samples[i] + samples[half + i]) / 2.0)
            .collect();
    }
    let n = samples.len();
    s.data = samples;
    if flags & 16 != 0 && loop_end > loop_begin {
        s.loop_start = loop_begin.min(n);
        s.loop_end = loop_end.min(n);
        s.loop_type = if flags & 64 != 0 {
            LoopType::PingPong
        } else {
            LoopType::Forward
        };
    }
    if flags & 32 != 0 && sus_end > sus_begin {
        s.sus_loop_start = sus_begin.min(n);
        s.sus_loop_end = sus_end.min(n);
        s.sus_loop = if flags & 128 != 0 {
            LoopType::PingPong
        } else {
            LoopType::Forward
        };
    }
    Ok(s)
}

fn read_env(h: Bytes, at: usize, signed: bool) -> Result<(Envelope, bool)> {
    let flags = h.u8(at)?;
    let mut env = Envelope {
        enabled: flags & 1 != 0,
        looping: flags & 2 != 0,
        sustain: flags & 4 != 0,
        loop_start: usize::from(h.u8(at + 2)?),
        loop_end: usize::from(h.u8(at + 3)?),
        sustain_start: usize::from(h.u8(at + 4)?),
        sustain_end: usize::from(h.u8(at + 5)?),
        points: Vec::new(),
    };
    for i in 0..usize::from(h.u8(at + 1)?).min(25) {
        let raw = h.u8(at + 6 + i * 3)?;
        let value = if signed {
            f32::from(raw as i8)
        } else {
            f32::from(raw)
        };
        env.points.push(EnvPoint {
            tick: usize::from(h.u16(at + 7 + i * 3)?),
            value,
        });
    }
    if env.points.is_empty() {
        env.enabled = false;
    }
    Ok((env, flags & 0x80 != 0))
}

fn load_instrument(d: Bytes, p: usize) -> Result<Instrument> {
    let mut inst = Instrument::default();
    if p == 0 || p + 554 > d.len() || d.slice(p, 4)? != b"IMPI" {
        return Ok(inst);
    }
    let h = Bytes(d.from(p)?);
    inst.name = trim_name(h.slice(32, 26)?);
    inst.nna = match h.u8(17)? {
        0 => Nna::Cut,
        1 => Nna::Continue,
        2 => Nna::Off,
        _ => Nna::Fade,
    };
    inst.dct = h.u8(18)?;
    inst.dca = h.u8(19)?;
    inst.fadeout = i32::from(h.u16(20)?) * 64;
    inst.global_volume = usize::from(h.u8(24)?).min(128);
    let pan = h.u8(25)?;
    if pan & 0x80 == 0 {
        inst.has_pan = true;
        inst.pan = f32::from((pan & 0x7F).min(64)) / 32.0 - 1.0;
    }
    if h.u8(58)? & 0x80 != 0 {
        inst.filter_cutoff = i32::from(h.u8(58)? & 0x7F);
    }
    if h.u8(59)? & 0x80 != 0 {
        inst.filter_resonance = i32::from(h.u8(59)? & 0x7F);
    }
    for n in 0..120 {
        let note = h.u8(64 + n * 2)?;
        let smp = h.u8(65 + n * 2)?;
        if note < 120 {
            inst.note_map[n] = i32::from(note);
        }
        inst.sample_map[n] = i32::from(smp) - 1;
    }
    inst.vol_env = read_env(h, 304, false)?.0;
    inst.pan_env = read_env(h, 386, true)?.0;
    (inst.pitch_env, inst.pitch_is_filter) = read_env(h, 468, true)?;
    Ok(inst)
}

/// Decodes one packed pattern and reports the highest channel used.
fn load_pattern(d: Bytes, p: usize) -> Result<(Pattern, usize)> {
    if p == 0 {
        return Ok((
            Pattern {
                rows: (0..64).map(|_| empty_row(64)).collect(),
            },
            0,
        ));
    }
    let length = usize::from(d.u16(p)?);
    let rows = usize::from(d.u16(p + 2)?);
    d.slice(p, 8)
        .map_err(|_| bad("pattern pointer is past the end of the file"))?;
    if rows > 1024 {
        return Err(bad(format!("pattern has {rows} rows")));
    }
    let mut grid: Vec<Vec<Cell>> = (0..rows).map(|_| empty_row(64)).collect();
    let mut last_mask = [0u8; 64];
    let mut last = [[0u8; 5]; 64]; // note, instrument, volume, command, parameter
    let mut used = 0;
    let mut off = p + 8;
    let end = (p + 8 + length).min(d.len());
    let mut r = 0;
    while r < rows && off < end {
        let b = d.u8(off)?;
        off += 1;
        if b == 0 {
            r += 1;
            continue;
        }
        let c = usize::from(b.wrapping_sub(1)) & 63;
        let mut mask = last_mask[c];
        if b & 128 != 0 {
            if off >= end {
                break;
            }
            mask = d.u8(off)?;
            off += 1;
            last_mask[c] = mask;
        }
        used = used.max(c + 1);
        let need = [(1, 1), (2, 1), (4, 1), (8, 2)];
        let mut truncated = false;
        for (idx, (bit, size)) in need.iter().enumerate() {
            if mask & bit == 0 {
                continue;
            }
            if off + size > end {
                truncated = true;
                break;
            }
            if idx == 3 {
                last[c][3] = d.u8(off)?;
                last[c][4] = d.u8(off + 1)?;
            } else {
                last[c][idx] = d.u8(off)?;
            }
            off += size;
        }
        if truncated {
            break;
        }
        let cell = &mut grid[r][c];
        if mask & (1 | 16) != 0 {
            cell.note = note(last[c][0]);
        }
        if mask & (2 | 32) != 0 {
            cell.instrument = usize::from(last[c][1]);
        }
        if mask & (4 | 64) != 0 {
            (cell.vol_cmd, cell.vol_param) = volume(last[c][2]);
        }
        if mask & (8 | 128) != 0 {
            (cell.effect, cell.param) = s3m_effect(last[c][3], last[c][4]);
        }
    }
    Ok((Pattern { rows: grid }, used))
}

fn note(n: u8) -> i32 {
    match n {
        0..=119 => i32::from(n),
        255 => NOTE_OFF,
        254 => NOTE_CUT,
        _ => NOTE_FADE,
    }
}

const PORTA_TABLE: [usize; 10] = [0, 1, 4, 8, 16, 32, 64, 96, 128, 255];

/// Decodes the volume and panning column byte.
fn volume(v: u8) -> (VolCmd, usize) {
    let x = usize::from(v);
    match v {
        0..=64 => (VolCmd::Set, x),
        65..=74 => (VolCmd::FineUp, x - 65),
        75..=84 => (VolCmd::FineDown, x - 75),
        85..=94 => (VolCmd::SlideUp, x - 85),
        95..=104 => (VolCmd::SlideDown, x - 95),
        105..=114 => (VolCmd::PortaDown, x - 105),
        115..=124 => (VolCmd::PortaUp, x - 115),
        128..=192 => (VolCmd::Pan, x - 128),
        193..=202 => (VolCmd::TonePorta, PORTA_TABLE[x - 193]),
        203..=212 => (VolCmd::Vibrato, x - 203),
        _ => (VolCmd::None, 0),
    }
}
