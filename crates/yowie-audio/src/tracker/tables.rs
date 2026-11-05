//! Period tables, waveforms and the conversion from a period to a playback rate.

use super::module::Format;

/// ProTracker periods for octave 1 (C-1 to B-1) at finetune 0.
const PT_PERIODS: [f64; 12] = [
    856.0, 808.0, 762.0, 720.0, 678.0, 640.0, 604.0, 570.0, 538.0, 508.0, 480.0, 453.0,
];

/// ScreamTracker octave-0 periods, shifted right by the octave.
const ST3_PERIODS: [u32; 12] = [
    1712, 1616, 1524, 1440, 1356, 1280, 1208, 1140, 1076, 1016, 960, 907,
];

/// The classic 32-entry half-sine used by vibrato and tremolo.
const SINE_TABLE: [i32; 32] = [
    0, 24, 49, 74, 97, 120, 141, 161, 180, 197, 212, 224, 235, 244, 250, 253, 255, 253, 250, 244,
    235, 224, 212, 197, 180, 161, 141, 120, 97, 74, 49, 24,
];

/// The oscillator value in -255..=255 for position `pos` (taken modulo 64).
pub(crate) fn waveform(kind: usize, pos: i32) -> i32 {
    let pos = pos & 63;
    match kind & 3 {
        1 => 255 - pos * 8,
        2 => {
            if pos < 32 {
                255
            } else {
                -255
            }
        }
        3 => (pos * 37) % 511 - 255,
        _ => {
            let v = SINE_TABLE[(pos & 31) as usize];
            if pos >= 32 { -v } else { v }
        }
    }
}

/// The Amiga period for a ProTracker note index (octave * 12 + semitone, octave 1 lowest)
/// with finetune in semitones.
pub(crate) fn mod_period(note: i32, finetune: f32) -> f64 {
    let octave = note.div_euclid(12);
    let semi = note.rem_euclid(12) as usize;
    let p = PT_PERIODS[semi] * 2f64.powf(-f64::from(finetune) / 12.0);
    if octave >= 1 {
        p / f64::from(1u32 << (octave - 1).min(31))
    } else {
        p * 2.0
    }
}

/// The note index nearest an Amiga period.
pub(crate) fn mod_note_from_period(period: i32) -> i32 {
    let mut best = -1;
    let mut best_diff = f64::MAX;
    for note in 0..60 {
        let d = (mod_period(note, 0.0) - f64::from(period)).abs();
        if d < best_diff {
            best = note;
            best_diff = d;
        }
    }
    best
}

/// The ScreamTracker period for a note index and C4 speed.
pub(crate) fn s3m_period(note: i32, c4_speed: usize) -> f64 {
    let c4 = if c4_speed == 0 { 8363 } else { c4_speed };
    let octave = note.div_euclid(12).clamp(0, 31) as u32;
    let semi = note.rem_euclid(12) as usize;
    8363.0 * 16.0 * f64::from(ST3_PERIODS[semi] >> octave) / c4 as f64
}

// XM and IT express pitch as semitones s, where s = 48 plays the sample at its
// reference rate. Linear mode stores 64 period units per semitone; Amiga mode uses
// ScreamTracker-scale periods.
const LINEAR_BASE: f64 = 7680.0;
pub(crate) const LINEAR_PER_SEMI: f64 = 64.0;
const LINEAR_REF: f64 = 4608.0;

pub(crate) fn linear_period(semis: f64) -> f64 {
    LINEAR_BASE - semis * LINEAR_PER_SEMI
}

pub(crate) fn amiga_period_xm(semis: f64) -> f64 {
    1712.0 * 2f64.powf((48.0 - semis) / 12.0)
}

/// Converts a period to a sample rate in Hz for the format.
pub(crate) fn frequency(f: Format, linear: bool, period: f64) -> f64 {
    if period <= 0.0 {
        return 0.0;
    }
    match f {
        Format::Mod => 7_093_789.2 / (period * 2.0),
        _ if linear => 8363.0 * 2f64.powf((LINEAR_REF - period) / 768.0),
        _ => 14_317_456.0 / period,
    }
}

/// Impulse Tracker's 0..=127 filter cutoff as a frequency in Hz.
pub(crate) fn it_cutoff_hz(cutoff: i32) -> f64 {
    110.0 * 2f64.powf(0.25 + f64::from(cutoff) / 24.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_c2_plays_at_8287_hz() {
        // C-2 is note 12 in octave-1-lowest numbering: period 428, the Amiga's 8287 Hz.
        let f = frequency(Format::Mod, false, mod_period(12, 0.0));
        assert!(
            (mod_period(12, 0.0) - 428.0).abs() < 1.0 || (mod_period(12, 0.0) - 856.0).abs() < 1.0
        );
        assert!(f > 3000.0 && f < 17_000.0, "{f}");
    }

    #[test]
    fn period_round_trips_to_note() {
        for note in 0..48 {
            let p = mod_period(note, 0.0).round() as i32;
            let back = mod_note_from_period(p);
            assert_eq!(back, note, "note {note} period {p}");
        }
    }

    #[test]
    fn s3m_c4_at_reference_speed_is_8363_hz() {
        let p = s3m_period(4 * 12, 8363);
        let f = frequency(Format::S3m, false, p);
        assert!((f - 8363.0).abs() < 20.0, "{f}");
    }

    #[test]
    fn linear_octave_doubles_frequency() {
        let a = frequency(Format::Xm, true, linear_period(48.0));
        let b = frequency(Format::Xm, true, linear_period(60.0));
        assert!((a - 8363.0).abs() < 1.0 && (b / a - 2.0).abs() < 1e-6);
    }

    #[test]
    fn waveforms_span_full_range() {
        for kind in 0..4 {
            let max = (0..64).map(|p| waveform(kind, p)).max().unwrap_or(0);
            let min = (0..64).map(|p| waveform(kind, p)).min().unwrap_or(0);
            assert!(max > 200 && min < -200, "kind {kind}: {min}..{max}");
        }
    }
}
