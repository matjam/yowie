//! Every material feature on a row of spheres: metal, clearcoat, sheen,
//! subsurface, vertex colours, unlit, refracting glass with absorption,
//! iridescence, anisotropy, a specular tint and fur. Around them:
//! alpha-cutout leaves casting cut-out shadows, a floor scrolling its
//! texture, a projected decal, an outlined sphere, an x-ray tint through a
//! wall and a stencil mask, under image-based lighting from the sky.
//!
//! - `--env <panorama>` lights the scene from an equirectangular OpenEXR,
//!   Radiance HDR, PNG or JPEG image instead of the procedural sky.
//!
//! Dragging with the left mouse button turns the camera and the wheel
//! zooms; the panel's check boxes switch the x-ray tint and the stencil
//! mask. Escape quits.

use std::f32::consts::FRAC_PI_2;

use yowie::gfx::scene::{
    AlphaMode, Anisotropy, Camera3D, Clearcoat, Decal, DirectionalLight, Environment,
    EnvironmentOptions, Fur, Iridescence, Material, Mesh, MeshData, Outline, Projection, Scene,
    Sheen, Sky, Specular, Stencil, Transmission,
};
use yowie::gfx::{Font, TextStyle, Texture, TextureOptions, Wrap};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, Vec2, Vec3};

struct Materials {
    orbit: OrbitCamera,
    font: Font,
    sphere: Mesh,
    colored: Mesh,
    cube: Mesh,
    quad: Mesh,
    stripes: Texture,
    leaf: Texture,
    splat: Texture,
    thick: Texture,
    strands: Texture,
    env: Option<Environment>,
    xray: bool,
    mask: bool,
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

/// RGBA8 pixels of a `size` square from a function of the texel.
fn image(size: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..size * size)
        .flat_map(|i| f(i % size, i / size))
        .collect()
}

/// Grey stripes eight texels wide.
fn stripes(x: u32, _y: u32) -> [u8; 4] {
    if (x / 8).is_multiple_of(2) {
        [150, 150, 160, 255]
    } else {
        [200, 200, 205, 255]
    }
}

/// A green leaf with a stem on a transparent background, 128 texels.
fn leaf(x: u32, y: u32) -> [u8; 4] {
    let u = x as f32 / 64.0 - 1.0;
    let v = y as f32 / 64.0 - 1.0;
    let blade = u * u / 0.36 + v * v <= 1.0 && u.abs() > 0.03;
    let stem = u.abs() <= 0.03;
    if blade || stem {
        [30, (110.0 + 80.0 * u.abs()) as u8, 40, 255]
    } else {
        [0; 4]
    }
}

/// A white blob with a wavy soft edge, 128 texels.
fn splat(x: u32, y: u32) -> [u8; 4] {
    let u = x as f32 / 64.0 - 1.0;
    let v = y as f32 / 64.0 - 1.0;
    let r = u.hypot(v) + 0.15 * (6.0 * v.atan2(u)).sin();
    let a = ((0.9 - r) * 4.0).clamp(0.0, 1.0);
    [255, 255, 255, (a * 255.0) as u8]
}

/// Thin (dark) bands every eight texels, through which light shows.
fn thickness(_x: u32, y: u32) -> [u8; 4] {
    let v = if (y / 8).is_multiple_of(2) { 40 } else { 255 };
    [v, v, v, 255]
}

/// The fur mask: each texel the height its strand reaches, a hash of
/// the cell it falls in, 40 cells across 128 texels.
fn strands(x: u32, y: u32) -> [u8; 4] {
    let (cx, cy) = (x * 40 / 128, y * 40 / 128);
    let mut h = cx
        .wrapping_mul(374_761_393)
        .wrapping_add(cy.wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    let v = (h >> 24) as u8;
    [v, v, v, 255]
}

/// A sphere coloured per vertex around its equator and up its axis.
fn colored_sphere() -> MeshData {
    let mut data = MeshData::sphere(1.0, 32, 64);
    for v in &mut data.vertices {
        let a = v.position.z.atan2(v.position.x);
        v.color = Color::rgb(
            0.5 + 0.5 * a.cos(),
            0.5 + 0.5 * a.sin(),
            0.5 + 0.5 * v.position.y,
        );
    }
    data
}

fn trs(scale: Vec3, rotation: Quat, at: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(scale, rotation, at)
}

/// The square `quad` mesh stood up to face +Z.
fn upright() -> Quat {
    Quat::from_rotation_x(FRAC_PI_2)
}

impl Materials {
    /// The row of spheres, each showing one feature, with its name.
    fn row(&self) -> Vec<(&'static str, Material, &Mesh)> {
        let c = Color::rgb8;
        let plain = |base, metallic, roughness| Material::color(base, metallic, roughness);
        let mut fur = plain(c(190, 140, 70), 0.0, 0.9);
        fur.fur = Some(Fur {
            texture: Some(self.strands.clone()),
            ..Fur::new(16, 0.22)
        });
        fur.uv_transform.scale = Vec2::new(8.0, 4.0);
        vec![
            ("metal", plain(c(240, 200, 120), 1.0, 0.15), &self.sphere),
            (
                "clearcoat",
                Material {
                    clearcoat: Some(Clearcoat::new(1.0, 0.05)),
                    ..plain(c(150, 20, 30), 0.0, 0.6)
                },
                &self.sphere,
            ),
            (
                "sheen",
                Material {
                    sheen: Some(Sheen::new(c(200, 180, 255), 0.4)),
                    ..plain(c(40, 30, 90), 0.0, 0.9)
                },
                &self.sphere,
            ),
            (
                "subsurface",
                Material {
                    subsurface: 1.0,
                    thickness_texture: Some(self.thick.clone()),
                    ..plain(c(240, 180, 150), 0.0, 0.7)
                },
                &self.sphere,
            ),
            (
                "vertex colours",
                Material {
                    vertex_colors: true,
                    ..plain(Color::WHITE, 0.0, 0.5)
                },
                &self.colored,
            ),
            (
                "unlit",
                Material {
                    unlit: true,
                    ..plain(c(255, 140, 40), 0.0, 1.0)
                },
                &self.sphere,
            ),
            (
                "glass",
                Material {
                    ior: 1.5,
                    transmission: Some(Transmission {
                        attenuation_color: c(120, 200, 255),
                        attenuation_distance: 1.0,
                        ..Transmission::glass(0.8)
                    }),
                    ..plain(Color::WHITE, 0.0, 0.05)
                },
                &self.sphere,
            ),
            (
                "iridescence",
                Material {
                    iridescence: Some(Iridescence::new(480.0)),
                    ..plain(c(200, 200, 210), 1.0, 0.2)
                },
                &self.sphere,
            ),
            (
                "anisotropy",
                Material {
                    anisotropy: Some(Anisotropy::new(0.9)),
                    ..plain(c(200, 200, 210), 1.0, 0.35)
                },
                &self.sphere,
            ),
            (
                "specular tint",
                Material {
                    specular: Some(Specular::tint(c(255, 170, 90))),
                    ..plain(c(20, 20, 22), 0.0, 0.2)
                },
                &self.sphere,
            ),
            ("fur", fur, &self.sphere),
        ]
    }

    /// Where row sphere `i` stands.
    fn row_place(i: usize) -> Vec3 {
        Vec3::new(i as f32 * 1.8 - 9.0, 0.4, -2.0)
    }

    fn draw_scene(&self, s: &mut Scene, t: f32) {
        s.set_sun(
            DirectionalLight::new(Vec3::new(-0.4, -1.0, -0.5), Color::WHITE, 2.2)
                .with_shadows(30.0),
        );
        s.set_sky(Sky {
            zenith: Color::rgb8(80, 130, 220),
            horizon: Color::rgb8(210, 220, 235),
            ground: Color::rgb8(90, 80, 70),
            ..Sky::default()
        });
        s.set_environment(self.env.as_ref());
        // The floor scrolls its stripes and carries a turning decal.
        let mut floor = Material {
            base_color_texture: Some(self.stripes.clone()),
            ..Material::color(Color::WHITE, 0.0, 0.8)
        };
        floor.uv_transform.offset = Vec2::new(t * 0.05, 0.0);
        floor.uv_transform.scale = Vec2::new(6.0, 6.0);
        s.draw(
            &self.cube,
            &floor,
            trs(
                Vec3::new(22.0, 0.2, 14.0),
                Quat::IDENTITY,
                Vec3::new(0.0, -0.5, 0.0),
            ),
        );
        let decal = Decal {
            tint: Color::rgb8(120, 20, 20),
            ..Decal::new(self.splat.clone())
        };
        s.draw_decal(
            &decal,
            trs(
                Vec3::new(2.0, 1.0, 2.0),
                Quat::from_rotation_y(t * 0.3),
                Vec3::new(-2.5, -0.3, 2.5),
            ),
        );
        for (i, (_, m, mesh)) in self.row().iter().enumerate() {
            s.draw(
                mesh,
                m,
                trs(Vec3::splat(0.8), Quat::IDENTITY, Self::row_place(i)),
            );
        }
        // Cut-out leaves: double-sided, casting cut-out shadows.
        let leaf = Material {
            base_color_texture: Some(self.leaf.clone()),
            alpha_mode: AlphaMode::Mask(0.5),
            double_sided: true,
            ..Material::color(Color::WHITE, 0.0, 0.8)
        };
        for i in 0..3 {
            let turn = Quat::from_rotation_y(t * 0.5 + i as f32) * upright();
            let at = Vec3::new(3.0 + i as f32 * 0.9, 0.9, 1.5);
            s.draw(&self.quad, &leaf, trs(Vec3::new(0.8, 1.0, 1.2), turn, at));
        }
        let outlined = Material {
            outline: Some(Outline::new(3.0, Color::WHITE)),
            ..Material::color(Color::rgb8(90, 200, 120), 0.0, 0.5)
        };
        s.draw(
            &self.sphere,
            &outlined,
            trs(Vec3::splat(0.7), Quat::IDENTITY, Vec3::new(-4.0, 0.3, 1.5)),
        );
        // A sphere behind a wall, tinted through it.
        let wall = Material::color(Color::rgb8(120, 120, 130), 0.0, 0.9);
        s.draw(
            &self.cube,
            &wall,
            trs(
                Vec3::new(2.4, 1.6, 0.15),
                Quat::IDENTITY,
                Vec3::new(-1.0, 0.5, 2.6),
            ),
        );
        let mut hidden = Material::color(Color::rgb8(255, 80, 80), 0.0, 0.4);
        if self.xray {
            hidden.xray = Some(Color::rgba8(255, 60, 60, 160));
        }
        s.draw(
            &self.sphere,
            &hidden,
            trs(Vec3::splat(0.5), Quat::IDENTITY, Vec3::new(-1.0, 0.4, 1.2)),
        );
        // A stencil mask: the pane marks where it draws and the orb in
        // front of it draws only there, which is how a portal or a cutaway
        // is built. Materials that write the stencil draw first, so the
        // two can be queued either way round.
        let mut pane = Material::color(Color::rgb8(25, 35, 55), 0.0, 0.3);
        let mut orb = Material {
            emissive: Color::rgb8(255, 120, 40),
            emissive_strength: 0.8,
            ..Material::color(Color::rgb8(255, 120, 40), 0.0, 0.3)
        };
        if self.mask {
            pane.stencil = Stencil::mark(1);
            orb.stencil = Stencil::inside(1);
        }
        s.draw(
            &self.sphere,
            &orb,
            trs(Vec3::splat(1.3), Quat::IDENTITY, Vec3::new(-4.6, 1.2, 5.2)),
        );
        s.draw(
            &self.quad,
            &pane,
            trs(
                Vec3::new(1.8, 1.0, 1.8),
                upright(),
                Vec3::new(-4.6, 1.2, 4.2),
            ),
        );
    }

    /// Names each row sphere under it.
    fn labels(&self, ctx: &mut Context, cam: &Camera3D) {
        let g = &mut ctx.gfx;
        let view = g.view_size();
        let style = TextStyle::new(13.0).color(Color::rgb8(235, 238, 245));
        for (i, (name, _, _)) in self.row().iter().enumerate() {
            let below = Self::row_place(i) - Vec3::new(0.0, 0.95, -0.3);
            let Some(at) = cam.world_to_screen(below, view) else {
                continue;
            };
            let size = g.measure_text(&self.font, name, &style);
            let at = at - Vec2::new(size.x * 0.5, 0.0);
            g.fill_rect(
                Rect::new(at.x - 4.0, at.y - 2.0, size.x + 8.0, size.y + 4.0),
                Color::rgba(0.02, 0.03, 0.05, 0.65),
            );
            g.draw_text(&self.font, name, at, &style);
        }
    }
}

impl Game for Materials {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let g = &ctx.gfx;
        let smooth = TextureOptions::smooth();
        let tiled = TextureOptions {
            wrap: Wrap::Repeat,
            ..smooth
        };
        let data = TextureOptions {
            data: true,
            ..tiled
        };
        let env = match arg("--env") {
            Some(path) => {
                let bytes = std::fs::read(&path)?;
                let lower = path.to_lowercase();
                let hdr = lower.ends_with(".exr") || lower.ends_with(".hdr");
                let options = EnvironmentOptions {
                    // An 8-bit panorama needs a boost to make up some of
                    // the range it cannot carry.
                    intensity: if hdr { 1.0 } else { 1.5 },
                    ..EnvironmentOptions::default()
                };
                Some(Environment::from_bytes(g, &bytes, options)?)
            }
            None => None,
        };
        let mut orbit = OrbitCamera::new(Vec3::new(-0.5, 0.4, 0.8), 0.25, 0.42, 15.0);
        orbit.lens.projection = Projection::Perspective {
            fov_y: 50f32.to_radians(),
        };
        orbit.lens.far = 200.0;
        Ok(Materials {
            orbit,
            font: g.default_font(),
            sphere: Mesh::new(g, &MeshData::sphere(1.0, 32, 64))?,
            colored: Mesh::new(g, &colored_sphere())?,
            cube: Mesh::new(g, &MeshData::cube(1.0))?,
            quad: Mesh::new(g, &MeshData::plane(1.0, 1))?,
            stripes: Texture::from_pixels(g, 64, 64, &image(64, stripes), tiled)?,
            leaf: Texture::from_pixels(g, 128, 128, &image(128, leaf), smooth)?,
            splat: Texture::from_pixels(g, 128, 128, &image(128, splat), smooth)?,
            thick: Texture::from_pixels(g, 64, 64, &image(64, thickness), data)?,
            strands: Texture::from_pixels(g, 128, 128, &image(128, strands), data)?,
            env,
            xray: true,
            mask: true,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if !ctx.ui.wants_mouse() {
            self.orbit.update(&ctx.input, ctx.time.delta());
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let t = ctx.time.elapsed() as f32;
        let cam = self.orbit.camera();
        ctx.gfx.scene(&cam, |s| self.draw_scene(s, t));
        self.labels(ctx, &cam);
        let (mut xray, mut mask) = (self.xray, self.mask);
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.panel(
                Some("Materials"),
                Rect::new(12.0, 12.0, 340.0, 190.0),
                |f| {
                    f.checkbox("X-ray the sphere behind the wall", &mut xray);
                    f.checkbox("Stencil mask on the pane", &mut mask);
                    f.paragraph(
                        "The leaves are alpha cut-outs, the green sphere is outlined, \
                     and the floor scrolls its texture and carries a decal.",
                    );
                },
            );
        });
        self.xray = xray;
        self.mask = mask;
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(24, 26, 34),
        ..Config::new("Yowie materials").with_size(1100, 680)
    };
    yowie::run::<Materials>(config)
}
