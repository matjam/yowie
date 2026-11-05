//! The renderer playground: five skinned glTF creatures playing their
//! animation clips, cascaded sun shadows, two orbiting lamps that cast
//! cube-map shadows over a field of 160 small clustered point lights, a
//! procedural atmospheric sky that thins to black and stars as the
//! altitude climbs to orbit, image-based lighting from a panorama, and
//! every post-processing setting on a slider or checkbox in two panels.
//!
//! - `--env <panorama>` lights the scene from an equirectangular OpenEXR,
//!   Radiance HDR, PNG or JPEG image; E toggles it.
//! - `--model <file.gltf|file.glb>` replaces the creatures with a model;
//!   C cycles its clips.
//! - `--altitude <units>` starts the camera's air that high; the altitude
//!   slider, PageUp and PageDown climb and descend.
//! - `--effects <list>` turns effects on from the start, for screenshots:
//!   a comma-separated list of `taa`, `msaa`, `dof`, `motion`, `rays`,
//!   `ssr`, `lens`, `grade`, `auto` and `occlusion` (the occlusion buffer).
//! - `--no-ui` hides the panels.
//!
//! S toggles shadows, F the light field, Tab the panels; dragging with
//! the left mouse button turns the camera, the wheel zooms; Escape quits.

#[path = "common/creature.rs"]
mod creature;
#[path = "common/post_panel.rs"]
mod post_panel;

use std::f32::consts::TAU;

use yowie::gfx::scene::{
    Atmosphere, Camera3D, DirectionalLight, Environment, EnvironmentOptions, Lut, Material, Mesh,
    MeshData, Model, ModelInstance, PointLight, PostSettings, Projection, Sky,
};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, Vec3};

/// Small point lights over the floor: far more than a fixed array would
/// hold, which the cluster grid sorts into the parts of the view each
/// one reaches.
const FIELD_LIGHTS: usize = 160;
const FIELD_COLS: usize = 16;
/// The depth of the air in world units.
const AIR: f32 = 3000.0;

struct Lighting {
    orbit: OrbitCamera,
    floor: Mesh,
    sphere: Mesh,
    creatures: Vec<ModelInstance>,
    clip: usize,
    env: Option<Environment>,
    use_env: bool,
    shadows: bool,
    field: bool,
    altitude: f32,
    post: PostSettings,
    warm: Lut,
    panels: bool,
    /// The game time of the last frame, for the moving sphere's previous
    /// placement.
    last_t: f32,
}

/// The value after `name` on the command line.
fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
    }
    None
}

/// The post settings the example starts with: bloom, ambient occlusion
/// and FXAA, plus the effects `--effects` names.
fn starting_post(effects: &str, warm: &Lut) -> PostSettings {
    let mut p = PostSettings {
        bloom: 0.25,
        ambient_occlusion: 0.6,
        fxaa: true,
        ..PostSettings::default()
    };
    for e in effects.split(',') {
        match e.trim() {
            "taa" => p.temporal_aa = true,
            "msaa" => p.samples = 4,
            "dof" => p.focus_distance = 9.0,
            "motion" => p.motion_blur = 1.0,
            "rays" => p.god_rays = 1.0,
            "ssr" => p.reflections = 1.0,
            "lens" => {
                p.vignette = 0.5;
                p.aberration = 2.0;
                p.distortion = 0.15;
                p.ghosts = 0.5;
                p.grain = 0.04;
            }
            "grade" => {
                p.temperature = 0.3;
                p.saturation = 1.2;
                p.contrast = 1.1;
                p.lut = Some(warm.clone());
                p.lut_strength = 0.5;
            }
            "auto" => p.auto_exposure = true,
            "occlusion" => p.show_occlusion = true,
            _ => {}
        }
    }
    p
}

/// A lookup table that warms the picture: red up, blue down.
fn warm_lut(ctx: &Context) -> GameResult<Lut> {
    let mut img = Lut::neutral_image(16);
    for p in img.pixels_mut() {
        p.0[0] = (f32::from(p.0[0]) * 1.08).min(255.0) as u8;
        p.0[2] = (f32::from(p.0[2]) * 0.85) as u8;
    }
    Ok(Lut::from_image(&ctx.gfx, &img)?)
}

/// Where the sphere on a fast orbit is at time `t`.
fn orbit_at(t: f32) -> Mat4 {
    let a = t * 1.3;
    Mat4::from_scale_rotation_translation(
        Vec3::splat(0.55),
        Quat::IDENTITY,
        Vec3::new(3.2 * a.cos(), 2.2, 3.2 * a.sin()),
    )
}

impl Lighting {
    fn play_clip(&mut self) {
        for (i, c) in self.creatures.iter_mut().enumerate() {
            let model = c.model().clone();
            let Some(clip) = model.clips().get(self.clip % model.clips().len().max(1)) else {
                continue;
            };
            c.play(&clip.name);
            // Each creature starts at its own point in the clip.
            c.advance(i as f32 * 0.37);
        }
    }

    fn draw_scene(&self, s: &mut yowie::gfx::scene::Scene, t: f32) {
        s.set_post(self.post.clone());
        let mut sun = DirectionalLight::new(Vec3::new(-0.5, -1.0, -0.4), Color::WHITE, 2.5);
        if self.shadows {
            sun = sun.with_shadows(30.0);
        }
        s.set_sun(sun);
        let atmosphere = Atmosphere::new(AIR).at_altitude(self.altitude);
        s.set_sky(Sky {
            stars: 1.0,
            ..Sky::atmospheric(atmosphere, Color::rgb8(80, 70, 60))
        });
        if self.use_env {
            s.set_environment(self.env.as_ref());
        }
        // Two lamps orbit the floor; each renders six shadow faces.
        for i in 0..2 {
            let a = t * 0.7 + i as f32 * 3.1;
            let color = if i == 0 {
                Color::rgb(1.0, 0.55, 0.25)
            } else {
                Color::rgb(0.3, 0.55, 1.0)
            };
            let mut lamp = PointLight::new(
                Vec3::new(4.0 * a.cos(), 2.2, 4.0 * a.sin()),
                color,
                5.0,
                9.0,
            );
            lamp.shadows = self.shadows;
            s.add_point_light(lamp);
        }
        if self.field {
            for i in 0..FIELD_LIGHTS {
                let x = (i % FIELD_COLS) as f32 * 0.85 - 6.4;
                let z = (i / FIELD_COLS) as f32 * 0.85 - 3.8;
                let pulse = 0.5 + 0.5 * (t * 2.0 + i as f32 * 0.7).sin();
                // The golden angle keeps neighbouring lights apart in hue.
                let hue = i as f32 * 2.4;
                let c = Color::rgb(
                    0.5 + 0.5 * hue.sin(),
                    0.5 + 0.5 * (hue + 2.1).sin(),
                    0.5 + 0.5 * (hue + 4.2).sin(),
                );
                s.add_point_light(PointLight::new(Vec3::new(x, 0.3, z), c, 4.0 * pulse, 1.0));
            }
        }
        let floor = Material::color(Color::rgb8(150, 150, 160), 0.0, 0.9);
        s.draw(
            &self.floor,
            &floor,
            Mat4::from_scale_rotation_translation(
                Vec3::new(14.0, 0.2, 14.0),
                Quat::IDENTITY,
                Vec3::new(0.0, -0.1, 0.0),
            ),
        );
        let gold = Material::color(Color::rgb8(230, 200, 120), 1.0, 0.2);
        s.draw(&self.sphere, &gold, Vec3::new(3.5, 1.0, -2.0));
        let glow = Material::glowing(Color::rgb8(90, 200, 255), 2.5);
        s.draw(
            &self.sphere,
            &glow,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.6),
                Quat::IDENTITY,
                Vec3::new(-3.5, 0.6, 2.0),
            ),
        );
        // The fast sphere says where it was last frame, so temporal
        // anti-aliasing follows it and motion blur streaks it its own way.
        let coral = Material::color(Color::rgb8(255, 130, 90), 0.0, 0.35);
        s.draw_moved(&self.sphere, &coral, orbit_at(t), orbit_at(self.last_t));
        let n = self.creatures.len();
        for (i, c) in self.creatures.iter().enumerate() {
            let place = if n == 1 {
                Vec3::ZERO
            } else {
                let a = i as f32 / n as f32 * TAU;
                Vec3::new(2.2 * a.cos(), 0.0, 2.2 * a.sin())
            };
            s.draw_model(c, place);
        }
    }

    fn draw_panels(&mut self, ctx: &mut Context) {
        let st = ctx.gfx.stats();
        let view = ctx.gfx.view_size();
        let Lighting {
            post,
            warm,
            altitude,
            shadows,
            field,
            use_env,
            env,
            ..
        } = self;
        let has_env = env.is_some();
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            let left = Rect::new(12.0, 12.0, 290.0, view.y - 24.0);
            post_panel::picture_panel(f, left, post, warm, |f| {
                f.label(&format!(
                    "{} lights ({} dropped), {} shadow draws",
                    st.lights, st.lights_dropped, st.shadow_instances
                ));
                f.label(&format!(
                    "{} meshes in {} draw calls, {} culled",
                    st.draws_3d, st.draw_calls_3d, st.culled_3d
                ));
                f.slider("Altitude", altitude, 0.0..=AIR * 1.2);
                f.checkbox("Shadows", shadows);
                f.checkbox("Light field (160 point lights)", field);
                if has_env {
                    f.checkbox("Environment (image-based lighting)", use_env);
                }
                f.label_dim("C next clip, Tab hides the panels");
            });
            let right = Rect::new(view.x - 302.0, 12.0, 290.0, view.y - 24.0);
            post_panel::lens_panel(f, right, post);
        });
    }
}

impl Game for Lighting {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let creatures = match arg("--model") {
            Some(path) => vec![Model::load(&ctx.gfx, path)?.instance()],
            None => {
                let colors = [
                    [0.75, 0.3, 0.55],
                    [0.3, 0.6, 0.75],
                    [0.8, 0.55, 0.2],
                    [0.4, 0.75, 0.35],
                    [0.6, 0.4, 0.85],
                ];
                let mut v = Vec::new();
                for c in colors {
                    v.push(Model::from_slice(&ctx.gfx, &creature::creature(c))?.instance());
                }
                v
            }
        };
        let env = match arg("--env") {
            Some(path) => {
                let bytes = std::fs::read(&path)?;
                let options = EnvironmentOptions {
                    intensity: 1.5,
                    ..EnvironmentOptions::default()
                };
                Some(Environment::from_bytes(&ctx.gfx, &bytes, options)?)
            }
            None => None,
        };
        let altitude = arg("--altitude")
            .and_then(|a| a.parse().ok())
            .unwrap_or(0.0f32);
        let mut orbit = OrbitCamera::new(Vec3::new(0.0, 1.2, 0.0), 0.7, 0.35, 11.0);
        orbit.lens.projection = Projection::Perspective {
            fov_y: 55f32.to_radians(),
        };
        orbit.lens.far = 400.0;
        let warm = warm_lut(ctx)?;
        let mut game = Lighting {
            orbit,
            floor: Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?,
            sphere: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 20, 40))?,
            creatures,
            clip: 0,
            use_env: env.is_some(),
            env,
            shadows: true,
            field: true,
            altitude,
            post: starting_post(&arg("--effects").unwrap_or_default(), &warm),
            warm,
            panels: !std::env::args().any(|a| a == "--no-ui"),
            last_t: 0.0,
        };
        game.play_clip();
        Ok(game)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let input = &ctx.input;
        let typing = ctx.ui.wants_keyboard();
        if !typing {
            if input.key_pressed(Key::S) {
                self.shadows = !self.shadows;
            }
            if input.key_pressed(Key::F) {
                self.field = !self.field;
            }
            if input.key_pressed(Key::Tab) {
                self.panels = !self.panels;
            }
            if input.key_pressed(Key::E) && self.env.is_some() {
                self.use_env = !self.use_env;
            }
            if input.key_pressed(Key::C) {
                self.clip += 1;
                self.play_clip();
            }
        }
        let dt = ctx.time.delta();
        if input.key_down(Key::PageUp) {
            self.altitude = (self.altitude.max(10.0) * (1.0 + 1.5 * dt)).min(AIR * 1.2);
        }
        if input.key_down(Key::PageDown) {
            self.altitude = (self.altitude / (1.0 + 1.5 * dt)).max(0.0);
            if self.altitude < 10.0 {
                self.altitude = 0.0;
            }
        }
        if !(self.panels && ctx.ui.wants_mouse()) {
            self.orbit.update(&ctx.input, dt);
        }
        self.orbit.yaw += dt * 0.15;
        for c in &mut self.creatures {
            c.advance(dt);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let t = ctx.time.elapsed() as f32;
        let cam: Camera3D = self.orbit.camera();
        ctx.gfx.scene(&cam, |s| self.draw_scene(s, t));
        self.last_t = t;
        if self.panels {
            self.draw_panels(ctx);
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config::new("Yowie lighting").with_size(1024, 720);
    yowie::run::<Lighting>(config)
}
