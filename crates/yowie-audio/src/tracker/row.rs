//! Tick zero of a row: triggering notes and instruments.

use super::effects::{Effect, VolCmd};
use super::module::{Cell, Format, Module, NOTE_CUT, NOTE_FADE, NOTE_OFF, Nna};
use super::player::{State, note_period};
use super::state::{AutoVibState, Channel, EnvState, Voice};

/// Releases a voice: envelopes leave sustain and fading may start.
pub(crate) fn key_off(v: &mut Voice) {
    v.key_off = true;
    v.sustain_on = false;
    if v.inst.is_none() {
        v.active = false; // sample-only formats have nothing to release
    }
}

fn reset_envelopes(v: &mut Voice, inst: Option<usize>) {
    v.vol_env = EnvState::default();
    v.pan_env = EnvState::default();
    v.pitch_env = EnvState::default();
    v.key_off = false;
    v.fading = false;
    v.fade = 65536;
    v.auto_vib = AutoVibState::default();
    if inst.is_some() {
        v.inst = inst;
    }
}

/// Finds the sample and instrument for an instrument number and note.
fn resolve(m: &Module, inst_num: usize, note: i32) -> (Option<usize>, Option<usize>) {
    if inst_num == 0 {
        return (None, None);
    }
    if m.instruments.is_empty() {
        return (
            if inst_num <= m.samples.len() {
                Some(inst_num - 1)
            } else {
                None
            },
            None,
        );
    }
    let Some(inst) = m.instruments.get(inst_num - 1) else {
        return (None, None);
    };
    let n = note.clamp(0, 119) as usize;
    let si = inst.sample_map[n];
    if si < 0 || si as usize >= m.samples.len() {
        return (None, Some(inst_num - 1));
    }
    (Some(si as usize), Some(inst_num - 1))
}

impl State {
    /// Handles tick 0 of a row: note and instrument triggers, the volume column, and the
    /// effects that act once per row.
    pub(crate) fn process_row(&mut self, m: &Module) {
        let cells = self.current_row(m);
        for i in 0..self.chans.len() {
            let cell = cells.get(i).copied().unwrap_or_default();
            let mut ch = std::mem::take(&mut self.chans[i]);
            ch.note_cut = -1;
            ch.note_delay = -1;
            ch.key_off_tick = -1;
            ch.retrig_count = 0;
            if cell.effect == Effect::NoteDelay && cell.param > 0 {
                ch.note_delay = i32::from(cell.param);
                ch.delayed = cell;
                (ch.out_period, ch.out_volume, ch.out_pan) = (ch.v.period, ch.v.volume, ch.pan);
            } else {
                self.trigger_cell(m, &mut ch, cell);
                self.volume_column_row(m, &mut ch, cell);
                self.row_effect(m, &mut ch, cell);
                (ch.out_period, ch.out_volume, ch.out_pan) = (ch.v.period, ch.v.volume, ch.v.pan);
            }
            self.chans[i] = ch;
        }
    }

    /// Applies the instrument and note of a cell to the channel.
    pub(crate) fn trigger_cell(&mut self, m: &Module, ch: &mut Channel, cell: Cell) {
        let porta = cell.effect == Effect::TonePorta
            || cell.effect == Effect::TonePortaVol
            || cell.vol_cmd == VolCmd::TonePorta;
        let has_note = cell.note >= 0 && cell.note < NOTE_OFF;
        if cell.instrument > 0 {
            ch.last_inst = cell.instrument;
        }
        match cell.note {
            NOTE_OFF => {
                key_off(&mut ch.v);
                if cell.instrument > 0 && m.format == Format::It {
                    let (s, _) = resolve(m, ch.last_inst, ch.last_note);
                    ch.v.volume = s
                        .and_then(|s| m.samples.get(s))
                        .map_or(0, |s| s.volume as i32);
                }
                return;
            }
            NOTE_CUT => {
                ch.v.active = false;
                ch.v.volume = 0;
                return;
            }
            NOTE_FADE => {
                ch.v.fading = true;
                return;
            }
            _ => {}
        }
        let mut note = cell.note;
        if has_note {
            ch.last_note = note;
        } else {
            note = ch.last_note;
        }
        let (sample, inst) = resolve(m, ch.last_inst, note);
        if let (true, Some(si)) = (cell.instrument > 0, sample) {
            // An instrument number resets volume and pan; the sample itself only changes on a
            // new note (ProTracker swaps it when the loop wraps).
            let s = &m.samples[si];
            ch.v.volume = s.volume as i32;
            if s.has_pan && (has_note || m.format != Format::Mod) {
                ch.v.pan = s.pan;
            }
            if let Some(i) = inst.map(|i| &m.instruments[i]).filter(|i| i.has_pan) {
                ch.v.pan = i.pan;
            }
            if !has_note
                && m.format == Format::Mod
                && ch.v.sample.is_some()
                && ch.v.sample != sample
            {
                ch.pending_sample = sample;
            }
            if !has_note && m.format != Format::Mod {
                // XM and IT: an instrument without a note restarts envelopes but keeps the sample.
                reset_envelopes(&mut ch.v, inst);
            }
        }
        if !has_note {
            return;
        }
        let Some(sample) = sample else { return };
        if let Some(i) = inst {
            let mapped = m.instruments[i].note_map[note.clamp(0, 119) as usize];
            if mapped >= 0 {
                note = mapped;
            }
        }
        if porta && ch.v.active {
            // Tone portamento: aim at the new note without retriggering.
            if cell.instrument > 0 && m.format != Format::Mod {
                reset_envelopes(&mut ch.v, inst);
            }
            ch.v.note = note;
            if ch.v.sample.is_none() {
                ch.v.sample = Some(sample);
            }
            ch.porta_target = note_period(m, ch.v.sample, note);
            return;
        }
        self.start_note(m, ch, sample, inst, note);
    }

    /// Begins a fresh voice on the channel, applying the instrument's new-note action to
    /// whatever was playing.
    fn start_note(
        &mut self,
        m: &Module,
        ch: &mut Channel,
        sample: usize,
        inst: Option<usize>,
        note: i32,
    ) {
        if ch.v.active
            && m.format == Format::It
            && let Some(i) = inst
        {
            self.apply_nna(m, ch, i, sample, note);
        }
        let old = ch.v;
        ch.v = Voice {
            sample: Some(sample),
            inst,
            active: !m.samples[sample].data.is_empty(),
            index: old.index,
            note,
            volume: old.volume,
            pan: old.pan,
            cutoff: old.cutoff,
            resonance: old.resonance,
            filter_on: old.filter_on,
            sustain_on: true,
            ..Voice::default()
        };
        if let Some(i) = inst.map(|i| &m.instruments[i]) {
            if i.filter_cutoff >= 0 {
                ch.v.cutoff = i.filter_cutoff;
                ch.v.filter_on = true;
            }
            if i.filter_resonance >= 0 {
                ch.v.resonance = i.filter_resonance;
                ch.v.filter_on = true;
            }
        }
        ch.v.period = note_period(m, Some(sample), note);
        ch.porta_target = ch.v.period;
        ch.pending_sample = None;
        if ch.vib_wave & 4 == 0 {
            ch.vib_pos = 0;
        }
        if ch.trem_wave & 4 == 0 {
            ch.trem_pos = 0;
        }
        ch.tremor_count = 0;
        ch.tremor_off = false;
        reset_envelopes(&mut ch.v, inst);
    }

    /// Moves the channel's current voice to the background according to the new-note
    /// action, after the duplicate check.
    fn apply_nna(
        &mut self,
        m: &Module,
        ch: &mut Channel,
        inst_idx: usize,
        sample: usize,
        note: i32,
    ) {
        let inst = &m.instruments[inst_idx];
        let mut action = ch.v.nna_override.unwrap_or(inst.nna);
        if let Some(old_idx) = ch.v.inst {
            let old = &m.instruments[old_idx];
            if old.dct != 0 && old_idx == inst_idx {
                let dup = match old.dct {
                    1 => ch.v.note == note,
                    2 => ch.v.sample == Some(sample),
                    _ => true,
                };
                if dup {
                    action = match old.dca {
                        0 => Nna::Cut,
                        1 => Nna::Off,
                        2 => Nna::Fade,
                        _ => action,
                    };
                }
            }
        }
        if action == Nna::Cut || self.bg.len() >= 64 {
            return; // a full background simply cuts the oldest notes
        }
        let mut bg = ch.v;
        bg.nna_override = None;
        match action {
            Nna::Off => key_off(&mut bg),
            Nna::Fade => bg.fading = true,
            _ => {}
        }
        self.bg.push(bg);
    }

    /// Applies the tick-0 part of an XM or IT volume column.
    pub(crate) fn volume_column_row(&mut self, _m: &Module, ch: &mut Channel, cell: Cell) {
        let x = cell.vol_param as i32;
        match cell.vol_cmd {
            VolCmd::Set => ch.v.volume = x.min(64),
            VolCmd::FineUp => ch.v.volume = (ch.v.volume + x).min(64),
            VolCmd::FineDown => ch.v.volume = (ch.v.volume - x).max(0),
            VolCmd::VibratoSpeed => ch.vib_speed = x,
            VolCmd::VibratoDepth | VolCmd::Vibrato => {
                if x != 0 {
                    ch.vib_depth = x;
                }
            }
            VolCmd::Pan => ch.v.pan = x as f32 / 32.0 - 1.0,
            VolCmd::TonePorta if x != 0 => {
                ch.porta_speed = x;
            }
            _ => {}
        }
    }
}
