//! Tracker tests over modules built in memory.

use super::test_modules::{build_it, build_mod, build_s3m, build_xm};
use super::{Cell, Effect, Format, Module, NOTE_NONE, Player, PlayerOptions};
use crate::Stream;
use std::sync::Arc;

const RATE: u32 = 48_000;

fn player(data: &[u8], looping: bool) -> Player {
    let m = Module::load(data).expect("module loads");
    Player::with_options(
        Arc::new(m),
        RATE,
        PlayerOptions {
            looping,
            ..Default::default()
        },
    )
}

fn zero_crossings(out: &[f32]) -> usize {
    out.chunks_exact(2)
        .map(|f| f[0])
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count()
}

fn peak(out: &[f32]) -> f32 {
    out.iter().fold(0.0, |a, b| a.max(b.abs()))
}

fn render(p: &mut Player, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    let n = p.read(&mut out);
    out.truncate(n * 2);
    out
}

#[test]
fn every_format_plays_the_right_pitch() {
    // MOD C-2 at period 428 is 8287 Hz over a 32-frame cycle: about 518 crossings a second.
    // S3M C-4, XM C-5 (note 49) and IT C-5 all play the sample at 8363 Hz: about 523.
    let cases = [
        ("MOD", build_mod(None, None), Format::Mod, 518),
        ("S3M", build_s3m(), Format::S3m, 523),
        ("XM", build_xm(), Format::Xm, 523),
        ("IT", build_it(), Format::It, 523),
    ];
    for (name, data, format, want) in cases {
        let m = Module::load(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(m.format, format, "{name}");
        let mut p = Player::with_options(
            Arc::new(m),
            RATE,
            PlayerOptions {
                looping: true,
                ..Default::default()
            },
        );
        let out = render(&mut p, RATE as usize);
        assert_eq!(out.len(), RATE as usize * 2, "{name}");
        let zc = zero_crossings(&out);
        assert!(
            zc.abs_diff(want) < 50,
            "{name}: {zc} crossings, want about {want}"
        );
        assert!(peak(&out) > 0.005, "{name} is silent");
    }
}

#[test]
fn loaders_decode_cells_and_samples() {
    let m = Module::load(&build_mod(None, None)).expect("mod");
    assert_eq!((m.channels, m.patterns.len(), m.samples.len()), (4, 1, 31));
    assert!(m.samples[0].loops());
    let c = m.patterns[0].rows[0][0];
    assert_eq!(c.instrument, 1);
    assert_eq!(c.note, 24, "period 428 is note 24");
    assert_eq!(m.patterns[0].rows[0][1], Cell::default());
    let s = Module::load(&build_s3m()).expect("s3m");
    assert_eq!(
        (s.channels, s.samples.len(), s.samples[0].data.len()),
        (1, 1, 32)
    );
    assert_eq!(s.patterns[0].rows[0][0].note, 48);
    let x = Module::load(&build_xm()).expect("xm");
    assert_eq!((x.instruments.len(), x.samples.len()), (1, 1));
    assert_eq!(x.patterns[0].rows[0][0].note, 48);
    assert!(x.linear_slides);
    let i = Module::load(&build_it()).expect("it");
    assert_eq!(i.patterns[0].rows[0][0].note, 60);
    assert_eq!(i.channels, 1);
    assert_eq!(i.samples[0].data.len(), 32);
}

#[test]
fn song_ends_after_its_pattern() {
    let mut p = player(&build_mod(None, None), false);
    // 64 rows at speed 6 and tempo 125: 64 * 6 * 960 frames.
    let mut total = 0;
    loop {
        let n = render(&mut p, 48_000).len() / 2;
        total += n;
        if n < 48_000 {
            break;
        }
    }
    assert!(p.control().finished());
    assert_eq!(total, 64 * 6 * 960);
}

#[test]
fn pattern_break_and_speed_effects() {
    // D00 on row 1 breaks to the next order, which ends the song after two rows.
    let mut p = player(&build_mod(None, Some((0xD, 0))), false);
    let total = render(&mut p, RATE as usize * 4).len() / 2;
    assert_eq!(total, 2 * 6 * 960);
    // F03 on row 0 sets speed 3, halving the length.
    let mut p = player(&build_mod(Some((0xF, 3)), Some((0xD, 0))), false);
    let total = render(&mut p, RATE as usize * 4).len() / 2;
    assert_eq!(total, 2 * 3 * 960, "speed 3 applies from row 0");
}

#[test]
fn volume_effect_scales_output() {
    let full = peak(&render(&mut player(&build_mod(None, None), true), 4800));
    let half = peak(&render(
        &mut player(&build_mod(Some((0xC, 0x20)), None), true),
        4800,
    ));
    assert!(full > 0.05);
    assert!((half / full - 0.5).abs() < 0.02, "full {full} half {half}");
}

#[test]
fn seek_clamps_and_restarts() {
    let mut p = player(&build_mod(None, None), false);
    let c = p.control();
    assert_eq!(
        (c.length(), c.rows(0), c.rows(1), c.channels()),
        (1, 64, 0, 4)
    );
    c.seek(0, 32);
    let total = {
        let mut t = 0;
        loop {
            let n = render(&mut p, 48_000).len() / 2;
            t += n;
            if n < 48_000 {
                break t;
            }
        }
    };
    assert_eq!(total, 32 * 6 * 960);
    assert!(c.finished());
    c.seek(0, 0);
    let out = render(&mut p, 4800);
    assert!(!c.finished() && peak(&out) > 0.05);
    assert_eq!(c.position(), (0, 0));
    c.seek(9, 500);
    render(&mut p, 16);
    assert_eq!(c.position(), (0, 63));
}

#[test]
fn mute_and_solo() {
    let mut p = player(&build_mod(None, None), true);
    let c = p.control();
    assert!(peak(&render(&mut p, 4800)) > 0.05);
    c.mute(0, true);
    assert!(c.muted(0) && !c.muted(1));
    assert_eq!(peak(&render(&mut p, 4800)), 0.0);
    c.mute(0, false);
    assert!(peak(&render(&mut p, 4800)) > 0.05);
    c.solo(1, true);
    assert!(c.soloed(1));
    assert_eq!(
        peak(&render(&mut p, 4800)),
        0.0,
        "an empty channel is soloed"
    );
    c.solo(0, true);
    assert!(peak(&render(&mut p, 4800)) > 0.05);
    c.solo(0, false);
    c.solo(1, false);
    c.solo(1, false);
    c.mute(99, true);
    assert!(peak(&render(&mut p, 4800)) > 0.05);
}

#[test]
fn arpeggio_and_retrigger_change_the_pitch_pattern() {
    // Only row 0 carries the effect, so compare that row (six ticks of 960 frames).
    let plain = zero_crossings(&render(&mut player(&build_mod(None, None), true), 5760));
    // 0x37 arpeggio visits the note, +3 and +7 semitones, so the average pitch rises by about 23%.
    let arp = zero_crossings(&render(
        &mut player(&build_mod(Some((0, 0x37)), None), true),
        5760,
    ));
    assert!(
        arp as f32 > plain as f32 * 1.15,
        "plain {plain} arpeggio {arp}"
    );
}

#[test]
fn garbage_and_truncated_files_do_not_panic() {
    for data in [
        vec![],
        vec![0u8; 10],
        b"IMPM".to_vec(),
        b"Extended Module: ".to_vec(),
        vec![b'x'; 2000],
    ] {
        assert!(Module::load(&data).is_err());
    }
    let files = [build_mod(None, None), build_s3m(), build_xm(), build_it()];
    for file in &files {
        for len in (0..file.len()).step_by(7) {
            let _ = Module::load(&file[..len]);
        }
    }
}

#[test]
fn corrupted_modules_play_without_panicking() {
    let files = [
        build_mod(Some((0xE, 0x93)), Some((0xC, 0x10))),
        build_s3m(),
        build_xm(),
        build_it(),
    ];
    let mut seed = 0x1234_5678u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for file in &files {
        for _ in 0..150 {
            let mut data = file.clone();
            for _ in 0..4 {
                let at = next() as usize % data.len();
                data[at] = next() as u8;
            }
            if let Ok(m) = Module::load(&data) {
                let mut p = Player::with_options(
                    Arc::new(m),
                    22_050,
                    PlayerOptions {
                        looping: true,
                        ..Default::default()
                    },
                );
                for _ in 0..3 {
                    let mut out = vec![0.0; 2048];
                    p.read(&mut out);
                    assert!(out.iter().all(|s| s.is_finite()));
                }
            }
        }
    }
}

#[test]
fn hand_built_module_with_missing_data_is_safe() {
    let m = Module {
        channels: 2,
        orders: vec![0, 5],
        speed: 6,
        tempo: 125,
        ..Module::default()
    };
    let mut p = Player::new(Arc::new(m), RATE);
    let mut out = vec![0.0; 1024];
    p.read(&mut out);
    let cell = Cell {
        note: NOTE_NONE,
        effect: Effect::None,
        ..Cell::default()
    };
    assert_eq!(cell.instrument, 0);
}

fn square_sample() -> super::Sample {
    super::Sample {
        data: (0..32).map(|i| if i < 16 { 0.8 } else { -0.8 }).collect(),
        loop_end: 32,
        loop_type: super::LoopType::Forward,
        volume: 64,
        global_volume: 64,
        c4_speed: 8363,
        ..super::Sample::default()
    }
}

/// An IT-style module with one instrument and two notes on the first channel.
fn instrument_module(nna: super::Nna, fadeout: i32) -> Module {
    let mut inst = super::Instrument {
        nna,
        fadeout,
        ..super::Instrument::default()
    };
    inst.sample_map = [0; 120];
    let note = |n: i32| Cell {
        note: n,
        instrument: 1,
        ..Cell::default()
    };
    let rows = vec![
        vec![note(60)],
        vec![note(64)],
        vec![Cell::default()],
        vec![Cell::default()],
    ];
    Module {
        channels: 1,
        samples: vec![square_sample()],
        instruments: vec![inst],
        patterns: vec![super::Pattern { rows }],
        orders: vec![0],
        speed: 6,
        tempo: 125,
        pan: vec![0.0],
        format: Format::It,
        global_volume: 128,
        mix_volume: 128,
        linear_slides: true,
        ..Module::default()
    }
}

#[test]
fn new_note_action_moves_the_old_note_to_the_background() {
    let cases = [
        (super::Nna::Cut, 0),
        (super::Nna::Continue, 1),
        (super::Nna::Off, 1),
        (super::Nna::Fade, 1),
    ];
    for (nna, background) in cases {
        let mut p = Player::new(Arc::new(instrument_module(nna, 0)), RATE);
        render(&mut p, 960 * 6 + 100);
        assert_eq!(p.st.bg.len(), background, "{nna:?}");
    }
}

#[test]
fn faded_background_note_is_released() {
    let mut p = Player::new(Arc::new(instrument_module(super::Nna::Fade, 20_000)), RATE);
    render(&mut p, 960 * 6 * 3);
    assert_eq!(
        p.st.bg.len(),
        0,
        "a fast fadeout frees the background voice"
    );
}

#[test]
fn envelope_interpolates_and_sustains() {
    use super::{EnvPoint, Envelope};
    let env = Envelope {
        points: vec![
            EnvPoint {
                tick: 0,
                value: 0.0,
            },
            EnvPoint {
                tick: 10,
                value: 64.0,
            },
            EnvPoint {
                tick: 20,
                value: 32.0,
            },
        ],
        enabled: true,
        sustain: true,
        sustain_start: 1,
        sustain_end: 1,
        ..Envelope::default()
    };
    let cases = [
        (0, 0.0),
        (5, 32.0),
        (10, 64.0),
        (15, 48.0),
        (20, 32.0),
        (99, 32.0),
    ];
    for (tick, want) in cases {
        assert!((env.value(tick) - want).abs() < 1e-4, "tick {tick}");
    }
    let mut st = super::state::EnvState::default();
    for _ in 0..40 {
        env.advance(&mut st, false);
    }
    assert_eq!(st.tick, 10, "held on the sustain point");
    for _ in 0..40 {
        env.advance(&mut st, true);
    }
    assert!(st.done, "released, it runs to the end");
}
