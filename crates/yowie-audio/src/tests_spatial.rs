//! Offline tests of positional audio: distance, binaural delay, occlusion and Doppler.

use crate::tests::{RATE, dc, left, right, setup};
use crate::{Pcm, PlayOptions, Positional, SpatialSettings};
use yowie_core::Vec3;

#[test]
fn positional_voice_pans_toward_its_side_and_fades_with_distance() {
    let level = |pos: Vec3| {
        let (m, mut r) = setup();
        m.play(
            &dc(&m, 0.5, RATE),
            PlayOptions {
                positional: Some(Positional::at(pos)),
                ..Default::default()
            },
        );
        let out = r.render_frames(4096);
        (left(&out)[4000], right(&out)[4000])
    };
    let (l, rr) = level(Vec3::new(3.0, 0.0, 0.0));
    assert!(rr > l * 2.0, "{l} {rr}");
    let (nl, nr) = level(Vec3::new(0.0, 0.0, -2.0));
    let (fl, fr) = level(Vec3::new(0.0, 0.0, -20.0));
    assert!(fl + fr < (nl + nr) * 0.3);
    let (zl, zr) = level(Vec3::new(0.0, 0.0, -500.0));
    assert!(zl.abs() + zr.abs() < 1e-4);
}

#[test]
fn binaural_delays_the_far_ear() {
    let (m, mut r) = setup();
    m.set_spatial(SpatialSettings {
        binaural: true,
        head_radius: 0.0,
    });
    let mut samples = vec![0.0; 400];
    samples[3] = 1.0;
    let click = m
        .sound(&Pcm {
            samples,
            channels: 1,
            rate: RATE,
        })
        .expect("sound");
    m.play(
        &click,
        PlayOptions {
            positional: Some(Positional::at(Vec3::new(2.0, 0.0, 0.0))),
            ..Default::default()
        },
    );
    let out = r.render_frames(512);
    let first_over = |x: &[f32]| x.iter().position(|v| v.abs() > 0.02).unwrap_or(usize::MAX);
    let (fl, fr) = (first_over(&left(&out)), first_over(&right(&out)));
    assert!(
        fl > fr,
        "a source on the right reaches the left ear later: left {fl} right {fr}"
    );
    assert!(
        (20..=45).contains(&(fl - fr)),
        "delay of {} frames",
        fl - fr
    );
}

#[test]
fn binaural_delay_flips_with_the_source_side() {
    let (m, mut r) = setup();
    m.set_spatial(SpatialSettings {
        binaural: true,
        head_radius: 0.0,
    });
    let mut samples = vec![0.0; 400];
    samples[3] = 1.0;
    let click = m
        .sound(&Pcm {
            samples,
            channels: 1,
            rate: RATE,
        })
        .expect("sound");
    m.play(
        &click,
        PlayOptions {
            positional: Some(Positional::at(Vec3::new(-2.0, 0.0, 0.0))),
            ..Default::default()
        },
    );
    let out = r.render_frames(512);
    let first_over = |x: &[f32]| x.iter().position(|v| v.abs() > 0.02).unwrap_or(usize::MAX);
    assert!(first_over(&right(&out)) > first_over(&left(&out)));
}

#[test]
fn occlusion_dims_and_muffles() {
    let energy = |occ: f32| {
        let (m, mut r) = setup();
        m.play(
            &m.sound(&Pcm::sine(5000.0, 1.0, RATE)).expect("sound"),
            PlayOptions {
                occlusion: occ,
                ..Default::default()
            },
        );
        let out = left(&r.render_frames(8192));
        out[4096..].iter().map(|v| v * v).sum::<f32>()
    };
    assert!(energy(1.0) < energy(0.0) * 0.001);
    assert!(energy(0.5) < energy(0.0));
}

#[test]
fn doppler_raises_pitch_for_an_approaching_source() {
    let crossings = |vel: Vec3| {
        let (m, mut r) = setup();
        m.set_doppler(1.0);
        let s = m.sound(&Pcm::sine(200.0, 2.0, RATE)).expect("sound");
        m.play(
            &s,
            PlayOptions {
                positional: Some(Positional {
                    velocity: vel,
                    ..Positional::at(Vec3::new(0.0, 0.0, -30.0))
                }),
                ..Default::default()
            },
        );
        let out = left(&r.render_frames(RATE as usize / 2));
        out.windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count()
    };
    assert!(crossings(Vec3::new(0.0, 0.0, 60.0)) > crossings(Vec3::ZERO) + 10);
}
