//! The bundled fonts at several sizes, with wrapping and the four
//! alignments. Escape quits.

use yowie::gfx::{Font, TextAlign, TextStyle};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, vec2};

const STORY: &str = "The yowie keeps to the ridges above the tree line, where the \
mist settles each evening. Walkers who stay on the track report footprints the size \
of a dinner plate, and a smell of wet bark that follows them down to the creek.";

struct Text {
    regular: Font,
    mono: Font,
}

impl Game for Text {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        Ok(Text {
            regular: ctx.gfx.default_font(),
            mono: ctx.gfx.default_mono_font(),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let g = &mut ctx.gfx;
        let ink = Color::rgb8(235, 232, 220);
        let mut y = 16.0;
        for size in [12.0, 16.0, 24.0, 36.0, 48.0] {
            let line = format!("Go Regular {size}: judge my vow");
            g.draw_text(
                &self.regular,
                &line,
                vec2(20.0, y),
                &TextStyle::new(size).color(ink),
            );
            y += size * 1.3;
        }
        let mono = TextStyle::new(16.0).color(Color::rgb8(140, 220, 160));
        g.draw_text(
            &self.mono,
            "Go Mono 16: fn main() { println!(\"{}\", 6 * 7); }",
            vec2(20.0, y + 4.0),
            &mono,
        );

        let top = y + 44.0;
        let width = 215.0;
        let aligns = [
            (TextAlign::Left, "Left"),
            (TextAlign::Center, "Center"),
            (TextAlign::Right, "Right"),
            (TextAlign::Justified, "Justified"),
        ];
        for (i, (align, name)) in aligns.into_iter().enumerate() {
            let x = 20.0 + i as f32 * (width + 20.0);
            let style = TextStyle::new(14.0).color(ink).width(width).align(align);
            let size = g.measure_text(&self.regular, STORY, &style);
            g.fill_rect(
                Rect::new(x - 6.0, top - 6.0, width + 12.0, size.y + 30.0),
                Color::rgb8(36, 40, 56),
            );
            g.stroke_rect(
                Rect::new(x - 6.0, top - 6.0, width + 12.0, size.y + 30.0),
                1.0,
                Color::rgb8(90, 100, 140),
            );
            let label = TextStyle::new(12.0).color(Color::rgb8(250, 200, 90));
            g.draw_text(&self.mono, name, vec2(x, top), &label);
            g.draw_text(&self.regular, STORY, vec2(x, top + 20.0), &style);
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(20, 22, 30),
        ..Config::new("Yowie text").with_size(960, 540)
    };
    yowie::run::<Text>(config)
}
