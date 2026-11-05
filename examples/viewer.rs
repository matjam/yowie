//! A lit 3D scene with an orbit camera: a checkered floor, a polished
//! metal sphere, a rough red one, a glowing cube, spinning cubes from
//! metal to plastic, two translucent panes and an animated glTF creature,
//! with a 2D panel drawn over the scene. `--model <file.gltf|file.glb>`
//! shows a glTF file instead and plays its first clip.
//!
//! Drag with the left mouse button to orbit, with the right to pan; the
//! wheel zooms. Space pauses the turntable; Escape quits.

#[path = "common/creature.rs"]
mod creature;

use std::f32::consts::PI;

use yowie::gfx::scene::{
    AlphaMode, Camera3D, DirectionalLight, Material, Mesh, MeshData, Model, ModelInstance,
    PointLight, Projection, Sky,
};
use yowie::gfx::{Font, Sprite, TextStyle, Texture, TextureOptions, Wrap};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, Vec3, vec2};

struct Viewer {
    orbit: OrbitCamera,
    cube: Mesh,
    sphere: Mesh,
    pane: Mesh,
    floor: Material,
    badge: Texture,
    font: Font,
    /// The creature, or the file named by `--model`.
    model: ModelInstance,
    loaded: bool,
    spin: bool,
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

fn checker(size: u32, cell: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let c = if (x / cell + y / cell).is_multiple_of(2) {
                [200, 200, 210, 255]
            } else {
                [90, 95, 110, 255]
            };
            px.extend_from_slice(&c);
        }
    }
    px
}

impl Game for Viewer {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let options = TextureOptions {
            wrap: Wrap::Repeat,
            mipmaps: true,
            ..TextureOptions::smooth()
        };
        let checker = Texture::from_pixels(&ctx.gfx, 64, 64, &checker(64, 8), options)?;
        let mut floor = Material {
            base_color_texture: Some(checker),
            roughness: 0.8,
            ..Material::default()
        };
        floor.uv_transform.scale = vec2(5.0, 5.0);
        let badge = Texture::from_pixels(&ctx.gfx, 1, 1, &[255, 200, 40, 255], options)?;
        let (model, loaded) = match arg("--model") {
            Some(path) => (Model::load(&ctx.gfx, path)?, true),
            None => (
                Model::from_slice(&ctx.gfx, &creature::creature([0.75, 0.3, 0.55]))?,
                false,
            ),
        };
        let mut inst = model.instance();
        if let Some(clip) = model.clips().first() {
            let name = clip.name.clone();
            inst.play(&name);
        }
        inst.update_pose();
        let mut orbit = OrbitCamera::new(Vec3::new(0.0, 0.6, 0.0), 0.6, 0.45, 9.0);
        if loaded {
            let b = model.bounds();
            orbit.target = b.center();
            orbit.distance = (b.half_extents().length() * 3.0).max(0.5);
        }
        orbit.lens.far = orbit.distance * 20.0;
        Ok(Viewer {
            orbit,
            cube: Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?,
            sphere: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 24, 48))?,
            pane: Mesh::new(&ctx.gfx, &MeshData::plane(1.0, 1))?,
            floor,
            badge,
            font: ctx.gfx.default_font(),
            model: inst,
            loaded,
            spin: true,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::Space) {
            self.spin = !self.spin;
        }
        let dt = ctx.time.delta();
        self.orbit.update(&ctx.input, dt);
        if self.spin {
            self.orbit.yaw += dt * 0.3;
        }
        self.model.advance(dt);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let t = ctx.time.elapsed() as f32;
        let cam = Camera3D {
            projection: Projection::Perspective {
                fov_y: 50f32.to_radians(),
            },
            ..self.orbit.camera()
        };
        let lamp = Vec3::new(4.0 * t.sin(), 1.5, 4.0 * t.cos());
        ctx.gfx.scene(&cam, |s| {
            s.set_sun(
                DirectionalLight::new(Vec3::new(-0.4, -1.0, -0.6), Color::rgb(1.0, 0.96, 0.9), 3.0)
                    .with_shadows(40.0),
            );
            // The sky lights the scene too: metals reflect it.
            s.set_sky(Sky::default());
            s.add_point_light(PointLight::new(lamp, Color::rgb(1.0, 0.45, 0.2), 6.0, 8.0));
            if self.loaded {
                s.draw_model(&self.model, Vec3::ZERO);
                return;
            }
            s.draw(
                &self.cube,
                &self.floor,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(10.0, 0.2, 10.0),
                    Quat::IDENTITY,
                    Vec3::new(0.0, -1.1, 0.0),
                ),
            );
            let gold = Material::color(Color::rgb8(230, 200, 120), 1.0, 0.15);
            s.draw(
                &self.sphere,
                &gold,
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(1.2),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.2, 0.0),
                ),
            );
            let red = Material::color(Color::rgb8(220, 60, 60), 0.0, 0.7);
            s.draw(
                &self.sphere,
                &red,
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(0.8),
                    Quat::IDENTITY,
                    Vec3::new(-3.0, -0.2, 2.0),
                ),
            );
            let glow = Material::glowing(Color::rgb8(120, 200, 255), 3.0);
            s.draw(
                &self.cube,
                &glow,
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(0.8),
                    Quat::from_rotation_y(t * 0.5),
                    Vec3::new(3.0, -0.6, 2.0),
                ),
            );
            for i in 0..6 {
                let a = i as f32 * PI / 3.0 + t * 0.5;
                let pos = Vec3::new(
                    3.5 * a.cos(),
                    0.3 + 0.4 * (t + i as f32).sin(),
                    3.5 * a.sin(),
                );
                let rot = Quat::from_rotation_y(a) * Quat::from_rotation_x(t);
                let shade = 90 + 25 * i as u8;
                let m = Material::color(
                    Color::rgb8(shade, 255 - shade, 200),
                    i as f32 / 5.0,
                    0.3 + 0.1 * i as f32,
                );
                s.draw(
                    &self.cube,
                    &m,
                    Mat4::from_scale_rotation_translation(Vec3::splat(0.6), rot, pos),
                );
            }
            // Two translucent panes, drawn back to front over the scene.
            for (i, c) in [
                Color::rgba(0.15, 0.6, 1.0, 0.45),
                Color::rgba(1.0, 0.4, 0.35, 0.45),
            ]
            .into_iter()
            .enumerate()
            {
                let pane = Material {
                    base_color: c,
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    roughness: 0.25,
                    ..Material::default()
                };
                let x = -1.2 + 2.4 * i as f32;
                let place = Mat4::from_scale_rotation_translation(
                    Vec3::new(2.4, 1.0, 1.8),
                    Quat::from_rotation_y(0.4 - 0.8 * i as f32) * Quat::from_rotation_x(PI / 2.0),
                    Vec3::new(x, 2.2, -1.5 + i as f32),
                );
                s.draw(&self.pane, &pane, place);
            }
            s.draw_model(&self.model, Vec3::new(1.8, -1.0, -2.8));
        });

        // The 2D overlay draws after the scene.
        let g = &mut ctx.gfx;
        g.fill_rect(
            Rect::new(10.0, 10.0, 330.0, 54.0),
            Color::rgba(0.0, 0.0, 0.0, 0.6),
        );
        g.draw_sprite(
            &self.badge,
            &Sprite::at(vec2(18.0, 18.0)).size(vec2(20.0, 20.0)),
        );
        let style = TextStyle::new(14.0).color(Color::rgb8(225, 228, 238));
        let stats = g.stats();
        g.draw_text(
            &self.font,
            "drag to orbit, right-drag pans, wheel zooms",
            vec2(46.0, 18.0),
            &style,
        );
        g.draw_text(
            &self.font,
            &format!(
                "{} draws in {} calls, {} culled",
                stats.draws_3d, stats.draw_calls_3d, stats.culled_3d
            ),
            vec2(46.0, 38.0),
            &style,
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(24, 26, 34),
        ..Config::new("Yowie viewer").with_size(960, 640)
    };
    yowie::run::<Viewer>(config)
}
