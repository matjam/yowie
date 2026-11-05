//! The format-independent effect vocabulary and each format's mapping onto it.

/// A pattern effect. Loaders map their own command letters onto these; the player
/// interprets them with the module's [`Format`](super::Format) where semantics differ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    /// No effect.
    #[default]
    None,
    /// Arpeggio over the note and two offsets in semitones.
    Arpeggio,
    /// Portamento up.
    PortaUp,
    /// Portamento down.
    PortaDown,
    /// Tone portamento toward the new note.
    TonePorta,
    /// Vibrato.
    Vibrato,
    /// Tone portamento with a volume slide.
    TonePortaVol,
    /// Vibrato with a volume slide.
    VibratoVol,
    /// Tremolo.
    Tremolo,
    /// Set pan, 0..=255.
    SetPan,
    /// Set sample offset in units of 256 frames.
    Offset,
    /// Volume slide.
    VolSlide,
    /// Jump to a song position.
    PosJump,
    /// Set volume.
    SetVolume,
    /// Pattern break to a row; the parameter is already decoded from BCD.
    PatBreak,
    /// Set ticks per row.
    SetSpeed,
    /// Set tempo (0x20 and up); IT T0x and T1x slide the tempo down and up.
    SetTempo,
    /// Fine portamento up.
    FinePortaUp,
    /// Fine portamento down.
    FinePortaDown,
    /// Extra-fine portamento up.
    ExtraFinePortaUp,
    /// Extra-fine portamento down.
    ExtraFinePortaDown,
    /// Select the vibrato waveform.
    VibWave,
    /// Set the sample finetune.
    Finetune,
    /// Pattern loop.
    PatLoop,
    /// Select the tremolo waveform.
    TremWave,
    /// Retrigger the note.
    Retrig,
    /// Fine volume slide up.
    FineVolUp,
    /// Fine volume slide down.
    FineVolDown,
    /// Cut the note after a number of ticks.
    NoteCut,
    /// Delay the note by a number of ticks.
    NoteDelay,
    /// Delay the pattern by a number of rows.
    PatDelay,
    /// Tremor.
    Tremor,
    /// Set the global volume.
    GlobalVol,
    /// Global volume slide.
    GlobalVolSlide,
    /// Fine vibrato.
    FineVibrato,
    /// Set pan, 0..=15.
    SetPanCoarse,
    /// XM Kxx: key off at a tick.
    KeyOff,
    /// XM Lxx: set the envelope position.
    EnvPos,
    /// Pan slide.
    PanSlide,
    /// XM Rxy: retrigger every y ticks with a volume change of x.
    MultiRetrig,
    /// IT Mxx: set the channel volume.
    ChanVol,
    /// IT Nxy: channel volume slide.
    ChanVolSlide,
    /// IT Yxy: panbrello.
    Panbrello,
    /// IT Zxx: filter cutoff and resonance.
    Filter,
    /// MOD E0x: the Amiga LED filter.
    AmigaFilter,
    /// Glissando control.
    Glissando,
    /// IT SAy: high part of the sample offset.
    SampleOffsetHigh,
    /// IT surround; played as centre.
    Surround,
    /// IT S7x: past-note actions and new-note action overrides.
    PastNote,
}

/// The XM and IT volume column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VolCmd {
    /// Empty.
    #[default]
    None,
    /// Set volume.
    Set,
    /// Slide volume up each tick.
    SlideUp,
    /// Slide volume down each tick.
    SlideDown,
    /// Fine volume up on the first tick.
    FineUp,
    /// Fine volume down on the first tick.
    FineDown,
    /// Set vibrato speed.
    VibratoSpeed,
    /// Set vibrato depth.
    VibratoDepth,
    /// IT: vibrato with depth memory.
    Vibrato,
    /// Set pan.
    Pan,
    /// Pan slide left.
    PanSlideLeft,
    /// Pan slide right.
    PanSlideRight,
    /// Tone portamento.
    TonePorta,
    /// Portamento up.
    PortaUp,
    /// Portamento down.
    PortaDown,
}

/// Maps a ProTracker command and parameter.
pub(crate) fn mod_effect(cmd: u8, param: u8) -> (Effect, u8) {
    use Effect as E;
    match cmd {
        0x0 if param != 0 => (E::Arpeggio, param),
        0x1 => (E::PortaUp, param),
        0x2 => (E::PortaDown, param),
        0x3 => (E::TonePorta, param),
        0x4 => (E::Vibrato, param),
        0x5 => (E::TonePortaVol, param),
        0x6 => (E::VibratoVol, param),
        0x7 => (E::Tremolo, param),
        0x8 => (E::SetPan, param),
        0x9 => (E::Offset, param),
        0xA => (E::VolSlide, param),
        0xB => (E::PosJump, param),
        0xC => (E::SetVolume, param),
        0xD => (E::PatBreak, (param >> 4) * 10 + (param & 0x0F)),
        0xE => {
            let x = param & 0x0F;
            let e = match param >> 4 {
                0x0 => E::AmigaFilter,
                0x1 => E::FinePortaUp,
                0x2 => E::FinePortaDown,
                0x3 => E::Glissando,
                0x4 => E::VibWave,
                0x5 => E::Finetune,
                0x6 => E::PatLoop,
                0x7 => E::TremWave,
                0x8 => E::SetPanCoarse,
                0x9 => E::Retrig,
                0xA => E::FineVolUp,
                0xB => E::FineVolDown,
                0xC => E::NoteCut,
                0xD => E::NoteDelay,
                0xE => E::PatDelay,
                _ => E::None,
            };
            (e, x)
        }
        0xF if param < 32 => (E::SetSpeed, param),
        0xF => (E::SetTempo, param),
        _ => (E::None, 0),
    }
}

/// Maps a FastTracker 2 command; the first 16 match ProTracker.
pub(crate) fn xm_effect(cmd: u8, param: u8) -> (Effect, u8) {
    use Effect as E;
    if cmd <= 0xF {
        return mod_effect(cmd, param);
    }
    match cmd {
        0x10 => (E::GlobalVol, param),
        0x11 => (E::GlobalVolSlide, param),
        0x14 => (E::KeyOff, param),
        0x15 => (E::EnvPos, param),
        0x19 => (E::PanSlide, param),
        0x1B => (E::MultiRetrig, param),
        0x1D => (E::Tremor, param),
        0x21 => match param >> 4 {
            1 => (E::ExtraFinePortaUp, param & 0x0F),
            2 => (E::ExtraFinePortaDown, param & 0x0F),
            _ => (E::None, 0),
        },
        _ => (E::None, 0),
    }
}

/// Maps a ScreamTracker command (1 is A) and parameter; Impulse Tracker uses the same
/// letters with a few additions.
pub(crate) fn s3m_effect(cmd: u8, param: u8) -> (Effect, u8) {
    use Effect as E;
    let letter = cmd.wrapping_add(b'@');
    match letter {
        b'A' => (E::SetSpeed, param),
        b'B' => (E::PosJump, param),
        b'C' => (E::PatBreak, (param >> 4) * 10 + (param & 0x0F)),
        b'D' => (E::VolSlide, param),
        b'E' if param >= 0xF0 => (E::FinePortaDown, param & 0x0F),
        b'E' if param >= 0xE0 => (E::ExtraFinePortaDown, param & 0x0F),
        b'E' => (E::PortaDown, param),
        b'F' if param >= 0xF0 => (E::FinePortaUp, param & 0x0F),
        b'F' if param >= 0xE0 => (E::ExtraFinePortaUp, param & 0x0F),
        b'F' => (E::PortaUp, param),
        b'G' => (E::TonePorta, param),
        b'H' => (E::Vibrato, param),
        b'I' => (E::Tremor, param),
        b'J' => (E::Arpeggio, param),
        b'K' => (E::VibratoVol, param),
        b'L' => (E::TonePortaVol, param),
        b'M' => (E::ChanVol, param),
        b'N' => (E::ChanVolSlide, param),
        b'O' => (E::Offset, param),
        b'P' => (E::PanSlide, param),
        b'Q' => (E::Retrig, param),
        b'R' => (E::Tremolo, param),
        b'S' => {
            let x = param & 0x0F;
            let e = match param >> 4 {
                0x1 => E::Glissando,
                0x2 => E::Finetune,
                0x3 => E::VibWave,
                0x4 => E::TremWave,
                0x7 => E::PastNote,
                0x8 => E::SetPanCoarse,
                0x9 => E::Surround,
                0xA => E::SampleOffsetHigh,
                0xB => E::PatLoop,
                0xC => E::NoteCut,
                0xD => E::NoteDelay,
                0xE => E::PatDelay,
                _ => E::None,
            };
            (e, x)
        }
        b'T' => (E::SetTempo, param),
        b'U' => (E::FineVibrato, param),
        b'V' => (E::GlobalVol, param),
        b'W' => (E::GlobalVolSlide, param),
        b'X' if param == 0xA4 => (E::SetPan, 128),
        b'X' => (E::SetPan, (usize::from(param) * 2).min(255) as u8),
        b'Y' => (E::Panbrello, param),
        b'Z' => (E::Filter, param),
        _ => (E::None, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_commands() {
        let cases = [
            (0x0, 0x00, Effect::None, 0),
            (0x0, 0x37, Effect::Arpeggio, 0x37),
            (0xC, 0x20, Effect::SetVolume, 0x20),
            (0xD, 0x32, Effect::PatBreak, 32),
            (0xF, 0x06, Effect::SetSpeed, 6),
            (0xF, 0x7D, Effect::SetTempo, 0x7D),
            (0xE, 0x93, Effect::Retrig, 3),
            (0xE, 0xD2, Effect::NoteDelay, 2),
        ];
        for (cmd, param, effect, out) in cases {
            assert_eq!(
                mod_effect(cmd, param),
                (effect, out),
                "cmd {cmd:X} {param:X}"
            );
        }
    }

    #[test]
    fn s3m_commands() {
        let cases = [
            (1, 6, Effect::SetSpeed, 6),
            (5, 0xF3, Effect::FinePortaDown, 3),
            (5, 0xE3, Effect::ExtraFinePortaDown, 3),
            (6, 0x10, Effect::PortaUp, 0x10),
            (19, 0xB0, Effect::PatLoop, 0),
            (24, 0x40, Effect::SetPan, 128),
            (20, 0x80, Effect::SetTempo, 0x80),
        ];
        for (cmd, param, effect, out) in cases {
            assert_eq!(s3m_effect(cmd, param), (effect, out), "cmd {cmd} {param:X}");
        }
    }
}
