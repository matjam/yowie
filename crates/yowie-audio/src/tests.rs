//! Offline tests of the mixer: gain, panning, pitch, loops, fades, buses, reverb.

use crate::{Mixer, Pcm, PlayOptions, Renderer, ReverbSettings};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const RATE: u32 = 48_000;

pub(crate) fn setup() -> (Mixer, Renderer) {
    Mixer::offline(RATE).expect("offline mixer")
}

/// A mono sound holding a constant level.
pub(crate) fn dc(mixer: &Mixer, level: f32, frames: u32) -> crate::Sound {
    mixer
        .sound(&Pcm {
            samples: vec![level; frames as usize],
            channels: 1,
            rate: RATE,
        })
        .expect("sound")
}

pub(crate) fn left(out: &[f32]) -> Vec<f32> {
    out.chunks_exact(2).map(|f| f[0]).collect()
}

pub(crate) fn right(out: &[f32]) -> Vec<f32> {
    out.chunks_exact(2).map(|f| f[1]).collect()
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn gain_scales_with_volume_and_centre_pan_law() {
    for volume in [1.0f32, 0.5, 0.25] {
        let (m, mut r) = setup();
        let s = dc(&m, 0.8, RATE);
        m.play(
            &s,
            PlayOptions {
                volume,
                ..Default::default()
            },
        );
        let out = r.render_frames(2048);
        let want = 0.8 * volume * std::f32::consts::FRAC_1_SQRT_2;
        let got = left(&out)[1500];
        assert!(
            (got - want).abs() < 1e-3,
            "volume {volume}: {got} vs {want}"
        );
        assert!((right(&out)[1500] - want).abs() < 1e-3);
    }
}

#[test]
fn pan_law_is_constant_power() {
    for pan in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        let (m, mut r) = setup();
        let s = dc(&m, 1.0, RATE);
        m.play(
            &s,
            PlayOptions {
                pan,
                ..Default::default()
            },
        );
        let out = r.render_frames(2048);
        let (l, rr) = (left(&out)[1800], right(&out)[1800]);
        assert!((l * l + rr * rr - 1.0).abs() < 1e-3, "pan {pan}: {l} {rr}");
        match pan.partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Less) => assert!(l > rr),
            Some(std::cmp::Ordering::Greater) => assert!(rr > l),
            _ => assert!((l - rr).abs() < 1e-3),
        }
    }
    let (m, mut r) = setup();
    m.play(
        &dc(&m, 1.0, RATE),
        PlayOptions {
            pan: -1.0,
            ..Default::default()
        },
    );
    let out = r.render_frames(2048);
    assert!(right(&out)[1800].abs() < 1e-3 && (left(&out)[1800] - 1.0).abs() < 1e-3);
}

#[test]
fn pitch_doubles_frequency() {
    let crossings = |pitch: f32| {
        let (m, mut r) = setup();
        let s = m.sound(&Pcm::sine(100.0, 1.0, RATE)).expect("sound");
        m.play(
            &s,
            PlayOptions {
                pitch,
                ..Default::default()
            },
        );
        let out = left(&r.render_frames(RATE as usize / 4));
        out.windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count()
    };
    let base = crossings(1.0);
    let doubled = crossings(2.0);
    assert!(doubled.abs_diff(base * 2) <= 3, "{base} {doubled}");
}

#[test]
fn sound_ends_and_reports_finished() {
    let (m, mut r) = setup();
    let v = m.play(&dc(&m, 0.5, 1000), PlayOptions::default());
    assert!(v.is_playing() && !v.is_finished());
    let out = r.render_frames(2000);
    assert!(v.is_finished() && !v.is_playing());
    assert!(left(&out)[999].abs() > 0.1 && left(&out)[1100].abs() < 1e-6);
}

#[test]
fn done_callback_runs_on_update() {
    let (m, mut r) = setup();
    let hit = Arc::new(AtomicBool::new(false));
    let v = m.play(&dc(&m, 0.5, 100), PlayOptions::default());
    let h = hit.clone();
    v.on_done(move || h.store(true, Ordering::SeqCst));
    r.render_frames(1000);
    assert!(!hit.load(Ordering::SeqCst));
    m.update();
    assert!(hit.load(Ordering::SeqCst));
}

#[test]
fn loop_points_repeat_the_region() {
    let (m, mut r) = setup();
    let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
    let s = m
        .sound(&Pcm {
            samples: ramp,
            channels: 1,
            rate: RATE,
        })
        .expect("sound");
    let v = m.play(
        &s,
        PlayOptions {
            looping: true,
            loop_points: Some((100.0 / f64::from(RATE), 200.0 / f64::from(RATE))),
            pan: -1.0,
            ..Default::default()
        },
    );
    let out = left(&r.render_frames(2000));
    assert!(v.is_playing());
    assert!(out[1999] < 0.21 && out[1999] >= 0.099, "{}", out[1999]);
    assert!(out[1500] >= 0.099 && out[1500] < 0.21);
}

#[test]
fn stop_ramps_without_click() {
    let (m, mut r) = setup();
    let v = m.play(
        &m.sound(&Pcm::sine(200.0, 2.0, RATE)).expect("sound"),
        PlayOptions::default(),
    );
    let mut out = r.render_frames(2000);
    v.stop();
    out.extend(r.render_frames(2000));
    let l = left(&out);
    assert!(!v.is_playing() && v.is_finished());
    assert!(l[3999].abs() < 1e-6, "silent after the ramp");
    let tone_step = max_step(&l[..2000]);
    assert!(
        max_step(&l[1900..]) < tone_step * 1.2 + 0.01,
        "stop clicked: {}",
        max_step(&l[1900..])
    );
}

#[test]
fn fade_out_is_smooth_and_ends_voice() {
    let (m, mut r) = setup();
    let v = m.play(
        &dc(&m, 1.0, RATE * 2),
        PlayOptions {
            pan: -1.0,
            ..Default::default()
        },
    );
    r.render_frames(512);
    v.fade_out(0.1);
    let l = left(&r.render_frames(RATE as usize / 5));
    assert!(v.is_finished());
    assert!(max_step(&l) < 0.02, "fade stepped by {}", max_step(&l));
    assert!(
        l.windows(2).filter(|w| w[1] > w[0] + 1e-4).count() < 3,
        "fade is monotonic"
    );
}

#[test]
fn fade_in_starts_from_silence() {
    let (m, mut r) = setup();
    m.play(
        &dc(&m, 1.0, RATE),
        PlayOptions {
            fade_in: 0.1,
            pan: -1.0,
            ..Default::default()
        },
    );
    let l = left(&r.render_frames(RATE as usize / 5));
    assert!(l[0].abs() < 0.01 && l[l.len() - 1] > 0.99);
    assert!(max_step(&l) < 0.01);
}

#[test]
fn volume_changes_ramp_across_a_chunk() {
    let (m, mut r) = setup();
    let v = m.play(
        &dc(&m, 1.0, RATE),
        PlayOptions {
            pan: -1.0,
            ..Default::default()
        },
    );
    r.render_frames(1024);
    v.set_volume(0.0);
    let l = left(&r.render_frames(1024));
    assert!(max_step(&l) < 0.02, "{}", max_step(&l));
    assert!(l[1023].abs() < 1e-3);
}

#[test]
fn voice_limit_steals_by_priority() {
    let (m, mut r) = setup();
    m.set_max_voices(2);
    let long = dc(&m, 0.1, RATE);
    let a = m.play(
        &long,
        PlayOptions {
            priority: 5,
            ..Default::default()
        },
    );
    let b = m.play(
        &long,
        PlayOptions {
            priority: 1,
            ..Default::default()
        },
    );
    let low = m.play(
        &long,
        PlayOptions {
            priority: 0,
            ..Default::default()
        },
    );
    r.render_frames(512);
    assert!(low.is_finished(), "lowest priority is refused");
    let high = m.play(
        &long,
        PlayOptions {
            priority: 3,
            ..Default::default()
        },
    );
    r.render_frames(512);
    assert!(b.is_finished() && !a.is_finished() && !high.is_finished());
}

#[test]
fn bus_volume_mute_and_solo() {
    let (m, mut r) = setup();
    let bus = m.new_bus("sfx2").expect("bus");
    m.play(
        &dc(&m, 1.0, RATE),
        PlayOptions {
            bus: Some(bus.clone()),
            pan: -1.0,
            ..Default::default()
        },
    );
    bus.set_volume(0.5);
    let l = left(&r.render_frames(2048));
    assert!((l[2000] - 0.5).abs() < 1e-3);
    bus.set_mute(true);
    let l = left(&r.render_frames(2048));
    assert!(l[2000].abs() < 1e-4);
    bus.set_mute(false);
    m.music().set_solo(true);
    let l = left(&r.render_frames(2048));
    assert!(l[2000].abs() < 1e-4, "other buses are silent under a solo");
    assert!(m.new_bus("sfx2").expect("again").name() == "sfx2");
}

#[test]
fn bus_low_pass_darkens_the_bus() {
    let level = |cutoff: f32| {
        let (m, mut r) = setup();
        let bus = m.effects();
        bus.set_low_pass(cutoff);
        m.play(
            &m.sound(&Pcm::sine(8000.0, 1.0, RATE)).expect("sound"),
            PlayOptions {
                bus: Some(bus),
                ..Default::default()
            },
        );
        let out = left(&r.render_frames(8192));
        out[4096..].iter().map(|v| v * v).sum::<f32>()
    };
    assert!(level(500.0) < level(0.0) * 0.01);
}

#[test]
fn reverb_send_leaves_a_decaying_tail() {
    let (m, mut r) = setup();
    m.set_reverb(ReverbSettings {
        room_size: 0.6,
        wet: 0.5,
        ..Default::default()
    });
    let click = m
        .sound(&Pcm {
            samples: vec![1.0; 64],
            channels: 1,
            rate: RATE,
        })
        .expect("sound");
    m.play(
        &click,
        PlayOptions {
            reverb: 1.0,
            ..Default::default()
        },
    );
    let out = r.render_frames(RATE as usize * 2);
    let e = |a: usize, b: usize| out[a * 2..b * 2].iter().map(|v| v * v).sum::<f32>();
    let early = e(2000, 12_000);
    let late = e(80_000, 90_000);
    assert!(
        early > 1e-4 && late < early * 0.5,
        "early {early} late {late}"
    );
    assert!(out.iter().all(|v| v.abs() <= 1.0));
}

#[test]
fn output_is_clamped_and_nan_is_silenced() {
    use crate::Stream;
    struct Bad;
    impl Stream for Bad {
        fn read(&mut self, out: &mut [f32]) -> usize {
            out.fill(f32::NAN);
            out.len() / 2
        }
    }
    let (m, mut r) = setup();
    m.play_stream(Bad, PlayOptions::default());
    m.play(
        &dc(&m, 1.0, RATE),
        PlayOptions {
            volume: 8.0,
            ..Default::default()
        },
    );
    let out = r.render_frames(512);
    assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
}

#[test]
fn stream_voice_plays_and_seeks() {
    use crate::Stream;
    struct Counter(usize);
    impl Stream for Counter {
        fn read(&mut self, out: &mut [f32]) -> usize {
            for f in out.chunks_exact_mut(2) {
                f[0] = (self.0 % 100) as f32 / 1000.0;
                f[1] = f[0];
                self.0 += 1;
            }
            out.len() / 2
        }
    }
    let (m, mut r) = setup();
    let v = m.play_stream(
        Counter(0),
        PlayOptions {
            pan: -1.0,
            ..Default::default()
        },
    );
    let out = left(&r.render_frames(2000));
    assert!(out[1500] > 0.0 || out[1501] > 0.0);
    assert!(
        (v.position() - 2000.0 / f64::from(RATE)).abs() < 0.002,
        "{}",
        v.position()
    );
}
