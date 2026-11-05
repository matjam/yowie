//! Ticks after the first: slides, vibrato, retriggers and the other continuous effects.

use super::effects::{Effect, VolCmd};
use super::module::{Cell, Format, Module};
use super::player::{State, note_period};
use super::row::key_off;
use super::state::{Channel, clamp_pan};
use super::tables::waveform;

impl State {
    /// Runs the per-tick part of every channel's effect and volume column, then works out
    /// the channel's output pitch, volume and pan.
    pub(crate) fn process_tick(&mut self, m: &Module) {
        use Effect as E;
        let cells = self.current_row(m);
        for i in 0..self.chans.len() {
            let cell = cells.get(i).copied().unwrap_or_default();
            let mut ch = std::mem::take(&mut self.chans[i]);
            if ch.note_delay >= 0 {
                if self.tick as i32 == ch.note_delay {
                    ch.note_delay = -1;
                    let d = ch.delayed;
                    self.trigger_cell(m, &mut ch, d);
                    self.volume_column_row(m, &mut ch, d);
                    self.row_effect(m, &mut ch, d);
                }
                (ch.out_period, ch.out_volume, ch.out_pan) = (ch.v.period, ch.v.volume, ch.v.pan);
                self.chans[i] = ch;
                continue;
            }
            if ch.note_cut >= 0 && self.tick as i32 == ch.note_cut {
                ch.v.volume = 0;
            }
            if ch.key_off_tick > 0 && self.tick as i32 == ch.key_off_tick {
                key_off(&mut ch.v);
            }
            (ch.out_period, ch.out_volume, ch.out_pan) = (ch.v.period, ch.v.volume, ch.v.pan);
            self.volume_column_tick(m, &mut ch, cell);
            let unit = self.slide_unit(m);
            match cell.effect {
                E::Arpeggio => self.arpeggio(m, &mut ch),
                E::PortaUp => {
                    ch.v.period = (ch.v.period - unit * f64::from(ch.porta_speed)).max(1.0);
                    ch.out_period = ch.v.period;
                }
                E::PortaDown => {
                    ch.v.period += unit * f64::from(ch.porta_speed);
                    ch.out_period = ch.v.period;
                }
                E::TonePorta => self.tone_porta(m, &mut ch),
                E::TonePortaVol => {
                    self.tone_porta(m, &mut ch);
                    self.vol_slide(m, &mut ch);
                }
                E::Vibrato => self.vibrato(m, &mut ch, 1),
                E::FineVibrato => self.vibrato(m, &mut ch, 4),
                E::VibratoVol => {
                    self.vibrato(m, &mut ch, 1);
                    self.vol_slide(m, &mut ch);
                }
                E::Tremolo => tremolo(&mut ch),
                E::Panbrello => panbrello(&mut ch),
                E::VolSlide => self.vol_slide(m, &mut ch),
                E::Retrig | E::MultiRetrig => retrig(m, &mut ch),
                E::Tremor => tremor(&mut ch),
                E::GlobalVolSlide => self.global_vol_slide(m, &ch),
                E::PanSlide => pan_slide(m, &mut ch),
                E::ChanVolSlide => {
                    let (hi, lo) = (
                        i32::from(ch.chan_vol_slide >> 4),
                        i32::from(ch.chan_vol_slide & 0x0F),
                    );
                    if hi != 0 && lo == 0 {
                        ch.chan_vol = (ch.chan_vol + hi).min(64);
                    } else if lo != 0 && hi == 0 {
                        ch.chan_vol = (ch.chan_vol - lo).max(0);
                    }
                }
                E::SetTempo if cell.param < 0x20 && m.format == Format::It => {
                    let step = usize::from(cell.param & 0x0F);
                    if cell.param >> 4 == 0 {
                        self.set_tempo(self.tempo.saturating_sub(step));
                    } else {
                        self.set_tempo(self.tempo + step);
                    }
                }
                _ => {}
            }
            self.chans[i] = ch;
        }
    }

    fn volume_column_tick(&mut self, m: &Module, ch: &mut Channel, cell: Cell) {
        let x = cell.vol_param as i32;
        let unit = self.slide_unit(m);
        match cell.vol_cmd {
            VolCmd::SlideUp => {
                ch.v.volume = (ch.v.volume + x).min(64);
                ch.out_volume = ch.v.volume;
            }
            VolCmd::SlideDown => {
                ch.v.volume = (ch.v.volume - x).max(0);
                ch.out_volume = ch.v.volume;
            }
            VolCmd::Vibrato => self.vibrato(m, ch, 1),
            VolCmd::VibratoDepth => {
                if m.format == Format::Xm {
                    self.vibrato(m, ch, 1);
                }
            }
            VolCmd::PanSlideLeft => ch.v.pan = clamp_pan(ch.v.pan - x as f32 / 32.0),
            VolCmd::PanSlideRight => ch.v.pan = clamp_pan(ch.v.pan + x as f32 / 32.0),
            VolCmd::TonePorta => self.tone_porta(m, ch),
            VolCmd::PortaUp => {
                ch.v.period = (ch.v.period - unit * f64::from(x) * 4.0).max(1.0);
                ch.out_period = ch.v.period;
            }
            VolCmd::PortaDown => {
                ch.v.period += unit * f64::from(x) * 4.0;
                ch.out_period = ch.v.period;
            }
            _ => {}
        }
    }

    fn arpeggio(&self, m: &Module, ch: &mut Channel) {
        if !ch.v.active {
            return;
        }
        let offset = match self.tick % 3 {
            1 => i32::from(ch.arp_param >> 4),
            2 => i32::from(ch.arp_param & 0x0F),
            _ => return,
        };
        ch.out_period = note_period(m, ch.v.sample, ch.v.note + offset);
    }

    fn tone_porta(&self, m: &Module, ch: &mut Channel) {
        if ch.porta_target <= 0.0 {
            return;
        }
        let step = self.slide_unit(m) * f64::from(ch.porta_speed);
        if ch.v.period < ch.porta_target {
            ch.v.period = (ch.v.period + step).min(ch.porta_target);
        } else if ch.v.period > ch.porta_target {
            ch.v.period = (ch.v.period - step).max(ch.porta_target);
        }
        ch.out_period = ch.v.period;
    }

    /// Offsets the output period; divisor 4 gives the fine variant.
    fn vibrato(&self, m: &Module, ch: &mut Channel, divisor: i32) {
        let mut delta = waveform(ch.vib_wave, ch.vib_pos) * ch.vib_depth;
        delta /= match m.format {
            Format::Mod => 128,
            _ if m.linear_slides => 64,
            _ => 32,
        };
        ch.out_period = ch.v.period + f64::from(delta / divisor);
        ch.vib_pos += ch.vib_speed;
    }

    /// Applies Axy and Dxy on ticks after the first; fine slides act on tick 0 only.
    fn vol_slide(&self, m: &Module, ch: &mut Channel) {
        let (x, y) = (i32::from(ch.vol_slide >> 4), i32::from(ch.vol_slide & 0x0F));
        let shared = matches!(m.format, Format::S3m | Format::It);
        if shared && (x == 0x0F || y == 0x0F) && x != 0 && y != 0 {
            return;
        }
        if x > 0 && (y == 0 || !shared) {
            ch.v.volume = (ch.v.volume + x).min(64);
        } else if y > 0 {
            ch.v.volume = (ch.v.volume - y).max(0);
        }
        ch.out_volume = ch.v.volume;
    }

    fn global_vol_slide(&mut self, m: &Module, ch: &Channel) {
        let (x, y) = (
            i32::from(ch.global_slide >> 4),
            i32::from(ch.global_slide & 0x0F),
        );
        let scale = if m.format == Format::It { 1 } else { 2 };
        if x > 0 && y == 0 {
            self.global_vol = (self.global_vol + x * scale).min(128);
        } else if y > 0 && x == 0 {
            self.global_vol = (self.global_vol - y * scale).max(0);
        }
    }
}

fn tremolo(ch: &mut Channel) {
    let delta = waveform(ch.trem_wave, ch.trem_pos) * ch.trem_depth / 64;
    ch.out_volume = (ch.v.volume + delta).clamp(0, 64);
    ch.trem_pos += ch.trem_speed;
}

fn panbrello(ch: &mut Channel) {
    let delta = (waveform(0, ch.panbr_pos) * ch.panbr_depth) as f32 / (255.0 * 16.0);
    ch.out_pan = clamp_pan(ch.v.pan + delta);
    ch.panbr_pos += ch.panbr_speed;
}

fn pan_slide(m: &Module, ch: &mut Channel) {
    let (x, y) = (ch.pan_slide >> 4, ch.pan_slide & 0x0F);
    if x == 0x0F || y == 0x0F {
        return;
    }
    let (x, y) = (f32::from(x), f32::from(y));
    ch.v.pan = if m.format == Format::Xm {
        clamp_pan(ch.v.pan + x / 128.0 - y / 128.0) // Pxy: x right, y left
    } else {
        clamp_pan(ch.v.pan - x / 32.0 + y / 32.0) // IT Pxy: x left, y right
    };
    ch.out_pan = ch.v.pan;
}

fn retrig(m: &Module, ch: &mut Channel) {
    let interval = if m.format == Format::Mod {
        i32::from(ch.retrig)
    } else {
        i32::from(ch.retrig & 0x0F)
    };
    if interval == 0 {
        return;
    }
    ch.retrig_count += 1;
    if ch.retrig_count < interval {
        return;
    }
    ch.retrig_count = 0;
    ch.v.pos = 0.0;
    ch.v.dir = 1;
    ch.v.active =
        ch.v.sample
            .and_then(|i| m.samples.get(i))
            .is_some_and(|s| !s.data.is_empty());
    if m.format != Format::Mod {
        let vol = ch.v.volume;
        ch.v.volume = match ch.retrig >> 4 {
            v @ 1..=5 => (vol - (1 << (v - 1))).max(0),
            6 => vol * 2 / 3,
            7 => vol / 2,
            v @ (9..=0xD) => (vol + (1 << (v - 9))).min(64),
            0xE => (vol * 3 / 2).min(64),
            0xF => (vol * 2).min(64),
            _ => vol,
        };
        ch.out_volume = ch.v.volume;
    }
}

/// Switches the note on for x+1 ticks and off for y+1.
fn tremor(ch: &mut Channel) {
    let on = i32::from(ch.tremor_param >> 4) + 1;
    let off = i32::from(ch.tremor_param & 0x0F) + 1;
    ch.tremor_count += 1;
    if ch.tremor_off {
        if ch.tremor_count >= off {
            ch.tremor_off = false;
            ch.tremor_count = 0;
        }
    } else if ch.tremor_count >= on {
        ch.tremor_off = true;
        ch.tremor_count = 0;
    }
    if ch.tremor_off {
        ch.out_volume = 0;
    }
}
