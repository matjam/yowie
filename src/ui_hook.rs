//! Wiring of the interface toolkit and the debug console into the loop:
//! the clipboard, the log tee, routing input to the console while it is
//! open, drawing the console last, screenshots it asks for, and input
//! method placement for text fields.

use yowie_input::InputState;

use crate::{Config, Context};

/// What the hook remembers between frames.
#[derive(Debug, Default)]
pub(crate) struct UiHook {
    /// Whether this hook turned input-method composition on.
    ime: bool,
    /// A screenshot the console asked for, written after the frame renders.
    shot: Option<String>,
    /// The `time.scale` console variable, once the console is on.
    #[cfg(feature = "console")]
    time_var: Option<yowie_console::FloatVar>,
    /// The scale both sides agreed on at the last sync.
    time_scale: f32,
}

/// Lets the `time.scale` variable and `ctx.time` follow each other: a
/// change on either side since the last sync wins, the console's first.
pub(crate) fn sync_time_scale(ctx: &mut Context) {
    #[cfg(feature = "console")]
    if let Some(var) = &ctx.hook.time_var {
        let v = var.get();
        if v != ctx.hook.time_scale {
            ctx.time.set_scale(v);
        } else if ctx.time.scale() != ctx.hook.time_scale {
            var.set(ctx.time.scale());
        }
        // The clamp may have changed what the console typed.
        var.set(ctx.time.scale());
        ctx.hook.time_scale = ctx.time.scale();
    }
    let _ = ctx;
}

/// Registers the `time.scale` variable and the `timescale` command.
#[cfg(feature = "console")]
fn register_time(ctx: &mut Context) {
    let var = ctx
        .console
        .float("time.scale", 1.0, "game speed; 1 is normal, 0 stops game time");
    let handle = var.clone();
    ctx.console.register(
        "timescale",
        "timescale [factor]: show or set the game speed (1 normal, 0.5 half, 0 stops)",
        move |args| match args {
            [] => Ok(format!("timescale {}", handle.get())),
            [v] => match v.parse::<f32>() {
                Ok(f) if f.is_finite() && f >= 0.0 => {
                    handle.set(f);
                    Ok(format!("timescale {f}"))
                }
                _ => Err(format!("timescale wants a number of 0 or more, got {v:?}")),
            },
            _ => Err("usage: timescale [factor]".into()),
        },
    );
    ctx.hook.time_var = Some(var);
    ctx.hook.time_scale = 1.0;
}

/// The engine's clipboard as the interface toolkit uses it.
#[cfg(feature = "ui")]
struct EngineClipboard(crate::clipboard::Clipboard);

#[cfg(feature = "ui")]
impl yowie_ui::Clipboard for EngineClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.get()
    }

    fn set(&mut self, text: &str) {
        if let Err(e) = self.0.set(text) {
            log::warn!("yowie: clipboard: {e}");
        }
    }
}

#[cfg(feature = "ui")]
fn clipboard(headless: bool) -> Box<dyn yowie_ui::Clipboard> {
    let inner = if headless {
        crate::clipboard::Clipboard::memory()
    } else {
        crate::clipboard::Clipboard::system()
    };
    Box::new(EngineClipboard(inner))
}

/// Configures `ctx.ui` and builds `ctx.console` once the context exists.
pub(crate) fn setup(ctx: &mut Context, config: &Config) {
    #[cfg(feature = "ui")]
    ctx.ui.set_clipboard(clipboard(ctx.headless()));
    #[cfg(feature = "console")]
    if config.console {
        let options = yowie_console::ConsoleOptions {
            toggle_key: config.console_key,
            ..yowie_console::ConsoleOptions::default()
        };
        ctx.console = yowie_console::Console::new(options);
        ctx.console.set_clipboard(clipboard(ctx.headless()));
        register_time(ctx);
    }
    let _ = (ctx, config);
}

/// Installs the logger: `env_logger` filtered by `RUST_LOG`, teed into the
/// console's output when the console is on. An existing logger stays.
pub(crate) fn install_logger(config: &Config) {
    let env = env_logger::Env::default().default_filter_or("warn,yowie=info,yowie_gfx=info");
    #[cfg(feature = "console")]
    if config.console {
        let logger = env_logger::Builder::from_env(env).build();
        let filter = logger.filter();
        // An existing logger is the game's choice and stays.
        let _ = yowie_console::install_logger(Some(Box::new(logger)), filter, 1000);
        return;
    }
    let _ = config;
    // An existing logger is the game's choice and stays.
    let _ = env_logger::Builder::from_env(env).try_init();
}

/// Takes the real input away from the game while the console is open and
/// leaves it an idle one. Give the real one back with [`release_input`].
pub(crate) fn capture_input(ctx: &mut Context) -> Option<InputState> {
    ctx.input_captured().then(|| std::mem::take(&mut ctx.input))
}

/// Returns input taken by [`capture_input`].
pub(crate) fn release_input(ctx: &mut Context, saved: Option<InputState>) {
    if let Some(s) = saved {
        ctx.input = s;
    }
}

/// Sets how long interface time advances this frame.
pub(crate) fn begin_draw(ctx: &mut Context, dt: f32) {
    #[cfg(feature = "ui")]
    ctx.ui.set_delta_time(dt);
    let _ = (ctx, dt);
}

/// Draws the console after everything else and carries out what it asked
/// for.
pub(crate) fn draw_console(ctx: &mut Context, dt: f32) {
    #[cfg(feature = "console")]
    {
        if ctx.console.enabled() {
            ctx.console.set_schedule_stats(
                ctx.services.schedule_stats.as_slice(),
                ctx.services.schedule_updates,
            );
        }
        let input = &ctx.input;
        ctx.console.draw(&mut ctx.gfx, input, dt);
        for request in ctx.console.take_requests() {
            match request {
                yowie_console::Request::Quit => ctx.quit(),
                yowie_console::Request::Screenshot(path) => {
                    ctx.gfx.request_capture();
                    ctx.hook.shot = Some(path);
                }
            }
        }
    }
    let _ = (ctx, dt);
}

/// Turns input-method composition on while a text field has focus and
/// tells the platform where it is.
pub(crate) fn end_draw(ctx: &mut Context) {
    let mut want = false;
    let mut area = None;
    #[cfg(feature = "ui")]
    {
        want |= ctx.ui.wants_keyboard();
        area = area.or(ctx.ui.text_input_area());
    }
    #[cfg(feature = "console")]
    {
        want |= ctx.console.wants_keyboard();
        area = ctx.console.text_input_area().or(area);
    }
    if want != ctx.hook.ime {
        ctx.hook.ime = want;
        ctx.set_text_input(want);
    }
    if want && let Some(a) = area {
        ctx.set_text_input_area(a);
    }
}

/// Writes the screenshot the console asked for, once the frame rendered.
pub(crate) fn after_frame(ctx: &mut Context) {
    let Some(path) = ctx.hook.shot.take() else {
        return;
    };
    match ctx.gfx.take_capture() {
        Some(Ok(image)) => match image.save(&path) {
            Ok(()) => log::info!("yowie: wrote {path}"),
            Err(e) => log::error!("yowie: cannot write {path}: {e}"),
        },
        Some(Err(e)) => log::error!("yowie: screenshot failed: {e}"),
        None => log::error!("yowie: screenshot failed: no frame captured"),
    }
}

/// Feeds the frame's timings to the console's panels.
pub(crate) fn record(ctx: &mut Context, fps: f32, frame_ms: f32, update_ms: f32, draw_ms: f32) {
    #[cfg(feature = "console")]
    if ctx.console.enabled() {
        let stats = ctx.gfx.stats();
        ctx.console.record_frame(yowie_console::FrameTimes {
            fps,
            frame_ms,
            update_ms,
            draw_ms,
            stats,
        });
    }
    let _ = (ctx, fps, frame_ms, update_ms, draw_ms);
}

impl Context {
    /// Whether the debug console is open and has the keyboard. While it
    /// is, the engine gives the game an idle input state and no input
    /// events, so keys typed into the console do not move the player.
    pub fn input_captured(&self) -> bool {
        #[cfg(feature = "console")]
        {
            self.console.is_open()
        }
        #[cfg(not(feature = "console"))]
        {
            false
        }
    }
}

pub(crate) use UiHook as Hook;
