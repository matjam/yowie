//! The headless run: no window and no event loop, a GPU device with no
//! surface, frames under the fixed clock, and the last frame written out.

use std::path::Path;

use yowie_gfx::{Graphics, GraphicsSettings};

use crate::args::RunArgs;
use crate::context::Host;
use crate::stepper::{Stepper, Tick};
use crate::{Config, Context, Error, Game, Result};

/// Renders `config.width` by `config.height` pixel frames until `--seconds`
/// of game time pass (one frame without it), then writes the last frame to
/// the `--shot` path if one was given.
pub(crate) fn run_headless<G: Game>(config: Config, args: RunArgs) -> Result<()> {
    let gfx = Graphics::new_headless(
        config.size(),
        GraphicsSettings {
            vsync: false,
            view: config.view_settings(),
            clear_color: config.clear_color,
        },
    )?;
    let host = Host::Headless {
        title: config.title.clone(),
        fullscreen: config.fullscreen,
        size: config.size(),
    };
    let mut ctx = Context::new(gfx, host, config.debug_overlay);
    crate::ui_hook::setup(&mut ctx, &config);
    let mut game = G::init(&mut ctx).map_err(Error::Game)?;
    let mut stepper = Stepper::new(&config, true);
    let tick = Tick {
        turn: true,
        draw: true,
    };
    loop {
        stepper.tick(&mut ctx, &mut game, tick)?;
        let done = args.seconds.is_none_or(|s| ctx.time.elapsed() + 1e-9 >= s);
        if done || ctx.requests.quit {
            break;
        }
    }
    if let Some(path) = &args.shot {
        write_shot(&ctx.gfx, path)?;
    }
    Ok(())
}

fn write_shot(gfx: &Graphics, path: &Path) -> Result<()> {
    let image = gfx.read_output()?;
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        // A missing directory surfaces as the save error below.
        let _ = std::fs::create_dir_all(dir);
    }
    image.save(path).map_err(|source| Error::Screenshot {
        path: path.display().to_string(),
        source,
    })?;
    log::info!("yowie: wrote {}", path.display());
    Ok(())
}
