//! The complete game the Tetris guide builds on the ECS: systems, resources,
//! events, timers, tweens, a UI panel and synthesised sounds.
//!
//! Left and right move, Up rotates, Down soft-drops, Space hard-drops, R
//! restarts, P pauses, `[` and `]` halve and double the game speed (the time
//! scale), Escape quits.
//!
//! `--autoplay` lets a simple player fill in the controls, with a fixed
//! seed, so a headless run draws the same position every time.
//!
//! The source reads in the order the guide builds it: `model.rs` (what
//! exists), `systems.rs` (the rules and their order), then this file (input
//! in, drawing out).

mod autoplay;
mod model;
mod systems;

use autoplay::Autoplay;
use model::*;
use systems::{Effects, gravity_interval, rebuild_board};
use yowie::audio::Pcm;
use yowie::ecs::{Schedule, World};
use yowie::input::{InputEvent, Key};
use yowie::rng::Rng;
use yowie::timer::Timers;
use yowie::{Color, Config, Context, Event, Game, GameResult, Rect, vec2};

/// Pixels per board cell.
const CELL: f32 = 28.0;

struct Tetris {
    world: World,
    schedule: Schedule,
    autoplay: Option<Autoplay>,
    panel: Rect,
}

impl Tetris {
    /// Clears the board and deals the first piece: every resource is
    /// inserted fresh, so a restart is the same code as a start.
    fn reset(&mut self) {
        let w = &mut self.world;
        w.clear();
        w.insert_resource(Board::default());
        w.insert_resource(Score::default());
        w.insert_resource(Controls::default());
        w.insert_resource(Dt(0.0));
        let mut rng = Rng::new(7);
        let next = rng.below(SHAPES.len() as u64) as usize;
        w.insert_resource(Bag { rng, next });
        let mut timers = Timers::new();
        let gravity = timers.every(gravity_interval(0), Tick::Gravity);
        w.insert_resource(Clock {
            timers,
            gravity,
            lock: None,
        });
        // The first piece comes from the `spawn` system on the first update.
        rebuild_board(w);
        self.autoplay = self.autoplay.as_ref().map(|_| Autoplay::default());
    }

    /// The keys held this update, as the systems read them.
    fn read_keys(ctx: &Context) -> Controls {
        let i = &ctx.input;
        let key = |k| i.key_pressed(k) || i.key_repeated(k);
        Controls {
            left: key(Key::Left),
            right: key(Key::Right),
            rotate: i.key_pressed(Key::Up),
            down: key(Key::Down),
            drop: i.key_pressed(Key::Space),
        }
    }

    fn draw_cell(ctx: &mut Context, origin: yowie::Vec2, x: i32, y: i32, kind: usize, alpha: f32) {
        let color = SHAPES[kind].color.with_alpha(alpha);
        let p = origin + vec2(x as f32 * CELL + 1.0, y as f32 * CELL + 1.0);
        ctx.gfx
            .fill_rect(Rect::new(p.x, p.y, CELL - 2.0, CELL - 2.0), color);
    }
}

impl Game for Tetris {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let mixer = ctx.audio.clone();
        let rate = mixer.rate();
        let tone = |hz: f64, seconds: f64| mixer.sound(&Pcm::sine(hz, seconds, rate));
        let effects = Effects {
            lock: tone(220.0, 0.06)?,
            clear: tone(660.0, 0.25)?,
            rotate: tone(440.0, 0.03)?,
            over: tone(110.0, 0.6)?,
            mixer: mixer.clone(),
        };
        let schedule = systems::schedule(effects);
        let mut schedule = schedule;
        // Fail at start-up, not mid-game, if the ordering is wrong.
        schedule.validate()?;
        let autoplay = std::env::args()
            .any(|a| a == "--autoplay")
            .then(Autoplay::default);
        let mut game = Tetris {
            world: World::new(),
            schedule,
            autoplay,
            panel: Rect::new(404.0, 40.0, 200.0, 300.0),
        };
        game.reset();
        Ok(game)
    }

    fn event(&mut self, ctx: &mut Context, event: &Event) -> GameResult {
        // Pause and speed act on game time, so they work from an event even
        // while updates are stopped.
        if let Event::Input(InputEvent::KeyDown { key, repeat: false }) = event {
            match key {
                Key::P => {
                    let paused = ctx.time.paused();
                    ctx.time.set_paused(!paused);
                }
                Key::LeftBracket => ctx.time.set_scale((ctx.time.scale() * 0.5).max(0.25)),
                Key::RightBracket => ctx.time.set_scale((ctx.time.scale() * 2.0).min(4.0)),
                _ => {}
            }
        }
        Ok(())
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::R) {
            self.reset();
        }
        // Input becomes a resource so the systems stay functions of the
        // world alone.
        let controls = match &mut self.autoplay {
            Some(player) => player.controls(&self.world),
            None => Self::read_keys(ctx),
        };
        *res_mut::<Controls>(&mut self.world) = controls;
        res_mut::<Dt>(&mut self.world).0 = ctx.time.delta();
        self.schedule.run(&mut self.world);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let size = ctx.gfx.view_size();
        let origin = vec2(
            (size.x - COLS as f32 * CELL) / 2.0 - 80.0,
            (size.y - ROWS as f32 * CELL) / 2.0,
        );
        let board = vec2(COLS as f32 * CELL, ROWS as f32 * CELL);
        ctx.gfx.fill_rect(
            Rect::new(origin.x - 4.0, origin.y - 4.0, board.x + 8.0, board.y + 8.0),
            Color::rgb8(40, 42, 56),
        );
        ctx.gfx.fill_rect(
            Rect::new(origin.x, origin.y, board.x, board.y),
            Color::rgb8(16, 16, 24),
        );

        let w = &self.world;
        for c in w.query::<&Cell>().iter().filter(|c| c.y >= 0) {
            Self::draw_cell(ctx, origin, c.x, c.y, c.kind, 1.0);
        }
        if let Some((_, piece)) = falling(w) {
            // The ghost shows where the piece will land.
            let ghost = landing(res::<Board>(w), &piece);
            for (shown, alpha) in [(&ghost, 0.25), (&piece, 1.0)] {
                for (x, y) in shown.blocks().filter(|(_, y)| *y >= 0) {
                    Self::draw_cell(ctx, origin, x, y, shown.kind, alpha);
                }
            }
        }
        for flash in w.query::<&RowFlash>().iter() {
            let band = Rect::new(origin.x, origin.y + flash.row as f32 * CELL, board.x, CELL);
            ctx.gfx
                .fill_rect(band, Color::WHITE.with_alpha(0.6 * flash.fade.value()));
        }

        // The next piece, under the panel.
        let next = new_falling(res::<Bag>(w).next);
        let preview = vec2(self.panel.x + 20.0, self.panel.y + self.panel.h + 24.0);
        for (x, y) in next.blocks() {
            let (px, py) = (x - next.x, y);
            let p = preview + vec2(px as f32, py as f32) * (CELL * 0.7);
            let side = CELL * 0.7 - 2.0;
            ctx.gfx
                .fill_rect(Rect::new(p.x, p.y, side, side), SHAPES[next.kind].color);
        }

        let score = *res::<Score>(w);
        let next_name = SHAPES[res::<Bag>(w).next].name;
        let entities = w.len();
        let (paused, scale) = (ctx.time.paused(), ctx.time.scale());
        let mut restart = false;
        let panel = &mut self.panel;
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.window("Tetris", panel, |f| {
                f.label(&format!("Score {}", score.points));
                f.label(&format!("Lines {}", score.lines));
                f.label(&format!("Level {}", score.level));
                f.label(&format!("Next {next_name}"));
                f.label_dim(&format!("{entities} entities"));
                f.separator();
                if score.over {
                    f.label("Game over");
                } else if paused {
                    f.label("Paused");
                } else if scale != 1.0 {
                    f.label(&format!("Speed x{scale}"));
                }
                if f.button("Restart (R)").clicked {
                    restart = true;
                }
                f.label_dim("Arrows move, Space drops");
                f.label_dim("P pauses, [ ] change speed");
            });
        });
        if restart {
            self.reset();
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(24, 26, 34),
        ..Config::new("Yowie Tetris").with_size(640, 640)
    };
    yowie::run::<Tetris>(config)
}
