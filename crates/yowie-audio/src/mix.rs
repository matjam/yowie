//! Rendering one voice into its bus for one chunk.

use crate::binaural::{EarParams, head_model};
use crate::bus::BusState;
use crate::sound::SoundData;
use crate::spatial::{Listener, occlusion_gain};
use crate::voice::{Fade, Source, VoiceState, valid_pitch};
use std::sync::atomic::Ordering;
use yowie_core::Vec3;

/// Mixer-wide values for the chunk being rendered.
pub(crate) struct Ctx<'a> {
    pub rate: u32,
    pub listener: &'a Listener,
    pub doppler: f32,
    pub speed_of_sound: f32,
    pub binaural: bool,
    pub head_radius: f32,
    pub paused: bool,
    pub solo_voices: bool,
    pub stop_frames: usize,
}

/// A voice's position, velocity and facing in the world.
fn source_space(v: &VoiceState, l: &Listener) -> (Vec3, Vec3, Vec3) {
    if v.relative {
        (
            l.position + l.local_vector(v.position),
            l.velocity + l.local_vector(v.velocity),
            l.local_vector(v.direction),
        )
    } else {
        (v.position, v.velocity, v.direction)
    }
}

/// Reads sound frames at `step` source frames per output frame with linear interpolation.
/// Returns the frames written and whether more remain.
#[allow(clippy::too_many_arguments)]
fn read_sound(
    data: &SoundData,
    pos: &mut f64,
    step: f64,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
    dst: &mut [f32],
    mono: bool,
) -> (usize, bool) {
    let total = data.samples.len() / 2;
    if total == 0 {
        return (0, false);
    }
    let totalf = total as f64;
    let (lstart, lend) = if looping && loop_end > loop_start {
        (loop_start.min(totalf), loop_end.min(totalf))
    } else {
        (0.0, totalf)
    };
    let end = if looping { lend } else { totalf };
    let frames = if mono { dst.len() } else { dst.len() / 2 };
    let mut p = *pos;
    for i in 0..frames {
        if p >= end {
            if !looping || end <= lstart {
                *pos = p;
                return (i, false);
            }
            p = lstart + (p - lstart).rem_euclid(end - lstart);
        }
        let j = (p as usize).min(total - 1);
        let t = (p - j as f64) as f32;
        let mut k = j + 1;
        if k as f64 >= end {
            k = if looping { lstart as usize } else { j };
        }
        let k = k.min(total - 1);
        let s = &data.samples;
        if mono {
            dst[i] = s[j * 2] * (1.0 - t) + s[k * 2] * t;
        } else {
            dst[i * 2] = s[j * 2] * (1.0 - t) + s[k * 2] * t;
            dst[i * 2 + 1] = s[j * 2 + 1] * (1.0 - t) + s[k * 2 + 1] * t;
        }
        p += step;
    }
    *pos = p;
    (frames, true)
}

/// Mixes up to `n` frames of the voice into its bus. Returns false once the voice has ended.
pub(crate) fn mix_voice(
    v: &mut VoiceState,
    cx: &Ctx,
    buses: &mut [BusState],
    scratch: &mut [f32],
    n: usize,
) -> bool {
    if v.done {
        return false;
    }
    let bus_idx = if v.bus < buses.len() { v.bus } else { 0 };
    let bus_paused = buses[bus_idx].shared.paused();
    let mut frames = n;
    let mut step = f64::from(v.pitch);
    let (tl, tr);
    let mut ear: Option<EarParams> = None;
    let mut stopping = false;
    let mut stop_last = false;
    if v.stop {
        // A stopped voice fades to silence over about a millisecond and then ends.
        if !v.started || (v.cur_l == 0.0 && v.cur_r == 0.0) {
            v.finish();
            return false;
        }
        if v.positional && cx.binaural {
            ear = v.bin.current();
        }
        if v.stop_left == 0 {
            v.stop_left = cx.stop_frames;
        }
        let k = v.stop_left.min(n);
        let left = (v.stop_left - k) as f32 / v.stop_left as f32;
        tl = v.cur_l * left;
        tr = v.cur_r * left;
        frames = k;
        stopping = true;
        stop_last = k >= v.stop_left;
    } else {
        let held = v.paused || cx.paused || bus_paused;
        if held && v.cur_l == 0.0 && v.cur_r == 0.0 {
            return true;
        }
        let l = cx.listener;
        let (position, velocity, direction) = source_space(v, l);
        let mut s = v.pitch;
        if v.positional && cx.doppler > 0.0 {
            s *= l.doppler(position, velocity, cx.doppler, cx.speed_of_sound);
        }
        step = f64::from(valid_pitch(s));
        let vol = v.fade.as_ref().map_or(v.vol, |f| fade_after(f, n));
        let mut gain = vol;
        if held || v.mute || (cx.solo_voices && !v.solo) {
            gain = 0.0;
        }
        if v.occlusion > 0.0 {
            gain *= occlusion_gain(v.occlusion);
        }
        let mut pan = v.pan;
        if v.positional {
            gain *= v
                .attenuation
                .gain(position.distance(l.position), v.min_dist, v.max_dist);
            gain *= v.cone.gain(direction, l.position - position);
            pan = l.pan_for(position, v.min_dist);
        }
        if v.positional && cx.binaural {
            let e = head_model(l, position, cx.head_radius, cx.rate);
            tl = gain * e.gain_l;
            tr = gain * e.gain_r;
            ear = Some(e);
        } else {
            v.bin.started = false;
            let pan = pan.clamp(-1.0, 1.0);
            tl = gain * ((1.0 - pan) / 2.0).sqrt();
            tr = gain * ((1.0 + pan) / 2.0).sqrt();
        }
        if !v.started {
            let fresh = v.fade.is_none();
            v.cur_l = if fresh { tl } else { 0.0 };
            v.cur_r = if fresh { tr } else { 0.0 };
        }
    }

    let mono_src = matches!(&v.source, Source::Sound { data } if data.mono);
    let (read, more) = match &mut v.source {
        Source::Sound { data } => {
            let dst = if mono_src {
                &mut scratch[..frames]
            } else {
                &mut scratch[..frames * 2]
            };
            read_sound(
                data,
                &mut v.pos,
                step,
                v.looping,
                v.loop_start,
                v.loop_end,
                dst,
                mono_src,
            )
        }
        Source::Stream(st) => {
            let (k, used) = st.read(&mut scratch[..frames * 2], step);
            v.pos += used;
            (k, k == frames)
        }
    };
    let mut mono = mono_src;
    if read > 0 {
        if ear.is_some() && !mono {
            for i in 0..read {
                scratch[i] = (scratch[i * 2] + scratch[i * 2 + 1]) * 0.5;
            }
            mono = true;
        }
        let used = if mono { read } else { read * 2 };
        let buf = &mut scratch[..used];
        if v.lp_on {
            if mono {
                v.lps.process_mono(&v.lpc, buf);
            } else {
                v.lps.process(&v.lpc, buf);
            }
        }
        if v.occlusion > 0.0 {
            if mono {
                v.occs.process_mono(&v.occc, buf);
            } else {
                v.occs.process(&v.occc, buf);
            }
        }
        let bus = &mut buses[bus_idx];
        let (dry, send) = (&mut bus.dry[..read * 2], &mut bus.send[..read * 2]);
        let ramp = [(v.cur_l, v.cur_r), (tl, tr)];
        match ear {
            Some(e) => v.bin.render(buf, e, ramp, dry, send, v.reverb),
            None => ramp_into(buf, mono, ramp, dry, send, v.reverb),
        }
        v.cur_l = tl;
        v.cur_r = tr;
        v.started = true;
        if stopping {
            v.stop_left = v.stop_left.saturating_sub(read);
        }
        advance_fade(v, read);
    }
    v.shared.position.store(v.pos.to_bits(), Ordering::Relaxed);
    if !more || stop_last {
        v.finish();
        return false;
    }
    true
}

/// The fade's level after `frames` more frames.
fn fade_after(f: &Fade, frames: usize) -> f32 {
    if f.total == 0 {
        return f.to;
    }
    let left = f.left.saturating_sub(frames);
    let t = 1.0 - left as f32 / f.total as f32;
    f.from + (f.to - f.from) * t
}

fn advance_fade(v: &mut VoiceState, frames: usize) {
    let Some(f) = v.fade.as_mut() else { return };
    f.left = f.left.saturating_sub(frames);
    if f.left == 0 {
        let (to, stop) = (f.to, f.stop);
        v.vol = to;
        v.fade = None;
        if stop {
            v.begin_stop();
        }
    }
}

/// Adds a mono or stereo chunk to `dry`, ramping gains across it, and to `send` scaled by `rev`.
fn ramp_into(
    buf: &[f32],
    mono: bool,
    ramp: [(f32, f32); 2],
    dry: &mut [f32],
    send: &mut [f32],
    rev: f32,
) {
    let n = dry.len() / 2;
    let inv = 1.0 / n as f32;
    let [(mut l, mut r), (tl, tr)] = ramp;
    let (dl, dr) = ((tl - l) * inv, (tr - r) * inv);
    for i in 0..n {
        l += dl;
        r += dr;
        let (x, y) = if mono {
            (buf[i], buf[i])
        } else {
            (buf[i * 2], buf[i * 2 + 1])
        };
        let (sl, sr) = (x * l, y * r);
        dry[i * 2] += sl;
        dry[i * 2 + 1] += sr;
        if rev > 0.0 {
            send[i * 2] += sl * rev;
            send[i * 2 + 1] += sr * rev;
        }
    }
}
