//! Particle systems: a campfire of fire and smoke, rain from a line along
//! the top of the screen, sparks where the mouse is clicked and a confetti
//! burst on Space. Up and Down change the fire's rate, Left and Right its
//! gravity; the values show as text. Escape quits. Run headless
//! (`--shot`), the example clicks and bursts by itself at fixed times.

use yowie::gfx::{Font, TextStyle, Texture, TextureOptions};
use yowie::input::{Key, MouseButton};
use yowie::particle::{Emitter, Shape, System, presets};
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

const HEARTH: Vec2 = vec2(480.0, 430.0);

struct Particles {
    font: Font,
    mono: Font,
    soft: Texture,
    fire: System,
    smoke: System,
    rain: System,
    /// Short-lived bursts, dropped once they finish.
    bursts: Vec<System>,
    fire_rate: f32,
    gravity: f32,
    clock: f32,
    scripted: u8,
    seed: u64,
}

impl Particles {
    fn spark(&mut self, at: Vec2) {
        self.seed += 1;
        let emitter = Emitter {
            seed: self.seed,
            ..presets::sparks()
        };
        let mut s = System::at(emitter, at);
        s.set_texture(self.soft.clone());
        self.bursts.push(s);
    }

    fn confetti(&mut self) {
        self.seed += 1;
        let emitter = Emitter {
            seed: self.seed,
            ..presets::confetti()
        };
        self.bursts.push(System::at(emitter, vec2(480.0, 340.0)));
    }
}

impl Game for Particles {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let img = presets::soft_circle(64);
        let soft = Texture::from_image(&ctx.gfx, &img, TextureOptions::smooth())?;

        let mut fire = System::at(
            Emitter {
                layer: 2,
                prewarm: 1.5,
                ..presets::fire()
            },
            HEARTH,
        );
        fire.set_texture(soft.clone());

        let mut smoke = System::at(
            Emitter {
                layer: 1,
                prewarm: 3.0,
                ..presets::smoke()
            },
            HEARTH + vec2(0.0, -30.0),
        );
        smoke.set_texture(soft.clone());

        let mut rain_emitter = Emitter {
            layer: 3,
            prewarm: 2.0,
            shape: Shape::Line {
                to: vec2(1040.0, 0.0),
            },
            ..presets::rain()
        };
        rain_emitter.max = 900;
        let rain = System::at(rain_emitter, vec2(-40.0, -20.0));

        Ok(Particles {
            font: ctx.gfx.default_font(),
            mono: ctx.gfx.default_mono_font(),
            soft,
            fire_rate: fire.emitter().rate,
            gravity: fire.emitter().gravity.y,
            fire,
            smoke,
            rain,
            bursts: Vec::new(),
            clock: 0.0,
            scripted: 0,
            seed: 100,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        self.clock += dt;
        let input = &ctx.input;
        if input.key_down(Key::Up) {
            self.fire_rate = (self.fire_rate + 60.0 * dt).min(300.0);
        }
        if input.key_down(Key::Down) {
            self.fire_rate = (self.fire_rate - 60.0 * dt).max(10.0);
        }
        if input.key_down(Key::Right) {
            self.gravity = (self.gravity + 60.0 * dt).min(100.0);
        }
        if input.key_down(Key::Left) {
            self.gravity = (self.gravity - 60.0 * dt).max(-200.0);
        }
        let (clicked, at, space) = (
            input.mouse_pressed(MouseButton::Left),
            input.mouse(),
            input.key_pressed(Key::Space),
        );
        if clicked {
            self.spark(at);
        }
        if space {
            self.confetti();
        }
        // Headless runs play a fixed script so the frame is the same each
        // time.
        if ctx.headless() {
            if self.scripted == 0 && self.clock >= 1.0 {
                self.spark(vec2(260.0, 300.0));
                self.spark(vec2(700.0, 330.0));
                self.scripted = 1;
            }
            if self.scripted == 1 && self.clock >= 1.2 {
                self.confetti();
                self.scripted = 2;
            }
        }

        let mut e = self.fire.emitter().clone();
        e.rate = self.fire_rate;
        e.gravity.y = self.gravity;
        self.fire.set_emitter(e);

        self.fire.update(dt);
        self.smoke.update(dt);
        self.rain.update(dt);
        for b in &mut self.bursts {
            b.update(dt);
        }
        self.bursts.retain(|b| !b.finished());
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let g = &mut ctx.gfx;
        let view = g.view_size();
        // Sky, ground and the hearth, behind every particle layer.
        g.set_layer(-2);
        g.fill_rect(Rect::new(0.0, 0.0, view.x, 400.0), Color::rgb8(14, 18, 34));
        g.fill_rect(
            Rect::new(0.0, 400.0, view.x, view.y - 400.0),
            Color::rgb8(30, 26, 24),
        );
        for i in 0..7 {
            let a = i as f32 / 7.0 * std::f32::consts::TAU;
            let p = HEARTH + vec2(a.cos() * 44.0, 14.0 + a.sin() * 12.0);
            g.fill_circle(p, 8.0, Color::rgb8(96, 94, 100));
        }
        g.set_layer(0);
        g.fill_rect(
            Rect::new(HEARTH.x - 32.0, HEARTH.y + 4.0, 64.0, 10.0),
            Color::rgb8(86, 56, 34),
        );
        g.fill_rect(
            Rect::new(HEARTH.x - 24.0, HEARTH.y - 4.0, 48.0, 9.0),
            Color::rgb8(104, 68, 40),
        );

        self.smoke.draw(g);
        self.fire.draw(g);
        self.rain.draw(g);
        for b in &self.bursts {
            b.draw(g);
        }

        g.set_layer(yowie::gfx::layers::UI);
        let ink = Color::rgb8(232, 236, 250);
        g.draw_text(
            &self.font,
            "Yowie particles",
            vec2(14.0, 10.0),
            &TextStyle::new(22.0).color(Color::rgb8(255, 220, 120)),
        );
        g.draw_text(
            &self.font,
            "Click for sparks, Space for confetti, Up/Down fire rate, Left/Right gravity",
            vec2(14.0, 38.0),
            &TextStyle::new(14.0).color(ink),
        );
        let s = g.stats();
        let values = format!(
            "fire rate {:>5.0}/s   gravity {:>5.0}\nalive: fire {:>3}  smoke {:>3}  rain {:>3}  bursts {}\ndraw calls {}  vertices {}",
            self.fire_rate,
            self.gravity,
            self.fire.alive(),
            self.smoke.alive(),
            self.rain.alive(),
            self.bursts.len(),
            s.draw_calls,
            s.vertices
        );
        g.draw_text(
            &self.mono,
            &values,
            vec2(14.0, 62.0),
            &TextStyle::new(13.0).color(Color::rgb8(150, 220, 170)),
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(8, 10, 20),
        ..Config::new("Yowie particles").with_size(960, 540)
    };
    yowie::run::<Particles>(config)
}
