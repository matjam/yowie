//! One frame of the loop: the updates it owes, then the draw. Windowed and
//! headless runs share it.

use std::time::Instant;

use yowie_input::Key;

use crate::clock::FixedClock;
use crate::{Config, Context, Error, Game, LoopMode, Result};

/// The longest a turn-based delta can be, in seconds.
const MAX_TURN_DELTA: f64 = 1.0;

/// What one frame should do.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tick {
    /// Turn-based loops: whether something happened that runs a turn.
    pub(crate) turn: bool,
    /// Whether to draw; a hidden window updates without drawing.
    pub(crate) draw: bool,
}

/// The loop's clock and pacing state.
#[derive(Debug)]
pub(crate) struct Stepper {
    mode: LoopMode,
    clock: FixedClock,
    fixed_clock: bool,
    pause_unfocused: bool,
    last_frame: Option<Instant>,
    last_turn: Option<Instant>,
    drawn_any: bool,
    paused: bool,
    fps: f32,
}

impl Stepper {
    pub(crate) fn new(config: &Config, fixed_clock: bool) -> Self {
        Self {
            mode: config.loop_mode,
            clock: FixedClock::new(config.loop_mode.step(), config.max_updates_per_frame),
            fixed_clock,
            pause_unfocused: config.pause_when_unfocused,
            last_frame: None,
            last_turn: None,
            drawn_any: false,
            paused: false,
            fps: 0.0,
        }
    }

    /// Seconds per update.
    pub(crate) fn step(&self) -> f64 {
        self.clock.step()
    }

    /// Runs one frame: the updates due, then the draw.
    pub(crate) fn tick<G: Game>(
        &mut self,
        ctx: &mut Context,
        game: &mut G,
        tick: Tick,
    ) -> Result<()> {
        let now = Instant::now();
        let wall = self
            .last_frame
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        self.last_frame = Some(now);
        if wall > 0.0 {
            let instant = (1.0 / wall) as f32;
            self.fps = if self.fps == 0.0 {
                instant
            } else {
                self.fps + (instant - self.fps) * 0.05
            };
        }
        ctx.time.frame_delta = wall as f32;
        ctx.time.fps = self.fps;

        let paused = self.pause_unfocused && !ctx.focused;
        let resumed = self.paused && !paused;
        self.paused = paused;
        let step = self.clock.step();
        let updates = match self.mode {
            _ if paused => {
                self.clock.reset();
                0
            }
            LoopMode::FixedStep { .. } if self.fixed_clock => 1,
            LoopMode::FixedStep { .. } => self.clock.advance(wall),
            LoopMode::TurnBased if !self.drawn_any || !tick.turn => 0,
            LoopMode::TurnBased => 1,
        };
        ctx.input.set_update_step(step as f32);

        let started = Instant::now();
        for _ in 0..updates {
            let delta = match self.mode {
                LoopMode::FixedStep { .. } => step,
                LoopMode::TurnBased if self.fixed_clock => step,
                LoopMode::TurnBased => {
                    let since = self
                        .last_turn
                        .map_or(0.0, |t| now.duration_since(t).as_secs_f64());
                    if resumed {
                        0.0
                    } else {
                        since.min(MAX_TURN_DELTA)
                    }
                }
            };
            self.last_turn = Some(now);
            ctx.time.delta = delta as f32;
            game.update(ctx).map_err(Error::Game)?;
            ctx.input.end_update();
            ctx.time.elapsed += delta;
            ctx.time.updates += 1;
            if ctx.requests.quit {
                break;
            }
        }
        if self.last_turn.is_none() {
            self.last_turn = Some(now);
        }
        ctx.time.alpha = match self.mode {
            LoopMode::FixedStep { .. } if !self.fixed_clock => self.clock.alpha(),
            _ => 0.0,
        };
        let update_ms = started.elapsed().as_secs_f32() * 1000.0;

        let mut draw_ms = 0.0;
        if tick.draw {
            let started = Instant::now();
            ctx.gfx.set_time(ctx.time.elapsed as f32);
            ctx.gfx.begin_frame();
            ctx.input.begin_draw();
            let drawn = game.draw(ctx);
            if ctx.input.key_pressed(Key::F3) {
                ctx.overlay.enabled = !ctx.overlay.enabled;
            }
            let stats = ctx.gfx.stats();
            ctx.overlay.draw(&mut ctx.gfx, self.fps, stats);
            ctx.input.end_draw();
            drawn.map_err(Error::Game)?;
            draw_ms = started.elapsed().as_secs_f32() * 1000.0;
            ctx.gfx.end_frame()?;
            ctx.time.frame += 1;
            self.drawn_any = true;
        }
        ctx.overlay
            .record((wall * 1000.0) as f32, update_ms, draw_ms);
        Ok(())
    }
}
