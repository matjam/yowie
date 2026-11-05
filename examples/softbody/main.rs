//! A cloth flag flapping on a pole in a swinging gust, a jelly cube that
//! drops and can be kicked beside a rigid crate, and a tank of 2D fluid
//! breaking around a post.
//!
//! The flag and the jelly are soft bodies from `yowie_phys::soft`: their
//! particles are rewritten into a mesh every frame and they collide with
//! the floor, the pole and the ball. The fluid is drawn in a 2D panel over
//! the scene.
//!
//! Drag with the left button to orbit, the wheel zooms. A click or Space
//! kicks the jelly, R starts over, Escape quits. `--kick <seconds>` kicks
//! once at that game time.

mod scene;
mod tank;

use yowie::gfx::scene::{DirectionalLight, Material, Mesh, MeshData, Sky};
use yowie::input::{Key, MouseButton};
use yowie::{
    Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, Transform, Vec2, Vec3,
};

use scene::SoftScene;

struct Softbody {
    sim: SoftScene,
    cube: Mesh,
    ball: Mesh,
    flag: Mesh,
    jelly: Mesh,
    orbit: OrbitCamera,
    window: Rect,
    press: Vec2,
    kick_at: Option<f32>,
}

/// The seconds after `--kick`, if given.
fn kick_arg() -> Option<f32> {
    let mut args = std::env::args();
    args.by_ref().find(|a| a == "--kick")?;
    args.next()?.parse().ok()
}

impl Game for Softbody {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let sim = SoftScene::new()?;
        let g = &ctx.gfx;
        let mut orbit = OrbitCamera::new(Vec3::new(0.6, 1.6, 0.0), 0.9, 0.35, 12.0);
        orbit.min_distance = 4.0;
        orbit.max_distance = 40.0;
        Ok(Softbody {
            cube: Mesh::new(g, &MeshData::cube(1.0))?,
            ball: Mesh::new(g, &MeshData::sphere(1.0, 16, 24))?,
            flag: Mesh::new(g, &sim.cloth_data)?,
            jelly: Mesh::new(g, &sim.jelly_data)?,
            sim,
            orbit,
            window: Rect::new(24.0, 520.0, 300.0, 160.0),
            press: Vec2::ZERO,
            kick_at: kick_arg(),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::R) {
            self.sim.reset()?;
        }
        let mut kick = ctx.input.key_pressed(Key::Space);
        let over_ui = ctx.ui.wants_mouse() || ctx.input_captured();
        if !over_ui {
            // A click that did not drag the camera kicks the jelly.
            let mouse = ctx.input.mouse();
            if ctx.input.mouse_pressed(MouseButton::Left) {
                self.press = mouse;
            }
            if ctx.input.mouse_released(MouseButton::Left) && mouse.distance(self.press) < 4.0 {
                kick = true;
            }
            self.orbit.update(&ctx.input, ctx.time.delta());
        }
        if let Some(t) = self.kick_at
            && ctx.time.elapsed() >= f64::from(t)
        {
            self.kick_at = None;
            kick = true;
        }
        if kick {
            self.sim.kick();
        }
        self.orbit.pitch = self.orbit.pitch.clamp(0.05, 1.4);
        self.sim.step(ctx.time.delta());
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        self.flag.update(&ctx.gfx, &self.sim.cloth_data)?;
        self.jelly.update(&ctx.gfx, &self.sim.jelly_data)?;
        // The scene fills the window to the right of the tank panel.
        let size = ctx.gfx.view_size();
        let viewport = Rect::new(320.0, 0.0, (size.x - 320.0).max(1.0), size.y);
        let cam = self.orbit.camera().with_viewport(viewport);
        let alpha = ctx.time.alpha();
        ctx.gfx.scene(&cam, |s| {
            s.set_sun(
                DirectionalLight::new(Vec3::new(-0.6, -1.0, -0.4), Color::rgb(1.0, 0.96, 0.9), 3.0)
                    .with_shadows(30.0),
            );
            s.set_sky(Sky {
                zenith: Color::rgb8(82, 96, 140),
                horizon: Color::rgb8(170, 185, 205),
                ground: Color::rgb8(40, 33, 28),
                ..Sky::default()
            });
            let scale = |m: Vec3, half: Vec3| {
                Mat4::from_scale_rotation_translation(half * 2.0, Quat::IDENTITY, m)
            };
            let stone = Material::color(Color::rgb8(120, 124, 132), 0.0, 0.95);
            s.draw(&self.cube, &stone, scale(scene::FLOOR.0, scene::FLOOR.1));
            let wood = Material::color(Color::rgb8(90, 80, 70), 0.0, 0.6);
            s.draw(&self.cube, &wood, scale(scene::POLE.0, scene::POLE.1));
            let steel = Material::color(Color::rgb8(200, 200, 210), 0.6, 0.3);
            let (at, r) = scene::BALL;
            s.draw(
                &self.ball,
                &steel,
                Mat4::from_scale_rotation_translation(Vec3::splat(r), Quat::IDENTITY, at),
            );
            if let Some(t) = self
                .sim
                .world3
                .interpolated_transform(self.sim.crate_body, alpha)
            {
                let crate_mat = Material::color(Color::rgb8(190, 140, 70), 0.0, 0.8);
                let size = Vec3::splat(scene::CRATE_HALF * 2.0);
                s.draw(
                    &self.cube,
                    &crate_mat,
                    t.to_matrix() * Mat4::from_scale(size),
                );
            }
            // The particles are already in world space, so both soft
            // meshes are drawn with no transform, and the flag shows both
            // faces.
            let flag = Material {
                double_sided: true,
                ..Material::color(Color::WHITE, 0.0, 0.85)
            };
            s.draw(&self.flag, &flag, Transform::IDENTITY);
            let jelly = Material::color(Color::rgb8(80, 210, 140), 0.0, 0.25);
            s.draw(&self.jelly, &jelly, Transform::IDENTITY);
        });
        tank::draw(&mut ctx.gfx, &self.sim.fluid);
        self.hud(ctx);
        Ok(())
    }
}

impl Softbody {
    fn hud(&mut self, ctx: &mut Context) {
        let strain = self.sim.cloth.max_strain();
        let volume = self.sim.jelly.volume() / self.sim.jelly.rest_volume();
        let particles = self.sim.fluid.count();
        let mut gust = self.sim.gust;
        let (mut kick, mut again) = (false, false);
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.window("Soft bodies", &mut self.window, |f| {
                f.label(&format!(
                    "{}x{} flag, strain {:.1}%",
                    scene::FLAG_COLS,
                    scene::FLAG_ROWS,
                    strain * 100.0
                ));
                f.label(&format!(
                    "jelly volume {:.0}%, {particles} fluid particles",
                    volume * 100.0
                ));
                f.slider("Gust", &mut gust, 0.0..=20.0);
                kick = f.button("Kick the jelly (Space)").clicked;
                again = f.button("Start over (R)").clicked;
            });
        });
        self.sim.gust = gust;
        if kick {
            self.sim.kick();
        }
        if again {
            let _ = self.sim.reset();
        }
    }
}

fn main() -> Result<(), yowie::Error> {
    let mut config = Config::new("Yowie soft bodies").with_size(1100, 700);
    config.clear_color = Color::rgb8(20, 24, 34);
    yowie::run::<Softbody>(config)
}
