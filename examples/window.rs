//! The platform layer's smoke test: a window that prints every event as it
//! arrives, tagged with the window it came from. F11 toggles full screen,
//! C toggles a locked cursor, T toggles text input (input-method
//! composition), N opens or closes a second window that draws its own
//! content, Escape quits.
//!
//! Closing the second window with its close button leaves the first one
//! running; closing the first ends the program. A headless run (`--shot`)
//! opens the second window at once as an offscreen image, and
//! `--second-shot <png>` writes that image.
//!
//! Wayland compositors place windows themselves, so the second window
//! opens wherever the compositor puts it.

use std::collections::VecDeque;

use yowie::gfx::TextStyle;
use yowie::input::Key;
use yowie::{
    Color, Config, Context, CursorGrab, Event, Game, GameResult, WindowConfig, WindowId, vec2,
};

/// Lines of event history kept for drawing.
const HISTORY: usize = 14;

struct Window {
    grabbed: bool,
    text_input: bool,
    /// The second window, once opened.
    second: Option<WindowId>,
    /// Recent events, newest last, as `window: event`.
    log: VecDeque<String>,
    /// The last event that reached the second window.
    second_last: String,
    second_shot: Option<String>,
    shot_taken: bool,
}

impl Window {
    fn open_second(&mut self, ctx: &mut Context) {
        let config = WindowConfig {
            title: "Yowie second window".into(),
            clear_color: Color::rgb8(40, 24, 44),
            ..WindowConfig::new("Second").with_size(420, 260)
        };
        let id = ctx.open_window(config);
        println!("opened window {id:?}");
        self.second = Some(id);
    }

    fn note(&mut self, window: WindowId, event: &Event) {
        let line = format!("{window:?}: {event:?}");
        println!("{line}");
        if Some(window) == self.second {
            self.second_last = format!("{event:?}");
        }
        self.log.push_back(line);
        while self.log.len() > HISTORY {
            self.log.pop_front();
        }
    }
}

impl Game for Window {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        println!(
            "window: {:?} at scale {}, adapter {}",
            ctx.window_size(),
            ctx.scale_factor(),
            ctx.gfx.adapter_info().name
        );
        let mut game = Window {
            grabbed: false,
            text_input: false,
            second: None,
            log: VecDeque::new(),
            second_last: String::new(),
            second_shot: std::env::args().skip_while(|a| a != "--second-shot").nth(1),
            shot_taken: false,
        };
        // A headless run cannot press N, so the second window opens now.
        if ctx.headless() {
            game.open_second(ctx);
        }
        Ok(game)
    }

    fn window_event(&mut self, _ctx: &mut Context, window: WindowId, event: &Event) -> GameResult {
        self.note(window, event);
        if *event == Event::Closed && Some(window) == self.second {
            self.second = None;
        }
        Ok(())
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let input = &ctx.input;
        let (f11, c, t, n, esc) = (
            input.key_pressed(Key::F11),
            input.key_pressed(Key::C),
            input.key_pressed(Key::T),
            input.key_pressed(Key::N),
            input.key_pressed(Key::Escape),
        );
        if f11 {
            let on = !ctx.fullscreen();
            ctx.set_fullscreen(on);
        }
        if c {
            self.grabbed = !self.grabbed;
            let grab = if self.grabbed {
                CursorGrab::Locked
            } else {
                CursorGrab::None
            };
            if let Err(e) = ctx.set_cursor_grab(grab) {
                println!("cursor grab: {e}");
            }
            ctx.set_cursor_visible(!self.grabbed);
        }
        if t {
            self.text_input = !self.text_input;
            ctx.set_text_input(self.text_input);
            println!("text input {}", if self.text_input { "on" } else { "off" });
        }
        if n {
            match self.second.take() {
                Some(id) => ctx.close_window(id),
                None => self.open_second(ctx),
            }
        }
        if esc {
            ctx.quit();
        }
        // The frame drawn last update is ready to read.
        if let (Some(path), Some(id)) = (&self.second_shot, self.second)
            && ctx.headless()
            && ctx.time.frame() >= 2
            && !self.shot_taken
        {
            self.shot_taken = true;
            let image = ctx.gfx.read_window_image(id)?;
            image.save(path)?;
            println!("wrote {path}");
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let font = ctx.gfx.default_mono_font();
        let width = ctx.gfx.view_size().x - 40.0;
        let style = TextStyle::new(14.0).color(Color::WHITE).width(width);
        let help = "F11 full screen, C cursor lock, T text input, N second window, Esc quits.";
        ctx.gfx.draw_text(&font, help, vec2(20.0, 20.0), &style);
        let dim = TextStyle::new(13.0)
            .color(Color::rgb8(170, 180, 200))
            .width(width);
        for (i, line) in self.log.iter().enumerate() {
            let y = 60.0 + i as f32 * 20.0;
            ctx.gfx.draw_text(&font, line, vec2(20.0, y), &dim);
        }

        if let Some(id) = self.second {
            let t = ctx.time.elapsed() as f32;
            let last = &self.second_last;
            let pointer = ctx.window_pointer(id);
            ctx.gfx.draw_in(id, |g| {
                let font = g.default_mono_font();
                let view = g.view_size();
                let style = TextStyle::new(14.0)
                    .color(Color::WHITE)
                    .width(view.x - 40.0);
                g.draw_text(&font, "Second window", vec2(20.0, 20.0), &style);
                g.draw_text(&font, &format!("view {view}"), vec2(20.0, 44.0), &style);
                let x = view.x * 0.5 + (view.x * 0.35) * (t * 1.5).sin();
                g.fill_circle(vec2(x, view.y * 0.5), 34.0, Color::rgb8(255, 170, 60));
                if let Some(p) = pointer {
                    g.fill_circle(p, 8.0, Color::rgb8(120, 220, 255));
                }
                let shown = format!("last: {last}");
                g.draw_text(&font, &shown, vec2(20.0, view.y - 60.0), &style);
            });
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(24, 28, 36),
        ..Config::new("Yowie window").with_size(800, 450)
    };
    yowie::run::<Window>(config)
}
