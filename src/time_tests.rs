//! The time scale through the stepper: steps per wall second change with
//! the scale, pausing stops them, and the console variable follows.

use yowie_core::UVec2;
use yowie_gfx::{Graphics, GraphicsSettings};

use crate::context::Host;
use crate::stepper::{Stepper, Tick};
use crate::{Config, Context, Game, GameResult};

struct Counter {
    deltas: Vec<f32>,
}

impl Game for Counter {
    fn init(_: &mut Context) -> GameResult<Self> {
        Ok(Counter { deltas: Vec::new() })
    }
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.deltas.push(ctx.time.delta());
        Ok(())
    }
    fn draw(&mut self, _: &mut Context) -> GameResult {
        Ok(())
    }
}

fn context(config: &Config) -> Option<Context> {
    if !yowie_gfx::adapter_available() {
        println!("skipping: no GPU adapter");
        return None;
    }
    let size = UVec2::new(8, 8);
    let gfx = Graphics::new_headless(size, GraphicsSettings::default()).ok()?;
    let host = Host::Headless {
        title: "test".into(),
        fullscreen: false,
        size,
    };
    Context::new(gfx, host, config).ok()
}

/// Runs `frames` frames under the fixed clock (one step of wall time each)
/// and returns how many updates ran.
fn updates(ctx: &mut Context, frames: u32) -> u64 {
    let config = Config::default();
    let mut stepper = Stepper::new(&config, true);
    let mut game = Counter { deltas: Vec::new() };
    let tick = Tick {
        turn: true,
        draw: false,
    };
    for _ in 0..frames {
        stepper.tick(ctx, &mut game, tick).expect("tick");
    }
    assert!(game.deltas.iter().all(|d| (*d - 1.0 / 60.0).abs() < 1e-6));
    ctx.time.updates()
}

#[test]
fn the_scale_sets_updates_per_wall_second() {
    let cases = [
        // (scale, paused, updates in 60 frames)
        (1.0, false, 60),
        (2.0, false, 120),
        (0.5, false, 30),
        (0.0, false, 0),
        (1.0, true, 0),
    ];
    for (scale, paused, want) in cases {
        let Some(mut ctx) = context(&Config::default()) else {
            return;
        };
        ctx.time.set_scale(scale);
        ctx.time.set_paused(paused);
        assert_eq!(updates(&mut ctx, 60), want, "scale {scale} paused {paused}");
    }
}

#[test]
fn a_large_scale_is_capped_per_frame() {
    let Some(mut ctx) = context(&Config::default()) else {
        return;
    };
    ctx.time.set_scale(64.0);
    // The default cap is 8 updates per frame.
    assert_eq!(updates(&mut ctx, 10), 80);
}

#[cfg(feature = "console")]
#[test]
fn the_console_command_sets_the_scale() {
    let config = Config {
        console: true,
        ..Config::default()
    };
    let Some(mut ctx) = context(&config) else {
        return;
    };
    crate::ui_hook::setup(&mut ctx, &config);
    ctx.console.execute("timescale 0.5");
    crate::ui_hook::sync_time_scale(&mut ctx);
    assert_eq!(ctx.time.scale(), 0.5);
    // A change by the game shows in the variable.
    ctx.time.set_scale(3.0);
    crate::ui_hook::sync_time_scale(&mut ctx);
    ctx.console.execute("timescale");
    ctx.console.execute("set time.scale 2");
    crate::ui_hook::sync_time_scale(&mut ctx);
    assert_eq!(ctx.time.scale(), 2.0);
    ctx.console.execute("timescale -1");
    crate::ui_hook::sync_time_scale(&mut ctx);
    assert_eq!(ctx.time.scale(), 2.0);
}
