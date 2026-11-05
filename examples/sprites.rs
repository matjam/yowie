//! 300 tinted, rotating, alpha-blended sprites drawn from a texture made at
//! start-up, over and under a few shapes, and a lit brick floor where a
//! moving lamp throws shadows from three crates. Escape quits.

use std::f32::consts::TAU;

use yowie::gfx::{BlendMode, Light2D, Sprite, Texture, TextureOptions};
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
    floor: LitFloor,
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
        let floor = LitFloor::new(&ctx.gfx)?;
        Ok(Sprites { gem, items, floor })
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

        self.floor.draw(g, t);

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

// ---- The lit brick floor ------------------------------------------------
//
// A floor of bricks, drawn with a normal map so each brick catches the
// lamp from the side it faces, and three crates that block the lamp. The
// lamp circles the crates; every frame the example passes the lamp to
// `set_lights_2d` and each crate to `add_occluder_sprite`, then draws the
// floor with `draw_lit`. Crates are ordinary sprites tinted by hand, so lit
// and unlit sprites share one frame.

const FLOOR_W: u32 = 340;
const FLOOR_H: u32 = 192;
const CRATE: f32 = 40.0;
/// Crate centres relative to the floor's corner.
const CRATES: [Vec2; 3] = [vec2(95.0, 84.0), vec2(190.0, 118.0), vec2(235.0, 58.0)];

struct LitFloor {
    bricks: Texture,
    normals: Texture,
    crate_tex: Texture,
}

/// A hash of two integers, 0 to 1.
fn noise(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 13;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The brick pattern at a pixel: its height (mortar is low) and colour.
fn brick(x: i32, y: i32) -> (f32, [f32; 3]) {
    const BW: i32 = 34;
    const BH: i32 = 16;
    let row = y.div_euclid(BH);
    let shift = if row % 2 == 0 { 0 } else { BW / 2 };
    let col = (x + shift).div_euclid(BW);
    let (u, v) = ((x + shift).rem_euclid(BW), y.rem_euclid(BH));
    let edge = u.min(BW - 1 - u).min(v).min(BH - 1 - v);
    let grain = noise(col, row);
    let speck = noise(x, y) * 0.08;
    let base = [0.55 + grain * 0.25, 0.26 + grain * 0.1, 0.18 + grain * 0.06];
    if edge < 2 {
        (0.0, [0.20 + speck, 0.19 + speck, 0.18 + speck])
    } else {
        let bevel = (edge as f32 - 2.0).min(2.0) / 2.0;
        (0.5 + bevel * 0.5, base.map(|c| c + speck))
    }
}

/// Brick colours and a tangent-space normal map from the same heights.
fn floor_images() -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (FLOOR_W as i32, FLOOR_H as i32);
    let mut colors = Vec::with_capacity((w * h * 4) as usize);
    let mut normals = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (_, c) = brick(x, y);
            colors.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8));
            colors.push(255);
            let slope = |a: i32, b: i32, c2: i32, d: i32| brick(a, b).0 - brick(c2, d).0;
            let (dx, dy) = (slope(x + 1, y, x - 1, y), slope(x, y + 1, x, y - 1));
            let n = yowie::Vec3::new(-dx * 1.4, dy * 1.4, 1.0).normalize();
            normals.extend([n.x, n.y, n.z].map(|v| ((v * 0.5 + 0.5) * 255.0) as u8));
            normals.push(255);
        }
    }
    (colors, normals)
}

/// A wooden crate: planks, a dark frame and a diagonal brace.
fn crate_pixels(size: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    let s = size as i32;
    for y in 0..s {
        for x in 0..s {
            let edge = x.min(y).min(s - 1 - x).min(s - 1 - y);
            let brace = (x - y).abs() < 3 || (x + y - (s - 1)).abs() < 3;
            let plank = noise(x / 3, y) * 0.12;
            let (r, g, b) = if edge < 3 {
                (0.34, 0.2, 0.1)
            } else if brace && edge < s / 2 {
                (0.5, 0.32, 0.16)
            } else {
                (0.68 + plank, 0.46 + plank, 0.24 + plank)
            };
            px.extend([r, g, b].map(|v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8));
            px.push(255);
        }
    }
    px
}

impl LitFloor {
    fn new(gfx: &yowie::gfx::Graphics) -> GameResult<Self> {
        let (colors, normals) = floor_images();
        let (w, h) = (FLOOR_W, FLOOR_H);
        let data = TextureOptions {
            data: true,
            ..TextureOptions::default()
        };
        Ok(Self {
            bricks: Texture::from_pixels(gfx, w, h, &colors, TextureOptions::default())?,
            normals: Texture::from_pixels(gfx, w, h, &normals, data)?,
            crate_tex: Texture::from_pixels(
                gfx,
                40,
                40,
                &crate_pixels(40),
                TextureOptions::default(),
            )?,
        })
    }

    fn rect(view: Vec2) -> Rect {
        let size = vec2(FLOOR_W as f32, FLOOR_H as f32);
        Rect::new(
            (view.x - size.x) * 0.5,
            view.y - size.y - 14.0,
            size.x,
            size.y,
        )
    }

    fn draw(&self, g: &mut yowie::gfx::Graphics, t: f32) {
        let floor = Self::rect(g.view_size());
        let centre = floor.min() + floor.size() * 0.5;
        let lamp = centre + vec2((t * 0.9).cos() * 125.0, (t * 0.9).sin() * 58.0);
        let warm = Color::rgb8(255, 196, 120);
        g.with_layer(2, |g| {
            g.set_lights_2d(
                Color::rgb8(24, 26, 44),
                &[
                    Light2D::point(lamp, 250.0, warm)
                        .height(46.0)
                        .intensity(1.5)
                        .shadows(true)
                        .softness(10.0),
                    Light2D::point(
                        floor.min() + vec2(300.0, 28.0),
                        120.0,
                        Color::rgb8(90, 120, 255),
                    )
                    .height(30.0)
                    .intensity(0.7),
                ],
            );
            let sprites = CRATES.map(|c| {
                Sprite::at(floor.min() + c)
                    .size(Vec2::splat(CRATE))
                    .centered()
            });
            for s in &sprites {
                g.add_occluder_sprite(&self.crate_tex, s);
            }
            g.draw_lit(&self.bricks, Some(&self.normals), &Sprite::at(floor.min()));
            // Crates are plain sprites, tinted by how near the lamp is.
            for s in &sprites {
                let near = (1.0 - (s.position - lamp).length() / 250.0).clamp(0.0, 1.0);
                let k = 0.42 + 0.58 * near * near;
                let tint = Color::rgb(k * 1.0, k * 0.92, k * 0.85);
                g.draw_sprite(&self.crate_tex, &s.tint(tint));
            }
            g.stroke_rect(floor, 2.0, Color::rgb8(70, 74, 96));
        });
        g.with_layer(3, |g| {
            g.with_blend(BlendMode::Additive, |g| {
                g.fill_circle(lamp, 9.0, Color::rgba(1.0, 0.8, 0.45, 0.9));
                g.fill_circle(lamp, 22.0, Color::rgba(1.0, 0.7, 0.3, 0.18));
            });
        });
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(14, 16, 28),
        ..Config::new("Yowie sprites").with_size(960, 540)
    };
    yowie::run::<Sprites>(config)
}
