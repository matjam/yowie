//! Birth: choosing a new particle's position, velocity and look.
//!
//! These functions take only the emitter and a random stream, so the CPU
//! [`System`](crate::System) and a GPU-instanced system share them.

use std::f32::consts::TAU;

use yowie_core::{Color, Vec2, vec2};
use yowie_rng::Rng;

use crate::emitter::{Emitter, Shape};

/// One live particle. Positions are world units for a world-space system
/// and relative to the system's position otherwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// Where it is.
    pub position: Vec2,
    /// How fast it moves, in units per second.
    pub velocity: Vec2,
    /// Where it was born; the centre for radial and tangential
    /// acceleration.
    pub origin: Vec2,
    /// Seconds alive.
    pub age: f32,
    /// Seconds it lives.
    pub life: f32,
    /// Width at birth.
    pub size: f32,
    /// Rotation in radians.
    pub rotation: f32,
    /// Rotation speed in radians per second.
    pub spin: f32,
    /// The palette colour it was born with, white without a palette.
    pub tint: Color,
}

impl Particle {
    /// Normalised age: 0 at birth, 1 at death.
    pub fn t(&self) -> f32 {
        (self.age / self.life).clamp(0.0, 1.0)
    }
}

/// A birth point and flight direction for `e.shape`, relative to the
/// system's position.
pub(crate) fn spawn_point(e: &Emitter, rng: &mut Rng) -> (Vec2, f32) {
    let spread = (rng.float() - 0.5) * e.spread;
    match e.shape {
        Shape::Point => (Vec2::ZERO, e.direction + spread),
        Shape::Circle { radius, edge } => {
            let a = rng.float() * TAU;
            let d = if edge {
                radius
            } else {
                radius * rng.float().sqrt()
            };
            (Vec2::from_angle(a) * d, e.direction + spread)
        }
        Shape::Rect { w, h, edge } => {
            let p = if edge {
                // Walk the border by length so every side is equally dense.
                let t = rng.float() * 2.0 * (w + h);
                if t < w {
                    vec2(t - w / 2.0, -h / 2.0)
                } else if t < w + h {
                    vec2(w / 2.0, t - w - h / 2.0)
                } else if t < 2.0 * w + h {
                    vec2(w / 2.0 - (t - w - h), h / 2.0)
                } else {
                    vec2(-w / 2.0, h / 2.0 - (t - 2.0 * w - h))
                }
            } else {
                vec2((rng.float() - 0.5) * w, (rng.float() - 0.5) * h)
            };
            (p, e.direction + spread)
        }
        Shape::Line { to } => (to * rng.float(), e.direction + spread),
        Shape::Cone { angle, base_radius } => {
            let a = rng.float() * TAU;
            let d = base_radius * rng.float().sqrt();
            (
                Vec2::from_angle(a) * d,
                e.direction + (rng.float() - 0.5) * angle.clamp(0.0, TAU),
            )
        }
    }
}

/// Makes a particle. `origin` is the system's position for a world-space
/// emitter and zero otherwise. `default_width` is used when the emitter's
/// size range is zero.
pub(crate) fn birth(e: &Emitter, rng: &mut Rng, origin: Vec2, default_width: f32) -> Particle {
    let (offset, angle) = spawn_point(e, rng);
    let position = origin + offset;
    let speed = e.speed.pick(rng);
    let life = e.lifetime.pick(rng);
    let size = e.size.pick(rng);
    Particle {
        position,
        velocity: Vec2::from_angle(angle) * speed,
        origin: position,
        age: 0.0,
        life: if life > 0.0 { life } else { 1.0 },
        size: if size > 0.0 { size } else { default_width },
        rotation: e.rotation.pick(rng),
        spin: e.spin.pick(rng),
        tint: rng.choose(&e.palette).copied().unwrap_or(Color::WHITE),
    }
}
