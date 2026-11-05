//! 300 tinted, rotating, alpha-blended sprites drawn from a texture made at
//! start-up, over and under a few shapes. Escape quits.

use std::f32::consts::TAU;

use yowie::gfx::{BlendMode, Sprite, Texture, TextureOptions};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

/// A small deterministic generator, so every run draws the same frame.
struct XorShift(u32);

impl XorShift {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

struct Item {
    start: Vec2,
    velocity: Vec2,
    spin: f32,
    size: f32,
    tint: Color,
}

struct Sprites {
    gem: Texture,
    items: Vec<Item>,
}

/// A soft-edged gem: a bright core, a dark ring and four points, with
/// alpha falling off towards the edge.
fn gem_pixels(size: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    let c = (size as f32 - 1.0) * 0.5;
    for y in 0..size {
        for x in 0..size {
            let p = vec2(x as f32 - c, y as f32 - c) / c;
            let d = p.length();
            let star = (p.x.abs() * p.y.abs()).sqrt();
            let shape = d + star * 0.8;
            let alpha = ((1.0 - shape) / 0.15).clamp(0.0, 1.0);
            let ring = if (0.45..0.55).contains(&d) { 0.55 } else { 1.0 };
            let light = (1.0 - d * 0.5) * ring;
            let v = (light * 255.0) as u8;
            px.extend_from_slice(&[v, v, v, (alpha * 255.0) as u8]);
        }
    }
    px
}

impl Game for Sprites {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let gem =
            Texture::from_pixels(&ctx.gfx, 64, 64, &gem_pixels(64), TextureOptions::smooth())?;
        let view = ctx.gfx.view_size();
        let mut rng = XorShift(0x9e37_79b9);
        let items = (0..300)
            .map(|_| {
                let angle = rng.range(0.0, TAU);
                Item {
                    start: vec2(rng.range(0.0, view.x), rng.range(0.0, view.y)),
                    velocity: Vec2::from_angle(angle) * rng.range(20.0, 120.0),
                    spin: rng.range(-3.0, 3.0),
                    size: rng.range(16.0, 56.0),
                    tint: Color::hsv(rng.range(0.0, 360.0), 0.7, 1.0)
                        .with_alpha(rng.range(0.5, 0.9)),
                }
            })
            .collect();
        Ok(Sprites { gem, items })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let t = ctx.time.elapsed() as f32 + ctx.time.alpha() * ctx.time.delta();
        let view = ctx.gfx.view_size();
        let g = &mut ctx.gfx;

        // Shapes behind the sprites.
        g.with_layer(-1, |g| {
            g.fill_circle(
                vec2(view.x * 0.5, view.y * 0.5),
                view.y * 0.35,
                Color::rgb8(40, 50, 90),
            );
            g.stroke_circle(
                vec2(view.x * 0.5, view.y * 0.5),
                view.y * 0.42,
                6.0,
                Color::rgb8(90, 110, 200),
            );
            let hex: Vec<Vec2> = (0..6)
                .map(|i| {
                    vec2(150.0, view.y - 120.0)
                        + Vec2::from_angle(i as f32 * TAU / 6.0 + t * 0.3) * 80.0
                })
                .collect();
            g.fill_polygon(&hex, Color::rgb8(120, 60, 40));
            g.fill_triangle(
                vec2(view.x - 220.0, view.y - 40.0),
                vec2(view.x - 40.0, view.y - 40.0),
                vec2(view.x - 130.0, view.y - 200.0),
                Color::rgb8(40, 110, 70),
            );
        });

        for item in &self.items {
            let span = view + Vec2::splat(64.0);
            let p = item.start + item.velocity * t;
            let pos = vec2(p.x.rem_euclid(span.x), p.y.rem_euclid(span.y)) - Vec2::splat(32.0);
            let sprite = Sprite::at(pos)
                .size(Vec2::splat(item.size))
                .centered()
                .rotation(item.spin * t)
                .tint(item.tint);
            g.draw_sprite(&self.gem, &sprite);
        }

        // Shapes in front: an additive glow, a frame and a zigzag.
        g.with_layer(1, |g| {
            g.with_blend(BlendMode::Additive, |g| {
                g.fill_circle(
                    vec2(view.x - 120.0, 110.0),
                    70.0,
                    Color::rgba(1.0, 0.6, 0.2, 0.35),
                );
            });
            g.stroke_rect(
                Rect::new(10.0, 10.0, view.x - 20.0, view.y - 20.0),
                4.0,
                Color::rgb8(230, 230, 240),
            );
            let zigzag: Vec<Vec2> = (0..9)
                .map(|i| {
                    vec2(
                        40.0 + i as f32 * 30.0,
                        50.0 + if i % 2 == 0 { 0.0 } else { 30.0 },
                    )
                })
                .collect();
            g.stroke_polyline(&zigzag, 5.0, Color::rgb8(250, 220, 90));
            g.line(vec2(40.0, 120.0), vec2(280.0, 140.0), 3.0, Color::WHITE);
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(14, 16, 28),
        ..Config::new("Yowie sprites").with_size(960, 540)
    };
    yowie::run::<Sprites>(config)
}
