//! Particle systems: a campfire of fire and smoke, rain from a line along
//! the top of the screen, sparks where the mouse is clicked and a confetti
//! burst on Space, with a tuning panel for the fire's emitter and a large
//! fountain simulated on the GPU.
//!
//! Up and Down change the fire's rate, Left and Right its gravity, and G
//! moves the fire, smoke, rain and bursts between the CPU system and the
//! GPU compute path (the fountain always runs on the GPU when the device
//! can). The panel's sliders edit the fire's emitter live. Escape quits.
//! `--gpu` starts with the effects on the GPU. Run headless (`--shot`), the
//! example clicks and bursts by itself at fixed times.

use yowie::gfx::{Font, TextStyle, Texture, TextureOptions};
use yowie::input::{Key, MouseButton};
use yowie::particle::{Backend, ColorKey, Curve, Emitter, Particles as Effect, Range, Shape, presets};
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

const HEARTH: Vec2 = vec2(480.0, 430.0);
const FOUNTAIN: Vec2 = vec2(70.0, 520.0);

struct Particles {
    font: Font,
    mono: Font,
    soft: Texture,
    fire: Effect,
    smoke: Effect,
    rain: Effect,
    fountain: Effect,
    /// Short-lived bursts, dropped once they finish.
    bursts: Vec<Effect>,
    /// The fire's emitter as the panel edits it.
    tuned: Emitter,
    panel: Rect,
    backend: Backend,
    clock: f32,
    scripted: u8,
    seed: u64,
}

/// The fountain: a thin jet that sprays up and to the right and falls back
/// under gravity. On the GPU it holds a hundred thousand particles; on a
/// device without compute it is a small CPU effect instead.
fn fountain(big: bool) -> Emitter {
    Emitter {
        rate: if big { 32_000.0 } else { 700.0 },
        lifetime: Range::new(2.4, 3.6),
        shape: Shape::Circle {
            radius: 5.0,
            edge: false,
        },
        direction: -std::f32::consts::FRAC_PI_2 + 0.5,
        spread: 0.45,
        speed: Range::new(300.0, 430.0),
        gravity: vec2(0.0, 270.0),
        drag: 0.04,
        size: Range::new(2.0, 4.0),
        color_over_life: vec![
            ColorKey {
                t: 0.0,
                color: Color::rgb8(170, 240, 255),
            },
            ColorKey {
                t: 1.0,
                color: Color::rgb8(40, 90, 255),
            },
        ],
        alpha_over_life: Curve::keys(&[(0.0, 0.0), (0.05, 0.5), (0.8, 0.4), (1.0, 0.0)]),
        blend: yowie::particle::Blend::Additive,
        layer: 1,
        world_space: true,
        max: if big { 100_000 } else { 2_000 },
        prewarm: 3.6,
        seed: 7,
        ..Emitter::default()
    }
}

impl Particles {
    fn spawn(&self, gfx: &yowie::gfx::Graphics, emitter: Emitter, at: Vec2) -> Effect {
        let mut e = Effect::at(gfx, emitter, at, self.backend);
        e.set_texture(self.soft.clone());
        e
    }

    fn spark(&mut self, gfx: &yowie::gfx::Graphics, at: Vec2) {
        self.seed += 1;
        let emitter = Emitter {
            seed: self.seed,
            ..presets::sparks()
        };
        let e = self.spawn(gfx, emitter, at);
        self.bursts.push(e);
    }

    fn confetti(&mut self, gfx: &yowie::gfx::Graphics) {
        self.seed += 1;
        let emitter = Emitter {
            seed: self.seed,
            ..presets::confetti()
        };
        let mut e = Effect::at(gfx, emitter, vec2(480.0, 340.0), self.backend);
        e.set_position(vec2(480.0, 340.0));
        self.bursts.push(e);
    }

    fn set_backend(&mut self, gfx: &yowie::gfx::Graphics, backend: Backend) {
        self.backend = backend;
        for e in [&mut self.fire, &mut self.smoke, &mut self.rain] {
            e.switch_to(gfx, backend);
        }
        for b in &mut self.bursts {
            b.switch_to(gfx, backend);
        }
    }
}

impl Game for Particles {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let img = presets::soft_circle(64);
        let soft = Texture::from_image(&ctx.gfx, &img, TextureOptions::smooth())?;
        let backend = if std::env::args().any(|a| a == "--gpu") {
            Backend::Gpu
        } else {
            Backend::Cpu
        };
        let gfx = &ctx.gfx;
        let make = |emitter: Emitter, at: Vec2, backend: Backend| {
            let mut e = Effect::at(gfx, emitter, at, backend);
            e.set_texture(soft.clone());
            e
        };

        let tuned = Emitter {
            layer: 2,
            prewarm: 1.5,
            ..presets::fire()
        };
        let fire = make(tuned.clone(), HEARTH, backend);
        let smoke = make(
            Emitter {
                layer: 1,
                prewarm: 3.0,
                ..presets::smoke()
            },
            HEARTH + vec2(0.0, -30.0),
            backend,
        );
        let mut rain_emitter = Emitter {
            layer: 3,
            prewarm: 2.0,
            shape: Shape::Line {
                to: vec2(1040.0, 0.0),
            },
            ..presets::rain()
        };
        rain_emitter.max = 900;
        let rain = make(rain_emitter, vec2(-40.0, -20.0), backend);
        let big = yowie::particle::gpu_supported(&ctx.gfx);
        let fountain = make(fountain(big), FOUNTAIN, Backend::Gpu);

        Ok(Particles {
            font: ctx.gfx.default_font(),
            mono: ctx.gfx.default_mono_font(),
            soft,
            fire,
            smoke,
            rain,
            fountain,
            bursts: Vec::new(),
            tuned: Emitter {
                prewarm: 0.0,
                ..tuned
            },
            panel: Rect::new(724.0, 150.0, 224.0, 330.0),
            backend,
            clock: 0.0,
            scripted: 0,
            seed: 100,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) && !ctx.input_captured() {
            ctx.quit();
        }
        self.clock += dt;
        let (clicked, at, space, toggle) = {
            let input = &ctx.input;
            if !ctx.input_captured() {
                if input.key_down(Key::Up) {
                    self.tuned.rate = (self.tuned.rate + 60.0 * dt).min(300.0);
                }
                if input.key_down(Key::Down) {
                    self.tuned.rate = (self.tuned.rate - 60.0 * dt).max(10.0);
                }
                if input.key_down(Key::Right) {
                    self.tuned.gravity.y = (self.tuned.gravity.y + 60.0 * dt).min(100.0);
                }
                if input.key_down(Key::Left) {
                    self.tuned.gravity.y = (self.tuned.gravity.y - 60.0 * dt).max(-200.0);
                }
            }
            (
                input.mouse_pressed(MouseButton::Left) && !ctx.input_captured(),
                input.mouse(),
                input.key_pressed(Key::Space) && !ctx.input_captured(),
                input.key_pressed(Key::G) && !ctx.input_captured(),
            )
        };
        if toggle {
            let next = match self.backend {
                Backend::Cpu => Backend::Gpu,
                Backend::Gpu => Backend::Cpu,
            };
            self.set_backend(&ctx.gfx, next);
        }
        if clicked {
            self.spark(&ctx.gfx, at);
        }
        if space {
            self.confetti(&ctx.gfx);
        }
        // Headless runs play a fixed script so the frame is the same each
        // time.
        if ctx.headless() {
            if self.scripted == 0 && self.clock >= 1.0 {
                self.spark(&ctx.gfx, vec2(260.0, 300.0));
                self.spark(&ctx.gfx, vec2(700.0, 330.0));
                self.scripted = 1;
            }
            if self.scripted == 1 && self.clock >= 1.2 {
                self.confetti(&ctx.gfx);
                self.scripted = 2;
            }
        }

        self.fire.set_emitter(self.tuned.clone());
        self.fire.update(dt);
        self.smoke.update(dt);
        self.rain.update(dt);
        self.fountain.update(dt);
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
        g.fill_rect(
            Rect::new(FOUNTAIN.x - 16.0, FOUNTAIN.y - 4.0, 32.0, 24.0),
            Color::rgb8(70, 72, 84),
        );

        self.smoke.draw(g);
        self.fire.draw(g);
        self.rain.draw(g);
        self.fountain.draw(g);
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
            "Click for sparks, Space for confetti, G for the GPU path, Up/Down rate, Left/Right gravity",
            vec2(14.0, 38.0),
            &TextStyle::new(14.0).color(ink),
        );
        let s = g.stats();
        let backend = match self.fire.backend() {
            Backend::Cpu => "CPU",
            Backend::Gpu => "GPU",
        };
        // A headless frame reads the exact GPU count so the picture is the
        // same each run; a window shows the count a few frames late.
        let headless = ctx.headless();
        let live = |e: &Effect| if headless { e.read_alive() } else { e.alive() };
        let fountain_live = live(&self.fountain);
        let values = format!(
            "effects on the {backend}   fire rate {:>5.0}/s   gravity {:>5.0}\nalive: fire {:>3}  smoke {:>3}  rain {:>3}  bursts {}\nGPU fountain: {fountain_live} live of {}\ndraw calls {}  vertices {}",
            self.tuned.rate,
            self.tuned.gravity.y,
            live(&self.fire),
            live(&self.smoke),
            live(&self.rain),
            self.bursts.len(),
            self.fountain.emitter().max,
            s.draw_calls,
            s.vertices
        );
        ctx.gfx.draw_text(
            &self.mono,
            &values,
            vec2(14.0, 62.0),
            &TextStyle::new(13.0).color(Color::rgb8(150, 220, 170)),
        );
        self.tuning_panel(ctx);
        Ok(())
    }
}

impl Particles {
    /// The panel of sliders that edit the fire's emitter while it runs.
    fn tuning_panel(&mut self, ctx: &mut Context) {
        let mut tuned = self.tuned.clone();
        let mut panel = self.panel;
        let mut gpu = self.backend == Backend::Gpu;
        let was_gpu = gpu;
        let mut additive = tuned.blend == yowie::particle::Blend::Additive;
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.window("Fire emitter", &mut panel, |f| {
                f.slider("Rate", &mut tuned.rate, 10.0..=300.0);
                f.slider("Speed", &mut tuned.speed.max, 40.0..=220.0);
                f.slider("Gravity", &mut tuned.gravity.y, -200.0..=100.0);
                f.slider("Size", &mut tuned.size.max, 10.0..=60.0);
                f.slider("Spread", &mut tuned.spread, 0.0..=3.0);
                f.slider("Drag", &mut tuned.drag, 0.0..=2.0);
                f.checkbox("Additive", &mut additive);
                f.checkbox("GPU effects (G)", &mut gpu);
                if f.button("Reset fire").clicked {
                    tuned = Emitter {
                        layer: 2,
                        ..presets::fire()
                    };
                }
            });
        });
        tuned.blend = if additive {
            yowie::particle::Blend::Additive
        } else {
            yowie::particle::Blend::Alpha
        };
        tuned.size.min = tuned.size.min.min(tuned.size.max);
        tuned.speed.min = tuned.speed.min.min(tuned.speed.max);
        self.tuned = tuned;
        self.panel = panel;
        if gpu != was_gpu {
            let next = if gpu { Backend::Gpu } else { Backend::Cpu };
            self.set_backend(&ctx.gfx, next);
        }
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(8, 10, 20),
        ..Config::new("Yowie particles").with_size(960, 540)
    };
    yowie::run::<Particles>(config)
}
