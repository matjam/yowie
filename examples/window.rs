//! The platform layer's smoke test: a window that prints every event as it
//! arrives. F11 toggles full screen, C toggles a locked cursor, T toggles
//! text input (input-method composition), Escape quits.

use yowie::gfx::TextStyle;
use yowie::input::Key;
use yowie::{Color, Config, Context, CursorGrab, Event, Game, GameResult, vec2};

struct Window {
    grabbed: bool,
    text_input: bool,
    last: String,
}

impl Game for Window {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        println!(
            "window: {:?} at scale {}, adapter {}",
            ctx.window_size(),
            ctx.scale_factor(),
            ctx.gfx.adapter_info().name
        );
        Ok(Window {
            grabbed: false,
            text_input: false,
            last: String::new(),
        })
    }

    fn event(&mut self, _ctx: &mut Context, event: &Event) -> GameResult {
        println!("{event:?}");
        self.last = format!("{event:?}");
        Ok(())
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let input = &ctx.input;
        let (f11, c, t, esc) = (
            input.key_pressed(Key::F11),
            input.key_pressed(Key::C),
            input.key_pressed(Key::T),
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
        if esc {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let font = ctx.gfx.default_mono_font();
        let width = ctx.gfx.view_size().x - 40.0;
        let style = TextStyle::new(14.0).color(Color::WHITE).width(width);
        let help =
            "Events print to stdout. F11 full screen, C cursor lock, T text input, Esc quits.";
        ctx.gfx.draw_text(&font, help, vec2(20.0, 20.0), &style);
        let last = format!("last event: {}", self.last);
        ctx.gfx.draw_text(&font, &last, vec2(20.0, 60.0), &style);
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
