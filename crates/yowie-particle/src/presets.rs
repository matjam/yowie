//! Ready-made emitters to use as they are or to start tuning from.
//!
//! Positions and sizes suit a view a few hundred units across, and the
//! effects expect the soft circle from [`soft_circle`] (or any round
//! texture) set on the system.

use std::f32::consts::{FRAC_PI_2, TAU};

use yowie_core::{Color, vec2};
use yowie_gfx::image::{Rgba, RgbaImage};

use crate::curve::{ColorKey, Curve, Range};
use crate::emitter::{Blend, Emitter, Shape};

/// A white disc of `size` pixels whose alpha falls smoothly to zero at the
/// rim, for fire, smoke and glows.
pub fn soft_circle(size: u32) -> RgbaImage {
    let r = size as f32 / 2.0;
    RgbaImage::from_fn(size, size, |x, y| {
        let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
        let a = (1.0 - (dx * dx + dy * dy).sqrt() / r).clamp(0.0, 1.0);
        let a = a * a * (3.0 - 2.0 * a);
        Rgba([255, 255, 255, (a * 255.0 + 0.5) as u8])
    })
}

/// A flame: additive, rising, from warm white to dark red.
pub fn fire() -> Emitter {
    Emitter {
        rate: 90.0,
        lifetime: Range::new(0.6, 1.2),
        shape: Shape::Circle {
            radius: 10.0,
            edge: false,
        },
        direction: -FRAC_PI_2,
        spread: 0.6,
        speed: Range::new(40.0, 90.0),
        gravity: vec2(0.0, -50.0),
        drag: 0.3,
        size: Range::new(14.0, 26.0),
        size_over_life: Curve::keys(&[(0.0, 0.5), (0.25, 1.0), (1.0, 0.15)]),
        color_over_life: vec![
            ColorKey {
                t: 0.0,
                color: Color::rgb8(255, 240, 180),
            },
            ColorKey {
                t: 0.3,
                color: Color::rgb8(255, 160, 40),
            },
            ColorKey {
                t: 0.7,
                color: Color::rgb8(200, 50, 10),
            },
            ColorKey {
                t: 1.0,
                color: Color::rgb8(60, 10, 0),
            },
        ],
        alpha_over_life: Curve::keys(&[(0.0, 0.0), (0.1, 1.0), (1.0, 0.0)]),
        blend: Blend::Additive,
        max: 400,
        ..Emitter::default()
    }
}

/// Smoke: grey puffs that grow, drift and fade, left behind in the world.
pub fn smoke() -> Emitter {
    Emitter {
        rate: 22.0,
        lifetime: Range::new(1.6, 3.0),
        shape: Shape::Circle {
            radius: 8.0,
            edge: false,
        },
        direction: -FRAC_PI_2,
        spread: 0.7,
        speed: Range::new(18.0, 40.0),
        gravity: vec2(6.0, -14.0),
        size: Range::new(16.0, 28.0),
        size_over_life: Curve::linear(0.6, 2.4),
        spin: Range::new(-0.6, 0.6),
        color: Color::rgb8(110, 110, 115),
        color_end: Some(Color::rgb8(60, 60, 65)),
        alpha_over_life: Curve::keys(&[(0.0, 0.0), (0.15, 0.35), (1.0, 0.0)]),
        world_space: true,
        max: 200,
        ..Emitter::default()
    }
}

/// Sparks: a burst of 40 bright specks thrown in every direction that fall
/// under gravity.
pub fn sparks() -> Emitter {
    Emitter {
        burst: 40,
        lifetime: Range::new(0.3, 0.9),
        spread: TAU,
        speed: Range::new(120.0, 340.0),
        gravity: vec2(0.0, 420.0),
        drag: 1.2,
        size: Range::new(2.0, 4.0),
        color: Color::rgb8(255, 245, 200),
        color_end: Some(Color::rgb8(255, 120, 30)),
        alpha_over_life: Curve::keys(&[(0.0, 1.0), (0.6, 1.0), (1.0, 0.0)]),
        blend: Blend::Additive,
        world_space: true,
        max: 600,
        ..Emitter::default()
    }
}

/// Rain: thin slanted streaks born along a 800-unit line. Move or retarget
/// the line by changing the shape's `to`.
pub fn rain() -> Emitter {
    let slant = 0.12;
    Emitter {
        rate: 350.0,
        lifetime: Range::new(1.4, 1.8),
        shape: Shape::Line {
            to: vec2(800.0, 0.0),
        },
        direction: FRAC_PI_2 - slant,
        spread: 0.03,
        speed: Range::new(520.0, 680.0),
        size: Range::new(1.5, 2.5),
        aspect: 7.0,
        rotation: Range::constant(slant),
        color: Color::rgba8(170, 200, 255, 170),
        world_space: true,
        max: 1200,
        ..Emitter::default()
    }
}

/// Confetti: a burst of 150 coloured flakes thrown upwards that tumble
/// down.
pub fn confetti() -> Emitter {
    Emitter {
        burst: 150,
        lifetime: Range::new(1.6, 2.6),
        direction: -FRAC_PI_2,
        spread: 1.6,
        speed: Range::new(220.0, 480.0),
        gravity: vec2(0.0, 520.0),
        drag: 1.4,
        size: Range::new(5.0, 9.0),
        aspect: 0.55,
        rotation: Range::new(0.0, TAU),
        spin: Range::new(-9.0, 9.0),
        alpha_over_life: Curve::keys(&[(0.0, 1.0), (0.75, 1.0), (1.0, 0.0)]),
        palette: vec![
            Color::rgb8(255, 80, 90),
            Color::rgb8(255, 200, 40),
            Color::rgb8(70, 220, 120),
            Color::rgb8(70, 150, 255),
            Color::rgb8(220, 90, 255),
            Color::rgb8(255, 255, 255),
        ],
        world_space: true,
        max: 600,
        ..Emitter::default()
    }
}
