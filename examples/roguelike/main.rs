//! A small turn-based dungeon crawl: rooms, corridors, field of view, goblins
//! that chase, and a message log. It runs in the engine's turn-based loop, so
//! the process sleeps between key presses. Move with the arrow keys, HJKL,
//! YUBN or the numpad; `.` waits; Escape quits.
//!
//! `--autoplay` lets the hero walk toward the nearest goblin on its own, one
//! turn per update, which is how the headless run gets a picture with some
//! history in it.

#[path = "../common/mod.rs"]
mod common;
mod dungeon;

use dungeon::{Dungeon, MAP_H, MAP_W};
use yowie::gfx::{Font, TextStyle};
use yowie::input::Key;
use yowie::rng::Rng;
use yowie::{Color, Config, Context, Game, GameResult, IVec2, LoopMode, Rect, ivec2, vec2};

const CELL: f32 = 20.0;

const MOVES: [(Key, IVec2); 20] = [
    (Key::Left, ivec2(-1, 0)),
    (Key::Right, ivec2(1, 0)),
    (Key::Up, ivec2(0, -1)),
    (Key::Down, ivec2(0, 1)),
    (Key::H, ivec2(-1, 0)),
    (Key::L, ivec2(1, 0)),
    (Key::K, ivec2(0, -1)),
    (Key::J, ivec2(0, 1)),
    (Key::Y, ivec2(-1, -1)),
    (Key::U, ivec2(1, -1)),
    (Key::B, ivec2(-1, 1)),
    (Key::N, ivec2(1, 1)),
    (Key::Keypad4, ivec2(-1, 0)),
    (Key::Keypad6, ivec2(1, 0)),
    (Key::Keypad8, ivec2(0, -1)),
    (Key::Keypad2, ivec2(0, 1)),
    (Key::Keypad7, ivec2(-1, -1)),
    (Key::Keypad9, ivec2(1, -1)),
    (Key::Keypad1, ivec2(-1, 1)),
    (Key::Keypad3, ivec2(1, 1)),
];

struct Crawl {
    font: Font,
    dungeon: Dungeon,
    turn: u32,
    log: Vec<String>,
    autoplay: bool,
}

impl Crawl {
    fn say(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 4 {
            self.log.remove(0);
        }
    }

    fn take_turn(&mut self, d: IVec2) {
        if self.dungeon.player.hp <= 0 {
            return;
        }
        self.turn += 1;
        if let Some(msg) = self.dungeon.move_player(d) {
            self.say(msg);
        }
        self.dungeon.compute_fov();
        for msg in self.dungeon.monsters_act() {
            self.say(msg);
        }
        log::info!(
            "turn {} player {} hp {}",
            self.turn,
            self.dungeon.player.pos,
            self.dungeon.player.hp
        );
    }

    fn glyph(&self, ctx: &mut Context, at: IVec2, ch: &str, color: Color) {
        let style = TextStyle::new(18.0).color(color);
        let w = ctx.gfx.measure_text(&self.font, ch, &style).x;
        let p = vec2(at.x as f32 * CELL + (CELL - w) / 2.0, at.y as f32 * CELL);
        ctx.gfx.draw_text(&self.font, ch, p, &style);
    }
}

impl Game for Crawl {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let mut rng = Rng::with_stream(7, 11);
        let mut g = Crawl {
            font: ctx.gfx.default_mono_font(),
            dungeon: Dungeon::new(&mut rng),
            turn: 0,
            log: Vec::new(),
            autoplay: std::env::args().any(|a| a == "--autoplay"),
        };
        g.say("You descend into the dark. Goblins stir.");
        Ok(g)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if self.autoplay {
            // Keep the turn-based loop running so the hero keeps walking.
            ctx.request_redraw();
            let step = self.dungeon.step_toward_nearest_goblin().unwrap_or(ivec2(0, 0));
            self.take_turn(step);
            return Ok(());
        }
        if let Some((_, d)) = MOVES.iter().find(|(k, _)| ctx.input.key_pressed(*k)) {
            self.take_turn(*d);
        } else if ctx.input.key_pressed(Key::Period) || ctx.input.key_pressed(Key::Keypad5) {
            self.take_turn(ivec2(0, 0));
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        for (p, t) in self.dungeon.tiles.iter() {
            if !t.seen {
                continue;
            }
            let (ch, mut color) = if t.wall {
                ("#", Color::rgb8(150, 140, 130))
            } else {
                (".", Color::rgb8(110, 110, 120))
            };
            if !t.visible {
                color = Color::rgb8(70, 70, 90);
            }
            self.glyph(ctx, p, ch, color);
        }
        for m in &self.dungeon.monsters {
            let seen = self.dungeon.tiles.get(m.pos).is_some_and(|t| t.visible);
            if m.hp > 0 && seen {
                self.glyph(ctx, m.pos, "g", Color::rgb8(120, 220, 90));
            }
        }
        self.glyph(ctx, self.dungeon.player.pos, "@", Color::rgb8(255, 240, 160));

        let top = MAP_H as f32 * CELL + 8.0;
        let p = &self.dungeon.player;
        let hud = format!(
            "Turn {}   HP {}/{}   goblins {}",
            self.turn,
            p.hp.max(0),
            p.max_hp,
            self.dungeon.alive()
        );
        common::label(ctx, &self.font, &hud, vec2(8.0, top), 18.0, common::TEXT);
        for (i, msg) in self.log.iter().enumerate() {
            let y = top + (i + 1) as f32 * 22.0;
            common::label(ctx, &self.font, msg, vec2(8.0, y), 16.0, common::DIM);
        }
        if p.hp <= 0 {
            let size = ctx.window_size();
            ctx.gfx.fill_rect(
                Rect::new(0.0, 0.0, size.x as f32, size.y as f32),
                Color::rgba8(0, 0, 0, 150),
            );
            let at = vec2(size.x as f32 / 2.0 - 120.0, size.y as f32 / 2.0);
            common::label(ctx, &self.font, "You died. Escape to quit.", at, 20.0, Color::rgb8(255, 80, 80));
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let mut config = Config::new("Yowie roguelike").with_size(MAP_W as u32 * CELL as u32, MAP_H as u32 * CELL as u32 + 120);
    config.loop_mode = LoopMode::TurnBased;
    yowie::run::<Crawl>(config)
}
