//! The ECS driving a scene: the sun, planets and moons come from a scene
//! document and a moon prefab embedded from `examples/assets/solar/`, with
//! the hierarchy carrying every moon around its planet and every planet
//! around the sun. Systems run orbits and spin; a belt of 400 asteroids
//! spawned in code draws as one instanced call; clicking picks a body with
//! a screen ray; a render texture shows the same scene from above as a
//! minimap; the Schedule window lists each system's run time.
//!
//! Drag to orbit, right-drag pans, the wheel zooms, a click selects a body.
//! F4 opens the console's Entities panel for the same world. `--select
//! <name>` starts with a body selected and `--timings` shows run times in
//! headless runs, which otherwise show run counts so the picture is the
//! same every time. Escape quits.

#[path = "../common/mod.rs"]
mod common;
mod systems;
mod view;

use std::time::Instant;

use systems::{Asteroid, Body, Solar};
use yowie::ecs::{Entity, GlobalTransform};
use yowie::ecs_glue::DeltaTime;
use yowie::gfx::scene::{Camera3D, Material, Mesh, MeshData, PointLight, Sky};
use yowie::gfx::{Font, RenderTexture, RenderTextureOptions};
use yowie::input::{Key, MouseButton};
use yowie::{Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Rect, Vec3, vec2, vec3};

const MAP: f32 = 220.0;

struct App {
    sim: Solar,
    sphere: Mesh,
    cube: Mesh,
    minimap: RenderTexture,
    font: Font,
    orbit: OrbitCamera,
    selected: Entity,
    window: Rect,
    /// CPU milliseconds the last update's systems, and the last frame's
    /// minimap and scene recording, took.
    scopes: [f32; 3],
    timings: bool,
}

/// What the draw pass needs to know about one body.
struct Drawn {
    entity: Entity,
    matrix: Mat4,
    body: Body,
}

fn arg(name: &str) -> Option<String> {
    std::env::args().skip_while(|a| a != name).nth(1)
}

impl App {
    /// Every body that is not a belt rock, and the belt's matrices.
    fn gather(&self) -> (Vec<Drawn>, Vec<Mat4>) {
        let (mut bodies, mut rocks) = (Vec::new(), Vec::new());
        let w = &self.sim.world;
        for (entity, body, g, rock) in w
            .query::<(Entity, &Body, &GlobalTransform, Option<&Asteroid>)>()
            .iter()
        {
            if rock.is_some() {
                rocks.push(g.0 * Mat4::from_scale(Vec3::splat(body.radius)));
            } else {
                bodies.push(Drawn {
                    entity,
                    matrix: g.0,
                    body: body.clone(),
                });
            }
        }
        (bodies, rocks)
    }

    fn material(&self, d: &Drawn) -> Material {
        let [r, g, b] = d.body.color;
        let color = Color::rgb(r, g, b);
        if d.body.emissive > 0.0 {
            return Material::glowing(color, d.body.emissive);
        }
        let mut m = Material::color(color, 0.0, 0.8);
        if d.entity == self.selected {
            m.emissive = color;
            m.emissive_strength = 0.8;
        }
        m
    }
}

impl Game for App {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let sim = Solar::load()?;
        let mut orbit = OrbitCamera::new(Vec3::ZERO, 0.4, 0.75, 42.0);
        orbit.lens =
            Camera3D::perspective(Vec3::ZERO, Vec3::ZERO, 0.8).with_depth_range(0.1, 300.0);
        let mut app = App {
            selected: sim.sun,
            sim,
            sphere: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 20, 40))?,
            cube: Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?,
            minimap: RenderTexture::new(
                &ctx.gfx,
                MAP as u32,
                MAP as u32,
                RenderTextureOptions::default(),
            )?,
            font: ctx.gfx.default_font(),
            orbit,
            window: Rect::new(10.0, 10.0, 290.0, 330.0),
            scopes: [0.0; 3],
            timings: !ctx.headless() || std::env::args().any(|a| a == "--timings"),
        };
        if let Some(name) = arg("--select") {
            let found = app
                .sim
                .world
                .iter()
                .map(|e| e.entity())
                .find(|e| app.sim.name(*e) == name);
            app.selected = found.unwrap_or(app.sim.sun);
        }
        ctx.console
            .set_panels_open(std::env::args().any(|a| a == "--panels"));
        Ok(app)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let dt = ctx.time.delta();
        self.sim.world.insert_resource(DeltaTime(dt));
        let start = Instant::now();
        self.sim.schedule.run(&mut self.sim.world);
        self.scopes[0] = start.elapsed().as_secs_f32() * 1000.0;
        ctx.console
            .feed_world("solar", &self.sim.world, Some(&mut self.sim.schedule));
        if !ctx.ui.wants_mouse() && !ctx.input_captured() {
            self.orbit.update(&ctx.input, dt);
        }
        self.orbit.yaw += dt * 0.05;
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let (bodies, rocks) = self.gather();
        let rings = view::rings(&self.sim);
        let rock = Material::color(Color::rgb(0.55, 0.52, 0.48), 0.0, 0.9);

        // The minimap: the same bodies, drawn flat from straight above.
        let start = Instant::now();
        let top = Camera3D {
            up: Vec3::NEG_Z,
            ..Camera3D::orthographic(vec3(0.0, 40.0, 0.0), Vec3::ZERO, 22.0)
        };
        let flat = |c: Color| Material::glowing(c, 1.0);
        let (sphere, cube) = (&self.sphere, &self.cube);
        ctx.gfx
            .draw_to(&self.minimap, Some(Color::rgb8(5, 5, 12)), |g| {
                g.scene(&top, |s| {
                    s.set_sky(None);
                    for (a, b) in &rings.planets {
                        s.line(*a, *b, Color::rgb8(70, 75, 100));
                    }
                    for d in &bodies {
                        let [r, g, b] = d.body.color;
                        let on = d.entity == self.selected;
                        let (color, grow) = if on {
                            (Color::WHITE, 1.6)
                        } else {
                            (Color::rgb(r, g, b), 1.0)
                        };
                        let size = (d.body.radius * grow).max(0.35);
                        s.draw(
                            sphere,
                            &flat(color),
                            d.matrix * Mat4::from_scale(Vec3::splat(size)),
                        );
                    }
                    let tiny: Vec<Mat4> = rocks
                        .iter()
                        .map(|m| *m * Mat4::from_scale(Vec3::splat(2.0)))
                        .collect();
                    s.draw_instanced(cube, &flat(Color::rgb8(150, 145, 135)), &tiny);
                });
            });
        self.scopes[1] = start.elapsed().as_secs_f32() * 1000.0;

        // The main view. Picking needs the camera, so it happens inside
        // the scene, after the bodies and before the belt.
        let start = Instant::now();
        let cam = self.orbit.camera();
        let click = (ctx.input.mouse_pressed(MouseButton::Left)
            && !ctx.ui.wants_mouse()
            && !ctx.input_captured())
        .then(|| ctx.input.mouse());
        let picked = ctx.gfx.scene(&cam, |s| {
            let black = Color::rgb8(1, 1, 4);
            s.set_sky(Sky {
                vacuum: 1.0,
                stars: 0.8,
                sun_size: 0.0,
                zenith: black,
                horizon: black,
                ground: black,
                ..Sky::default()
            });
            s.set_ambient(Color::rgb8(30, 32, 48));
            s.add_point_light(PointLight::new(
                Vec3::ZERO,
                Color::rgb(1.0, 0.85, 0.6),
                110.0,
                120.0,
            ));
            for (a, b) in rings.planets.iter().chain(&rings.moons) {
                s.line(*a, *b, Color::rgb8(60, 66, 90));
            }
            let ids: Vec<_> = bodies
                .iter()
                .map(|d| {
                    let m = d.matrix * Mat4::from_scale(Vec3::splat(d.body.radius));
                    (s.draw(sphere, &self.material(d), m), d.entity)
                })
                .collect();
            let hit = click
                .and_then(|m| s.pick(&s.screen_ray(m)))
                .and_then(|p| ids.iter().find(|(id, _)| *id == p.draw).map(|(_, e)| *e));
            s.draw_instanced(cube, &rock, &rocks);
            hit
        });
        if let Some(e) = picked {
            self.selected = e;
        }
        self.scopes[2] = start.elapsed().as_secs_f32() * 1000.0;

        let view = ctx.gfx.view_size();
        let map = Rect::new(view.x - MAP - 12.0, 12.0, MAP, MAP);
        ctx.gfx
            .draw_texture_rect(self.minimap.texture(), map, Color::WHITE);
        ctx.gfx.stroke_rect(map, 1.0, common::EDGE);
        common::label(
            ctx,
            &self.font,
            "minimap (render texture)",
            vec2(map.x, map.y + MAP + 4.0),
            13.0,
            common::DIM,
        );
        view::labels(ctx, &self.font, &cam, &self.sim, &bodies, self.selected);
        view::schedule_window(ctx, &mut self.window, &self.sim, self.scopes, self.timings);
        view::footer(ctx, &self.font, &self.sim, self.selected);
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(5, 5, 12),
        console: true,
        ..Config::new("Yowie solar").with_size(1024, 640)
    };
    yowie::run::<App>(config)
}
