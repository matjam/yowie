//! The tick-zero part of each effect.

use super::effects::Effect;
use super::module::{Cell, Format, Module, NOTE_OFF, Nna};
use super::player::{State, note_period_for};
use super::row::key_off;
use super::state::{Channel, clamp_pan};

/// A parameter of zero recalls the previous one where the format has memory.
fn recall(x: u8, has_memory: bool, shared: Option<u8>, slot: &mut u8) -> u8 {
    if x != 0 {
        *slot = x;
        return x;
    }
    if !has_memory {
        return 0;
    }
    shared.unwrap_or(*slot)
}

impl State {
    /// Runs the tick-0 part of a cell's effect.
    pub(crate) fn row_effect(&mut self, m: &Module, ch: &mut Channel, cell: Cell) {
        use Effect as E;
        let x = cell.param;
        let unit = self.slide_unit(m);
        let shared = matches!(m.format, Format::S3m | Format::It);
        if shared && x != 0 {
            ch.mem = x;
        }
        let has_mem = self.has_memory(m);
        let sh = shared.then_some(ch.mem);
        match cell.effect {
            E::Arpeggio => ch.arp_param = recall(x, has_mem, sh, &mut ch.arp_param),
            E::PortaUp | E::PortaDown => {
                ch.porta_speed = i32::from(recall(x, has_mem, sh, &mut ch.porta_mem))
            }
            E::TonePorta => {
                let v = recall(x, has_mem, sh, &mut ch.porta_mem);
                if (v != 0 || !shared) && (m.compat_gxx || !shared || v != 0) {
                    ch.porta_speed = i32::from(v);
                }
            }
            E::TonePortaVol | E::VibratoVol | E::VolSlide => {
                ch.vol_slide = recall(x, has_mem, sh, &mut ch.vol_slide);
                if shared {
                    fine_vol_slide(ch);
                }
            }
            E::Vibrato | E::FineVibrato => {
                if x >> 4 != 0 {
                    ch.vib_speed = i32::from(x >> 4);
                }
                if x & 0x0F != 0 {
                    ch.vib_depth = i32::from(x & 0x0F);
                }
                if shared && x == 0 {
                    ch.vib_speed = i32::from(ch.vib_mem >> 4);
                    ch.vib_depth = i32::from(ch.vib_mem & 0x0F);
                } else if shared {
                    ch.vib_mem = x;
                }
            }
            E::Tremolo => {
                if x >> 4 != 0 {
                    ch.trem_speed = i32::from(x >> 4);
                }
                if x & 0x0F != 0 {
                    ch.trem_depth = i32::from(x & 0x0F);
                }
            }
            E::Panbrello => {
                if x >> 4 != 0 {
                    ch.panbr_speed = i32::from(x >> 4);
                }
                if x & 0x0F != 0 {
                    ch.panbr_depth = i32::from(x & 0x0F);
                }
            }
            E::SetPan => ch.v.pan = f32::from(x) / 127.5 - 1.0,
            E::SetPanCoarse => ch.v.pan = f32::from(x) / 7.5 - 1.0,
            E::Surround => ch.v.pan = 0.0,
            E::SampleOffsetHigh => ch.offset_high = i32::from(x),
            E::Offset => {
                let param = recall(x, has_mem, sh, &mut ch.offset);
                self.sample_offset(m, ch, cell, param);
            }
            E::PosJump => self.jump_order = i32::from(x),
            E::SetVolume => ch.v.volume = i32::from(x).min(64),
            E::PatBreak => self.break_row = i32::from(x),
            E::SetSpeed => {
                if x > 0 {
                    self.speed = usize::from(x);
                }
            }
            E::SetTempo => {
                if x >= 0x20 {
                    self.set_tempo(usize::from(x));
                } else if m.format == Format::It {
                    ch.mem = x; // T0x and T1x slide on later ticks
                }
            }
            E::FinePortaUp => {
                let v = recall(x, has_mem, sh, &mut ch.porta_mem) & 0x0F;
                ch.v.period = (ch.v.period - unit * f64::from(v)).max(1.0);
            }
            E::FinePortaDown => {
                let v = recall(x, has_mem, sh, &mut ch.porta_mem) & 0x0F;
                ch.v.period += unit * f64::from(v);
            }
            E::ExtraFinePortaUp => {
                let v = recall(x, has_mem, sh, &mut ch.porta_mem) & 0x0F;
                ch.v.period = (ch.v.period - f64::from(v)).max(1.0);
            }
            E::ExtraFinePortaDown => {
                let v = recall(x, has_mem, sh, &mut ch.porta_mem) & 0x0F;
                ch.v.period += f64::from(v);
            }
            E::VibWave => ch.vib_wave = usize::from(x),
            E::TremWave => ch.trem_wave = usize::from(x),
            E::Glissando => ch.glissando = x != 0,
            E::Finetune => {
                let ft = if x > 7 {
                    i32::from(x) - 16
                } else {
                    i32::from(x)
                };
                if let Some(s) = ch.v.sample.and_then(|i| m.samples.get(i)) {
                    ch.finetune = ft as f32 / 8.0;
                    ch.v.period =
                        note_period_for(m, ch.finetune, s.c4_speed, s.relative_note, ch.v.note);
                }
            }
            E::PatLoop => self.pattern_loop(x),
            E::Retrig | E::MultiRetrig => {
                ch.retrig = if m.format == Format::Mod {
                    x
                } else {
                    recall(x, has_mem, sh, &mut ch.retrig)
                };
                if m.format == Format::Xm && cell.effect == E::MultiRetrig && cell.note < 0 {
                    ch.retrig_count = 0;
                }
            }
            E::FineVolUp => ch.v.volume = (ch.v.volume + i32::from(x)).min(64),
            E::FineVolDown => ch.v.volume = (ch.v.volume - i32::from(x)).max(0),
            E::NoteCut => {
                if x == 0 {
                    ch.v.volume = 0;
                } else {
                    ch.note_cut = i32::from(x);
                }
            }
            E::PatDelay => {
                if self.pat_delay == 0 {
                    self.pat_delay = usize::from(x);
                }
            }
            E::Tremor => ch.tremor_param = recall(x, has_mem, sh, &mut ch.tremor_param),
            E::GlobalVol => {
                self.global_vol = if matches!(m.format, Format::Xm | Format::S3m) {
                    i32::from(x).min(64) * 2
                } else {
                    i32::from(x).min(128)
                };
            }
            E::GlobalVolSlide => ch.global_slide = recall(x, has_mem, sh, &mut ch.global_slide),
            E::KeyOff => {
                ch.key_off_tick = i32::from(x);
                if x == 0 {
                    key_off(&mut ch.v);
                }
            }
            E::EnvPos => {
                ch.v.vol_env.tick = usize::from(x);
                ch.v.pan_env.tick = usize::from(x);
            }
            E::PanSlide => {
                ch.pan_slide = recall(x, has_mem, sh, &mut ch.pan_slide);
                if shared {
                    let (hi, lo) = (ch.pan_slide >> 4, ch.pan_slide & 0x0F);
                    if lo == 0xF && hi != 0 {
                        ch.v.pan = clamp_pan(ch.v.pan + f32::from(hi) / 32.0);
                    } else if hi == 0xF && lo != 0 {
                        ch.v.pan = clamp_pan(ch.v.pan - f32::from(lo) / 32.0);
                    }
                }
            }
            E::ChanVol => ch.chan_vol = i32::from(x).min(64),
            E::ChanVolSlide => {
                ch.chan_vol_slide = recall(x, has_mem, sh, &mut ch.chan_vol_slide);
                let (hi, lo) = (
                    i32::from(ch.chan_vol_slide >> 4),
                    i32::from(ch.chan_vol_slide & 0x0F),
                );
                if lo == 0xF && hi != 0 {
                    ch.chan_vol = (ch.chan_vol + hi).min(64);
                } else if hi == 0xF && lo != 0 {
                    ch.chan_vol = (ch.chan_vol - lo).max(0);
                }
            }
            E::Filter => {
                if x < 0x80 {
                    ch.v.cutoff = i32::from(x);
                    ch.v.filter_on = true;
                } else if x < 0x90 {
                    ch.v.resonance = i32::from(x & 0x0F) * 8;
                    ch.v.filter_on = true;
                }
            }
            E::AmigaFilter => self.amiga_filter = x == 0,
            E::PastNote => self.past_note(ch, x),
            _ => {}
        }
    }

    fn sample_offset(&mut self, m: &Module, ch: &mut Channel, cell: Cell, param: u8) {
        let mut off = usize::from(param) * 256;
        if m.format == Format::It {
            off += (ch.offset_high as usize) << 16;
        }
        let Some(s) = ch.v.sample.and_then(|i| m.samples.get(i)) else {
            return;
        };
        if cell.note >= 0 && cell.note < NOTE_OFF {
            ch.v.pos = off as f64;
            if ch.v.pos >= s.data.len() as f64 {
                if s.loops() {
                    ch.v.pos = s.loop_start as f64;
                } else if matches!(m.format, Format::It | Format::Xm) {
                    ch.v.active = false;
                } else {
                    ch.v.pos = s.data.len() as f64;
                }
            }
        }
    }

    fn pattern_loop(&mut self, x: u8) {
        if x == 0 {
            self.loop_row = self.row;
        } else if self.loop_count == 0 {
            self.loop_count = i32::from(x);
            self.loop_jump = true;
        } else {
            self.loop_count -= 1;
            if self.loop_count > 0 {
                self.loop_jump = true;
            }
        }
    }

    fn past_note(&mut self, ch: &mut Channel, x: u8) {
        match x {
            0 => self.bg.iter_mut().for_each(|v| v.active = false),
            1 => self.bg.iter_mut().for_each(key_off),
            2 => self.bg.iter_mut().for_each(|v| v.fading = true),
            3..=6 => {
                // S73 to S76 override the new-note action of this channel's next note.
                ch.v.nna_override = Some(match x {
                    3 => Nna::Cut,
                    4 => Nna::Continue,
                    5 => Nna::Off,
                    _ => Nna::Fade,
                });
            }
            _ => {}
        }
    }
}

/// Applies the tick-0 half of a ScreamTracker or Impulse Tracker Dxy.
pub(crate) fn fine_vol_slide(ch: &mut Channel) {
    let (x, y) = (i32::from(ch.vol_slide >> 4), i32::from(ch.vol_slide & 0x0F));
    if y == 0x0F && x != 0 {
        ch.v.volume = (ch.v.volume + x).min(64);
    } else if x == 0x0F && y != 0 {
        ch.v.volume = (ch.v.volume - y).max(0);
    }
}
