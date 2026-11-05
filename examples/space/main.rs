//! A ship under thrust in a fictional star system: seven planets with
//! thirteen moons, a belt of 300 rocks and a comet, every one on an exact
//! Kepler orbit around its primary. The ship is a free body that feels the
//! gravity of every massive body plus its own thrust, with its predicted
//! path drawn ahead in the frame of whatever it orbits. Masses, distances
//! and even `G` are game units.
//!
//! Drag to orbit the camera and the wheel zooms from the ship's hull out to
//! the whole system. Tab cycles the camera's focus, which is also the
//! floating origin that keeps the scene precise. W and S thrust prograde
//! and retrograde, A and D sideways, Q and E above and below the orbit.
//! The sliders set thrust and time warp. `--focus <name>`, `--dist <n>` and
//! `--warp <n>` set the starting view, and `--autoplay` burns prograde for
//! a second so a headless run shows a path that has changed. Escape quits.

mod path;
mod system;
mod view;

use path::{Around, PathCache};
use system::Cosmos;
use view::{Meshes, Viewpoint};
use yowie::gfx::Font;
use yowie::gfx::scene::{Mesh, MeshData};
use yowie::input::{Key, MouseButton};
use yowie::orbit::DVec3;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec3};

/// What the camera can follow.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Ship,
    Body(usize),
}

/// A longest slice of simulated time one integration step may cover.
const MAX_STEP: f64 = 0.25;

struct Space {
    cosmos: Cosmos,
    meshes: Meshes,
    font: Font,
    focus: Vec<Focus>,
    focused: usize,
    thrust: f32,
    warp: f32,
    yaw: f32,
    pitch: f32,
    dist: f32,
    path: PathCache,
    window: Rect,
    autoplay: bool,
}

fn arg(name: &str) -> Option<String> {
    std::env::args().skip_while(|a| a != name).nth(1)
}

impl Space {
    fn focus_name(&self, f: Focus) -> &str {
        match f {
            Focus::Ship => "Ship",
            Focus::Body(i) => &self.cosmos.bodies[i].name,
        }
    }

    fn focus_pos(&self, f: Focus) -> DVec3 {
        match f {
            Focus::Ship => self.cosmos.ship_state().pos,
            Focus::Body(i) => self.cosmos.body_pos(i),
        }
    }

    /// The thrust direction keys give, in system coordinates: along the
    /// ship's velocity, across it, or out of its orbital plane.
    fn thrust_vector(&self, ctx: &Context, burn_forward: bool) -> DVec3 {
        let prograde = self.cosmos.ship_state().vel.normalize_or(DVec3::X);
        let side = prograde.cross(DVec3::Z).normalize_or(DVec3::Y);
        let up = side.cross(prograde);
        let mut dir = DVec3::ZERO;
        let keys = [
            (Key::W, prograde),
            (Key::S, -prograde),
            (Key::D, side),
            (Key::A, -side),
            (Key::Q, up),
            (Key::E, -up),
        ];
        for (key, d) in keys {
            if ctx.input.key_down(key) {
                dir += d;
            }
        }
        if burn_forward {
            dir += prograde;
        }
        dir * f64::from(self.thrust)
    }
}

impl Game for Space {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let cosmos = Cosmos::new();
        let mut focus = vec![Focus::Ship, Focus::Body(0)];
        focus.extend(
            cosmos
                .bodies
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, b)| b.focusable)
                .map(|(i, _)| Focus::Body(i)),
        );
        let mut app = Space {
            cosmos,
            meshes: Meshes {
                sphere: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 16, 32))?,
                hull: Mesh::new(&ctx.gfx, &MeshData::capsule(0.4, 0.9, 6, 12))?,
            },
            font: ctx.gfx.default_font(),
            focus,
            focused: 0,
            thrust: 0.01,
            warp: arg("--warp").and_then(|v| v.parse().ok()).unwrap_or(1.0),
            yaw: 0.6,
            pitch: 0.35,
            dist: arg("--dist").and_then(|v| v.parse().ok()).unwrap_or(14.0),
            path: PathCache::default(),
            window: Rect::new(12.0, 12.0, 360.0, 330.0),
            autoplay: std::env::args().any(|a| a == "--autoplay"),
        };
        if let Some(name) = arg("--focus") {
            let found = app.focus.iter().position(|f| app.focus_name(*f) == name);
            app.focused = found.unwrap_or(0);
        }
        Ok(app)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let free = !ctx.input_captured();
        if free && ctx.input.key_pressed(Key::Tab) {
            self.focused = (self.focused + 1) % self.focus.len();
        }
        if free && !ctx.ui.wants_mouse() {
            if ctx.input.mouse_down(MouseButton::Left) {
                let d = ctx.input.mouse_delta();
                self.yaw -= d.x * 0.01;
                self.pitch = (self.pitch + d.y * 0.01).clamp(-1.5, 1.5);
            }
            let wheel = ctx.input.wheel().y;
            if wheel != 0.0 {
                self.dist = (self.dist * 0.88f32.powf(wheel)).clamp(0.5, 3000.0);
            }
        }
        let t = ctx.time.elapsed();
        let burn = self.autoplay && (0.5..1.5).contains(&t);
        let accel = if free {
            self.thrust_vector(ctx, burn)
        } else {
            DVec3::ZERO
        };
        let _ = self.cosmos.sys.set_thrust(self.cosmos.ship, accel);

        // Time warp: simulated time advances `warp` times faster, in steps
        // short enough for the ship's integrator.
        let span = f64::from(ctx.time.delta()) * f64::from(self.warp);
        let steps = (span / MAX_STEP).ceil().max(1.0);
        for _ in 0..steps as u32 {
            self.cosmos.sys.step(span / steps);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let focus = self.focus[self.focused];
        let origin = self.focus_pos(focus);
        let around = Around::of(&self.cosmos.sys, self.cosmos.ship);
        let path: Vec<Vec3> = around
            .as_ref()
            .map(|a| {
                let points = self
                    .path
                    .get(&self.cosmos.sys, self.cosmos.ship, a, self.warp);
                points.iter().map(|p| (*p - origin).as_vec3()).collect()
            })
            .unwrap_or_default();
        let accel = self
            .cosmos
            .sys
            .ship(self.cosmos.ship)
            .map_or(DVec3::ZERO, |s| s.thrust)
            .as_vec3();
        let v = Viewpoint {
            origin,
            dist: self.dist,
            thrust: accel,
        };
        let cam = view::camera(self.yaw, self.pitch, self.dist);
        let (cosmos, meshes) = (&self.cosmos, &self.meshes);
        ctx.gfx
            .scene(&cam, |s| view::record(s, cosmos, meshes, &v, &path));
        view::labels(ctx, &self.font, &cam, &self.cosmos, &v);
        let name = self.focus_name(focus).to_owned();
        view::window(
            ctx,
            &mut self.window,
            &self.cosmos,
            around.as_ref(),
            &name,
            &mut self.thrust,
            &mut self.warp,
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(2, 2, 6),
        ..Config::new("Yowie space").with_size(1024, 680)
    };
    yowie::run::<Space>(config)
}
