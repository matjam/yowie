//! The renderer's smoke test: a window cleared to a colour that cycles
//! through the hues. Escape quits.

use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult};

struct Clear;

impl Game for Clear {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Clear)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let hue = (ctx.time.elapsed() * 40.0) as f32;
        ctx.gfx.set_clear_color(Color::hsv(hue, 0.6, 0.8));
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Clear>(Config::new("Yowie clear").with_size(320, 180))
}
