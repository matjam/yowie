//! The clips the animation example plays, written in code. A clip is plain
//! data: named tracks of keyed values.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use yowie::anim::{Animatable, Channel, Clip, ClipSet, Curve, LoopMode};
use yowie::tween::{Curve as Shape, Ease};
use yowie::{Color, Quat, Vec2, Vec3, vec2, vec3};

/// Keys that run from `from` at `t0` to `to` at `t1` along `ease`. A curve
/// interpolates linearly between keys, so an eased move is sampled into
/// several.
fn eased<T: Animatable>(from: &T, to: &T, t0: f32, t1: f32, ease: Ease) -> Vec<(f32, T)> {
    const STEPS: usize = 10;
    (0..=STEPS)
        .map(|i| {
            let u = i as f32 / STEPS as f32;
            (t0 + (t1 - t0) * u, from.lerp(to, ease.apply(u)))
        })
        .collect()
}

/// Joins key runs end to end; each run after the first drops its first key,
/// which repeats the previous run's last.
fn join<T>(runs: Vec<Vec<(f32, T)>>) -> Vec<(f32, T)> {
    let mut out = Vec::new();
    for (i, run) in runs.into_iter().enumerate() {
        out.extend(run.into_iter().skip(usize::from(i > 0)));
    }
    out
}

fn constant<T: Clone>(value: T, end: f32) -> Vec<(f32, T)> {
    vec![(0.0, value.clone()), (end, value)]
}

/// A sprite that rises, falls and bounces, and shifts colour on the way.
pub fn bounce() -> Arc<Clip> {
    let up = eased(
        &Vec2::ZERO,
        &vec2(0.0, -90.0),
        0.0,
        0.6,
        Ease::Out(Shape::Quad),
    );
    let down = eased(
        &vec2(0.0, -90.0),
        &Vec2::ZERO,
        0.6,
        1.2,
        Ease::Out(Shape::Bounce),
    );
    let warm = Color::rgb8(255, 120, 80);
    let gold = Color::rgb8(255, 230, 120);
    Arc::new(
        Clip::new("bounce")
            .with_track(
                "position",
                Channel::Vec2(Curve::linear(join(vec![up, down]))),
            )
            .with_track(
                "tint",
                Channel::Color(Curve::linear([(0.0, warm), (0.6, gold), (1.2, warm)])),
            ),
    )
}

/// A sprite that swells and turns, then reverses.
pub fn pulse() -> Arc<Clip> {
    let size = eased(
        &vec2(30.0, 30.0),
        &vec2(90.0, 90.0),
        0.0,
        0.8,
        Ease::InOut(Shape::Sine),
    );
    Arc::new(
        Clip::new("pulse")
            .with_mode(LoopMode::PingPong)
            .with_track("size", Channel::Vec2(Curve::linear(size)))
            .with_track(
                "rotation",
                Channel::Float(Curve::linear([(0.0, 0.0), (0.8, PI / 2.0)])),
            ),
    )
}

/// A cube that circles the scene at angle `phase`, bobbing and tumbling.
pub fn orbit(phase: f32) -> Arc<Clip> {
    const KEYS: usize = 16;
    let axis = vec3(1.0, 1.0, 0.0).normalize();
    let position: Vec<(f32, Vec3)> = (0..=KEYS)
        .map(|i| {
            let u = i as f32 / KEYS as f32;
            let a = phase + u * TAU;
            (
                u * 4.0,
                vec3(4.2 * a.cos(), 2.3 + 0.35 * (2.0 * a).sin(), 4.2 * a.sin()),
            )
        })
        .collect();
    let tumble: Vec<(f32, Quat)> = (0..=8)
        .map(|i| {
            (
                i as f32 * 0.5,
                Quat::from_axis_angle(axis, i as f32 * PI / 4.0),
            )
        })
        .collect();
    Arc::new(
        Clip::new("orbit")
            .with_track("translation", Channel::Translation(Curve::linear(position)))
            .with_track("rotation", Channel::Rotation(Curve::linear(tumble))),
    )
}

const REST: Vec3 = vec3(0.0, 0.5, 0.0);

/// The hero breathing in place.
pub fn idle() -> Arc<Clip> {
    let sine = Ease::InOut(Shape::Sine);
    let up = vec3(0.0, 0.8, 0.0);
    let bob = join(vec![
        eased(&REST, &up, 0.0, 1.0, sine),
        eased(&up, &REST, 1.0, 2.0, sine),
    ]);
    let swell = vec3(1.05, 0.95, 1.05);
    let squash = join(vec![
        eased(&Vec3::ONE, &swell, 0.0, 1.0, sine),
        eased(&swell, &Vec3::ONE, 1.0, 2.0, sine),
    ]);
    Arc::new(
        Clip::new("idle")
            .with_track("translation", Channel::Translation(Curve::linear(bob)))
            .with_track("scale", Channel::Scale(Curve::linear(squash)))
            .with_track(
                "rotation",
                Channel::Rotation(Curve::linear(constant(Quat::IDENTITY, 2.0))),
            ),
    )
}

/// A crouch, a leap and a landing. Plays once and then reports `Finished`.
pub fn jump() -> Arc<Clip> {
    let apex = vec3(0.0, 3.0, 0.0);
    let height = join(vec![
        eased(&REST, &apex, 0.0, 0.4, Ease::Out(Shape::Quad)),
        eased(&apex, &REST, 0.4, 0.8, Ease::In(Shape::Quad)),
        constant(REST, 1.0),
    ]);
    let squash = Curve::linear([
        (0.0, vec3(1.3, 0.7, 1.3)),
        (0.2, vec3(0.8, 1.4, 0.8)),
        (0.8, vec3(1.2, 0.8, 1.2)),
        (1.0, Vec3::ONE),
    ]);
    Arc::new(
        Clip::new("jump")
            .with_mode(LoopMode::Once)
            .with_track("translation", Channel::Translation(Curve::linear(height)))
            .with_track("scale", Channel::Scale(squash))
            .with_track(
                "rotation",
                Channel::Rotation(Curve::linear(constant(Quat::IDENTITY, 1.0))),
            ),
    )
}

/// One full turn about the vertical axis that overshoots before it settles.
pub fn spin() -> Arc<Clip> {
    const STEPS: usize = 16;
    let turn: Vec<(f32, Quat)> = (0..=STEPS)
        .map(|i| {
            let u = i as f32 / STEPS as f32;
            let angle = TAU * Ease::Out(Shape::Back).apply(u);
            (u, Quat::from_rotation_y(angle))
        })
        .collect();
    Arc::new(
        Clip::new("spin")
            .with_mode(LoopMode::Once)
            .with_track("rotation", Channel::Rotation(Curve::linear(turn)))
            .with_track(
                "translation",
                Channel::Translation(Curve::linear(constant(REST, 1.0))),
            )
            .with_track(
                "scale",
                Channel::Scale(Curve::linear(constant(Vec3::ONE, 1.0))),
            ),
    )
}

fn about_z(degrees: f32) -> Quat {
    Quat::from_rotation_z(degrees.to_radians())
}

/// The two-second swing of a robot arm, with an event named `hit` at the
/// bottom of the stroke.
pub fn swing() -> Arc<Clip> {
    let rock =
        |a: f32, b: f32| Curve::linear([(0.0, about_z(a)), (1.0, about_z(b)), (2.0, about_z(a))]);
    Arc::new(
        Clip::new("swing")
            .with_track("shoulder", Channel::Rotation(rock(-35.0, 35.0)))
            .with_track("elbow", Channel::Rotation(rock(20.0, -50.0)))
            .with_event(1.0, "hit"),
    )
}

/// A quicker, wider rocking of the same joints, one second long.
pub fn stride() -> Arc<Clip> {
    let rock =
        |a: f32, b: f32| Curve::linear([(0.0, about_z(a)), (0.5, about_z(b)), (1.0, about_z(a))]);
    Arc::new(
        Clip::new("stride")
            .with_track("shoulder", Channel::Rotation(rock(-70.0, 60.0)))
            .with_track("elbow", Channel::Rotation(rock(45.0, -90.0))),
    )
}

/// The arm clips by name, for blend spaces.
pub fn arm_clips() -> ClipSet {
    [swing(), stride()]
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect()
}
