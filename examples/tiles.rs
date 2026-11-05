//! A sprite sheet, a tilemap that draws only what the camera sees, a
//! camera that follows a walking character with a dead zone, smoothing,
//! zoom, rotation and shake, a walking animation, layers (ground, decor,
//! the character, tree tops), timers and tweens, and a nine-slice HUD with
//! wrapped text. Arrow keys or WASD walk, Q and E zoom, R and T turn, Space
//! shakes the screen, Escape quits. With no keys held the character walks
//! a loop on its own.

#[path = "common/art.rs"]
mod art;

use std::f32::consts::TAU;
use std::sync::Arc;

use art::{Canvas, hash, shade};
use yowie::anim::{FlipbookPlayer, LoopMode};
use yowie::gfx::{
    CameraRig, Font, GridLayout, NineSlice, Sprite, SpriteSheet, TextStyle, Texture,
    TextureOptions, Tile, TileAnimation, Tilemap, sheet_flipbook,
};
use yowie::input::Key;
use yowie::timer::Timers;
use yowie::tween::{Curve, Ease, Tween};
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, uvec2, vec2};

const MAP_W: i32 = 96;
const MAP_H: i32 = 64;
const TILE: f32 = 16.0;

// Frames of the world sheet.
const GRASS: u32 = 0;
const DIRT: u32 = 3;
const WATER: u32 = 4;
const STONE: u32 = 7;
const BUSH: u32 = 8;
const ROCK: u32 = 9;
const FLOWERS: u32 = 10;
const TRUNK: u32 = 11;
const CANOPY: u32 = 12;

#[derive(Clone)]
enum Event {
    Shake,
    Refill,
}

fn is_water(x: i32, y: i32) -> bool {
    let ell = |cx: f32, cy: f32, rx: f32, ry: f32| {
        let (dx, dy) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
        dx * dx + dy * dy < 1.0
    };
    ell(26.0, 18.0, 9.0, 6.0) || ell(70.0, 46.0, 12.0, 8.0)
}

fn road_y(x: i32) -> i32 {
    32 + ((x as f32 * 0.18).sin() * 3.0).round() as i32
}

fn world_sheet() -> Canvas {
    let mut c = Canvas::new(8 * 16, 2 * 16);
    let grass = [70u8, 140, 66];
    for v in 0..3 {
        c.paint(v * 16, 0, 16, |x, y| {
            let n = hash(x, y, 1 + v as u32);
            let mut col = shade(grass, 0.9 + f32::from(n) / 255.0 * 0.2);
            if v == 1 && n > 235 {
                col = shade(grass, 0.65);
            }
            if v == 2 && n > 244 {
                col = [250, 240, 120, 255];
            }
            Some(col)
        });
    }
    c.paint(3 * 16, 0, 16, |x, y| {
        Some(shade(
            [150, 112, 72],
            0.88 + f32::from(hash(x, y, 9)) / 255.0 * 0.22,
        ))
    });
    for f in 0..3 {
        c.paint((4 + f) * 16, 0, 16, |x, y| {
            let wave =
                (((x + f as i32 * 5) as f32 / 16.0 * TAU).sin() * 1.5 + y as f32).round() as i32;
            let light = wave % 6 == 0;
            Some(if light {
                [120, 180, 235, 255]
            } else {
                shade(
                    [50, 110, 190],
                    0.95 + f32::from(hash(x, y, 4)) / 255.0 * 0.1,
                )
            })
        });
    }
    c.paint(7 * 16, 0, 16, |x, y| {
        let mortar = y % 8 == 0 || (x + if (y / 8) % 2 == 0 { 0 } else { 8 }) % 16 == 0;
        Some(if mortar {
            [90, 92, 100, 255]
        } else {
            shade(
                [140, 142, 150],
                0.9 + f32::from(hash(x, y, 7)) / 255.0 * 0.2,
            )
        })
    });
    // Decor and tree parts on a transparent background.
    c.disc(8.0 + 16.0 * 0.0, 9.0 + 16.0, 6.0, [38, 104, 54, 255]);
    c.disc(6.5, 7.5 + 16.0, 3.0, [70, 150, 80, 255]);
    c.disc(16.0 + 8.0, 10.0 + 16.0, 5.0, [120, 122, 130, 255]);
    c.disc(16.0 + 6.5, 8.5 + 16.0, 2.0, [170, 172, 180, 255]);
    for (i, col) in [
        [240, 90, 110, 255],
        [250, 230, 90, 255],
        [240, 240, 250, 255],
    ]
    .into_iter()
    .enumerate()
    {
        c.disc(
            32.0 + 3.0 + i as f32 * 4.5,
            16.0 + 6.0 + (i % 2) as f32 * 4.0,
            1.6,
            col,
        );
    }
    c.rect(48 + 6, 16, 4, 16, [96, 66, 40, 255]);
    c.rect(48 + 6, 16, 1, 16, [70, 46, 28, 255]);
    c.disc(64.0 + 8.0, 16.0 + 10.0, 8.0, [34, 110, 52, 255]);
    c.disc(64.0 + 6.0, 16.0 + 8.0, 5.0, [60, 150, 74, 255]);
    c
}

/// Sixteen walking frames: four directions (down, up, left, right) of four
/// frames, each 16 by 24.
fn walker_sheet() -> Canvas {
    let mut c = Canvas::new(4 * 16, 4 * 24);
    for dir in 0..4i32 {
        for f in 0..4i32 {
            let (ox, oy) = (f * 16, dir * 24);
            let step = [0, 1, 0, -1][f as usize];
            let bob = i32::from(f % 2 == 1);
            // Legs.
            let (lx, rx) = match dir {
                2 | 3 => (6 - step, 8 + step),
                _ => (5, 9),
            };
            let lift = |s: i32| (s * 2).max(0);
            c.rect(
                ox + lx,
                oy + 17 - lift(step),
                3,
                6 + lift(step),
                [50, 60, 120, 255],
            );
            c.rect(
                ox + rx,
                oy + 17 - lift(-step),
                3,
                6 + lift(-step),
                [50, 60, 120, 255],
            );
            // Body and arms.
            c.rect(ox + 4, oy + 9 - bob, 8, 9, [200, 70, 60, 255]);
            let arm = if dir < 2 { step } else { 0 };
            c.rect(ox + 2, oy + 10 - bob + arm, 2, 6, [180, 56, 50, 255]);
            c.rect(ox + 12, oy + 10 - bob - arm, 2, 6, [180, 56, 50, 255]);
            // Head.
            c.rect(ox + 5, oy + 2 - bob, 6, 7, [240, 200, 160, 255]);
            c.rect(ox + 5, oy + 1 - bob, 6, 3, [90, 56, 36, 255]);
            match dir {
                0 => {
                    c.put(ox + 6, oy + 5 - bob, [30, 30, 40, 255]);
                    c.put(ox + 9, oy + 5 - bob, [30, 30, 40, 255]);
                }
                1 => c.rect(ox + 5, oy + 2 - bob, 6, 5, [90, 56, 36, 255]),
                2 => c.put(ox + 6, oy + 5 - bob, [30, 30, 40, 255]),
                _ => c.put(ox + 9, oy + 5 - bob, [30, 30, 40, 255]),
            }
        }
    }
    c
}

fn panel_texture() -> Canvas {
    let mut c = Canvas::new(24, 24);
    c.paint(0, 0, 24, |x, y| {
        let (dx, dy) = ((x - 11).abs().max(0), (y - 11).abs().max(0));
        let edge = dx.max(dy);
        let corner = dx >= 9 && dy >= 9 && (dx - 9) + (dy - 9) >= 2;
        if corner {
            None
        } else if edge >= 11 {
            Some([20, 24, 40, 255])
        } else if edge >= 9 {
            Some([150, 170, 230, 255])
        } else {
            Some([28, 34, 62, 235])
        }
    });
    c
}

struct Tiles {
    font: Font,
    mono: Font,
    ground: Tilemap,
    decor: Tilemap,
    canopy: Tilemap,
    walker: SpriteSheet,
    player: FlipbookPlayer,
    hud: NineSlice,
    rig: CameraRig,
    timers: Timers<Event>,
    stamina: Tween<f32>,
    pos: Vec2,
    facing: usize,
    clock: f32,
    manual_zoom: f32,
    manual_turn: f32,
    shakes: u32,
    waypoint: usize,
}

const ROUTE: [(i32, i32); 5] = [(14, 30), (40, 34), (80, 30), (60, 38), (24, 36)];

impl Game for Tiles {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let opts = TextureOptions::default();
        let world = Texture::from_image(&ctx.gfx, &world_sheet().img, opts)?;
        let sheet = SpriteSheet::new(world, uvec2(16, 16));
        let layout = GridLayout::square(vec2(TILE, TILE));
        let size = uvec2(MAP_W as u32, MAP_H as u32);
        let mut ground = Tilemap::new(layout, size, sheet.clone());
        let mut decor = Tilemap::new(layout, size, sheet.clone());
        let mut canopy = Tilemap::new(layout, size, sheet);
        ground.fill_with(|x, y| {
            let n = hash(x, y, 3);
            let ry = road_y(x);
            Tile::new(if is_water(x, y) {
                WATER
            } else if (y - ry).abs() <= 1 {
                DIRT
            } else if (y - ry).abs() == 2 && n > 200 {
                STONE
            } else {
                GRASS + u32::from(n > 200) + u32::from(n > 245)
            })
        });
        decor.fill_with(|x, y| {
            let n = hash(x, y, 8);
            if is_water(x, y) || (y - road_y(x)).abs() <= 2 {
                return Tile::EMPTY;
            }
            match n {
                0..=6 => Tile::new(BUSH),
                7..=11 => Tile::new(ROCK),
                12..=22 => Tile::new(FLOWERS),
                _ => Tile::EMPTY,
            }
        });
        // Trees: a trunk on the decor layer, its crown on a layer above
        // the character.
        for y in 0..MAP_H {
            for x in 0..MAP_W {
                let tree =
                    hash(x, y, 21) < 5 && !is_water(x, y) && (y - road_y(x)).abs() > 3 && y > 0;
                if tree {
                    decor.set(x, y, Tile::new(TRUNK));
                    canopy.set(x, y - 1, Tile::new(CANOPY));
                    canopy.set(x - 1, y - 1, Tile::new(CANOPY));
                    canopy.set(x + 1, y - 1, Tile::new(CANOPY));
                }
            }
        }
        ground.animate(WATER, TileAnimation::uniform([4, 5, 6, 5], 0.35));

        let walker_tex = Texture::from_image(&ctx.gfx, &walker_sheet().img, opts)?;
        let walker = SpriteSheet::new(walker_tex, uvec2(16, 24));
        let book = sheet_flipbook(
            0..16,
            8.0,
            &[
                ("down", 0, 3),
                ("up", 4, 7),
                ("left", 8, 11),
                ("right", 12, 15),
            ],
        );
        let mut player = FlipbookPlayer::new(Arc::new(book));
        player.play_tag("down", LoopMode::Loop);

        let panel = Texture::from_image(&ctx.gfx, &panel_texture().img, opts)?;
        let view = ctx.gfx.view_size();
        let start = vec2(ROUTE[0].0 as f32, ROUTE[0].1 as f32) * TILE;
        let mut rig = CameraRig::new(start, view);
        rig.set_zoom(2.0);
        rig.dead_zone = vec2(64.0, 40.0);
        rig.smoothing = 5.0;
        rig.bounds = Some(ground.bounds());
        rig.snap_to(start);

        let mut timers = Timers::new();
        timers.every(3.5, Event::Shake);
        timers.every(5.0, Event::Refill);
        Ok(Tiles {
            font: ctx.gfx.default_font(),
            mono: ctx.gfx.default_mono_font(),
            ground,
            decor,
            canopy,
            walker,
            player,
            hud: NineSlice::uniform(panel, 8.0),
            rig,
            timers,
            stamina: Tween::new(1.0, 0.2, 5.0).ease(Ease::Out(Curve::Cubic)),
            pos: start,
            facing: 3,
            clock: 0.0,
            manual_zoom: 2.0,
            manual_turn: 0.0,
            shakes: 0,
            waypoint: 1,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let input = &ctx.input;
        self.clock += dt;
        let mut dir = Vec2::ZERO;
        for (keys, d) in [
            ([Key::Left, Key::A], vec2(-1.0, 0.0)),
            ([Key::Right, Key::D], vec2(1.0, 0.0)),
            ([Key::Up, Key::W], vec2(0.0, -1.0)),
            ([Key::Down, Key::S], vec2(0.0, 1.0)),
        ] {
            if keys.iter().any(|k| input.key_down(*k)) {
                dir += d;
            }
        }
        if input.key_down(Key::Q) {
            self.manual_zoom = (self.manual_zoom - 1.5 * dt).max(0.8);
        }
        if input.key_down(Key::E) {
            self.manual_zoom = (self.manual_zoom + 1.5 * dt).min(4.0);
        }
        if input.key_down(Key::R) {
            self.manual_turn -= dt;
        }
        if input.key_down(Key::T) {
            self.manual_turn += dt;
        }
        if input.key_pressed(Key::Space) {
            self.rig.add_trauma(0.7);
        }
        // Autopilot along the route when no key is held.
        if dir == Vec2::ZERO {
            let (wx, wy) = ROUTE[self.waypoint];
            let goal = vec2(wx as f32, wy as f32) * TILE;
            let to = goal - self.pos;
            if to.length() < 4.0 {
                self.waypoint = (self.waypoint + 1) % ROUTE.len();
            } else {
                dir = to.normalize();
            }
        }
        if dir != Vec2::ZERO {
            let dir = dir.normalize();
            self.pos += dir * 72.0 * dt;
            self.facing = if dir.x.abs() > dir.y.abs() {
                if dir.x < 0.0 { 2 } else { 3 }
            } else if dir.y < 0.0 {
                1
            } else {
                0
            };
            let tag = ["down", "up", "left", "right"][self.facing];
            if self.player.frame().map(|f| f / 4) != Some(self.facing) {
                self.player.play_tag(tag, LoopMode::Loop);
            }
            self.player.advance(dt);
        }
        self.pos = self
            .pos
            .clamp(Vec2::ZERO, vec2(MAP_W as f32, MAP_H as f32) * TILE);

        for fired in self.timers.update(f64::from(dt)) {
            match fired.data {
                Event::Shake => {
                    self.rig.add_trauma(0.45);
                    self.shakes += 1;
                }
                Event::Refill => {
                    self.stamina = Tween::new(1.0, 0.2, 5.0).ease(Ease::Out(Curve::Cubic))
                }
            }
        }
        self.stamina.update(dt);
        self.ground.advance(dt);

        self.rig.set_zoom(self.manual_zoom);
        self.rig
            .set_rotation(self.manual_turn + 0.07 * (self.clock * 0.5).sin());
        self.rig.follow(self.pos, dt);
        self.rig.update(dt);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let cam = self.rig.camera();
        let g = &mut ctx.gfx;
        g.with_camera(&cam, |g| {
            g.set_layer(0);
            self.ground.draw(g);
            g.set_layer(1);
            self.decor.draw(g);
            g.set_layer(2);
            g.set_sort_key(self.pos.y);
            if let Some(frame) = self.player.frame() {
                let sprite = Sprite::at(self.pos).origin(vec2(0.5, 1.0));
                self.walker.draw_frame(g, frame as u32, &sprite);
            }
            g.set_layer(3);
            self.canopy.draw(g);
            g.set_layer(0);
        });

        // HUD in view units, with no camera.
        g.set_layer(yowie::gfx::layers::UI);
        let panel = Rect::new(12.0, 12.0, 330.0, 150.0);
        g.draw_nine_slice(&self.hud, panel, Color::WHITE);
        let ink = Color::rgb8(232, 236, 250);
        g.draw_text(
            &self.font,
            "Yowie tiles",
            vec2(28.0, 22.0),
            &TextStyle::new(20.0).color(Color::rgb8(255, 220, 120)),
        );
        let body = format!(
            "The camera follows with a dead zone and smoothing, at zoom {:.1} and a slow \
             sway. Timers shake the view every 3.5 seconds ({} so far); a tween eases the bar.",
            self.manual_zoom, self.shakes
        );
        g.draw_text(
            &self.font,
            &body,
            vec2(28.0, 48.0),
            &TextStyle::new(13.0).color(ink).width(300.0),
        );
        let bar = Rect::new(28.0, 134.0, 200.0, 10.0);
        g.fill_rect(bar, Color::rgb8(20, 22, 36));
        g.fill_rect(
            Rect::new(
                bar.x,
                bar.y,
                bar.w * self.stamina.value().clamp(0.0, 1.0),
                bar.h,
            ),
            Color::rgb8(110, 210, 130),
        );
        let s = g.stats();
        let stats = format!(
            "{} cells, {} draw calls, {} vertices",
            self.ground.size().x * self.ground.size().y,
            s.draw_calls,
            s.vertices
        );
        g.draw_text(
            &self.mono,
            &stats,
            vec2(28.0, 146.0),
            &TextStyle::new(11.0).color(Color::rgb8(150, 160, 190)),
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(20, 28, 24),
        ..Config::new("Yowie tiles").with_size(960, 540)
    };
    yowie::run::<Tiles>(config)
}
