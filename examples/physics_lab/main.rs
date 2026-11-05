//! Capsules, hulls and spheres tumbling onto a mesh terrain, a chain of
//! hinge joints with a ball on the end, a paddle wheel turned by a hinge
//! motor, a ragdoll dropped from the sky and a character controller
//! walking a staircase, with colliders, joints and contacts drawn as debug
//! lines and the debug console driving the world.
//!
//! Drag with the left button to orbit, the wheel zooms, R throws the bodies
//! again, D toggles the debug lines, Escape quits. The console (`` ` ``)
//! has `spawn`, `reset`, `ragdoll`, `kick`, `pause`, `debug` and `gravity`,
//! and the variables `lab.gravity`, `lab.paused`, `lab.debug`,
//! `lab.contacts`, `wheel.speed` and `wheel.torque`. `--exec "a; b"` runs
//! console commands at start.

mod commands;
mod lab;
mod meshes;
mod view;

use yowie::input::Key;
use yowie::{Config, Context, Game, GameResult, OrbitCamera, Rect, Vec3};

use commands::{Queue, Request, Vars};
use lab::Lab;
use view::Meshes;

struct PhysicsLab {
    lab: Lab,
    meshes: Meshes,
    orbit: OrbitCamera,
    vars: Vars,
    queue: Queue,
    window: Rect,
    debug: bool,
    last_gravity: f32,
    last_wheel: (f32, f32),
}

impl PhysicsLab {
    fn handle(&mut self, request: Request, ctx: &mut Context) -> GameResult {
        match request {
            Request::Spawn(count, kind) => {
                let n = self.lab.spawn(count, kind)?;
                if n < count {
                    ctx.console.print(&format!("limit reached, threw {n}"));
                }
            }
            Request::Reset => self.lab.reset()?,
            Request::Ragdoll => self.lab.drop_ragdoll()?,
            Request::Kick(speed) => self.lab.kick(speed),
        }
        Ok(())
    }

    /// Applies the console variables that need pushing into the world.
    fn sync_vars(&mut self) {
        let g = self.vars.gravity.get();
        if g != self.last_gravity {
            self.last_gravity = g;
            self.lab.world.set_gravity(Vec3::new(0.0, -g, 0.0));
        }
        let wheel = (self.vars.wheel_speed.get(), self.vars.wheel_torque.get());
        if wheel != self.last_wheel {
            self.last_wheel = wheel;
            self.lab.set_wheel(wheel.0, wheel.1);
        }
        self.lab.world.set_debug_contacts(self.vars.contacts.get());
    }
}

/// Reads the text after `--exec`, if given.
fn exec_arg() -> Option<String> {
    let mut args = std::env::args();
    args.by_ref().find(|a| a == "--exec")?;
    args.next()
}

impl Game for PhysicsLab {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let terrain = meshes::terrain();
        let lab = Lab::new(&terrain)?;
        let meshes = Meshes::new(&ctx.gfx, &terrain, &lab)?;
        let (vars, queue) = commands::register(&mut ctx.console);
        if let Some(line) = exec_arg() {
            for command in line.split(';') {
                ctx.console.execute(command.trim());
            }
        }
        let mut orbit = OrbitCamera::new(Vec3::new(2.0, 2.0, 2.0), 0.6, 0.5, 27.0);
        orbit.min_distance = 8.0;
        orbit.max_distance = 120.0;
        orbit.lens.far = 500.0;
        Ok(PhysicsLab {
            lab,
            meshes,
            orbit,
            last_gravity: 9.8,
            last_wheel: (1.5, 400.0),
            vars,
            queue,
            window: Rect::new(12.0, 340.0, 330.0, 328.0),
            debug: true,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::R) {
            self.lab.reset()?;
        }
        if ctx.input.key_pressed(Key::D) {
            self.vars.debug.set(!self.vars.debug.get());
        }
        let requests: Vec<Request> = self.queue.borrow_mut().drain(..).collect();
        for r in requests {
            self.handle(r, ctx)?;
        }
        self.sync_vars();
        self.debug = self.vars.debug.get();
        let dt = ctx.time.delta();
        if !ctx.ui.wants_mouse() && !ctx.input_captured() {
            self.orbit.update(&ctx.input, dt);
        }
        self.orbit.pitch = self.orbit.pitch.clamp(0.05, 1.5);
        self.lab.step(dt, self.vars.paused.get());
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let cam = self.orbit.camera();
        let alpha = ctx.time.alpha();
        ctx.gfx.scene(&cam, |s| {
            self.meshes.draw(s, &mut self.lab, alpha, self.debug);
        });
        self.hud(ctx);
        Ok(())
    }
}

impl PhysicsLab {
    fn hud(&mut self, ctx: &mut Context) {
        let (asleep, total) = self.lab.sleeping();
        let spin = self
            .lab
            .world
            .angular_velocity(self.lab.wheel)
            .map_or(0.0, |w| w.z);
        let mut gravity = self.vars.gravity.get();
        let mut torque = self.vars.wheel_torque.get();
        let mut debug = self.vars.debug.get();
        let mut paused = self.vars.paused.get();
        let (mut again, mut more) = (false, false);
        let (hero, bodies) = (self.lab.hero.grounded, self.lab.debris.len());
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.window("Physics lab", &mut self.window, |f| {
                f.label(&format!(
                    "{bodies} thrown bodies, {asleep} of {total} asleep"
                ));
                f.label(&format!(
                    "wheel {spin:.1} rad/s, character grounded: {hero}"
                ));
                f.slider("Gravity", &mut gravity, 0.0..=20.0);
                f.slider("Wheel torque", &mut torque, 0.0..=800.0);
                f.checkbox("Debug lines", &mut debug);
                f.checkbox("Paused", &mut paused);
                again = f.button("Throw again (R)").clicked;
                more = f.button("Spawn 10").clicked;
                f.label("drag orbits, wheel zooms, ` opens the console");
            });
        });
        self.vars.gravity.set(gravity);
        self.vars.wheel_torque.set(torque);
        self.vars.debug.set(debug);
        self.vars.paused.set(paused);
        if again {
            self.queue.borrow_mut().push_back(Request::Reset);
        }
        if more {
            self.queue.borrow_mut().push_back(Request::Spawn(10, None));
        }
    }
}

fn main() -> Result<(), yowie::Error> {
    let mut config = Config::new("Yowie physics lab").with_size(1024, 680);
    config.console = true;
    yowie::run::<PhysicsLab>(config)
}
