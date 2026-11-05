//! Five hundred cubes dropped into a concrete pen: wood, brushed metal,
//! plastic, gold, car paint, velvet, glass and glowing cubes, stacked by
//! `yowie-phys` with friction and restitution. A ray cast from the
//! pointer into the physics world lights up the cube under it.
//!
//! Drag with the left mouse button to orbit, the wheel zooms. R drops the
//! cubes again, D shows the physics debug lines, Escape quits.

mod materials;

use yowie::gfx::scene::{Camera3D, DirectionalLight, Material, Mesh, MeshData, Sky};
use yowie::gfx::{Font, TextStyle};
use yowie::input::Key;
use yowie::phys::{BodyDesc3D, BodyId, ColliderDesc3D, QueryFilter, Shape3D, World3D};
use yowie::rng::Rng;
use yowie::{
    Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, SceneDebugDraw,
    Transform, Vec3, vec2,
};

use materials::{KINDS, Library};

const COUNT: usize = 500;
/// Strong hues so each surface reads.
const PALETTE: [Color; 9] = [
    Color::rgb(0.86, 0.2, 0.2),
    Color::rgb(0.94, 0.55, 0.12),
    Color::rgb(0.94, 0.86, 0.24),
    Color::rgb(0.24, 0.75, 0.31),
    Color::rgb(0.16, 0.78, 0.78),
    Color::rgb(0.2, 0.43, 0.94),
    Color::rgb(0.59, 0.27, 0.9),
    Color::rgb(0.94, 0.39, 0.7),
    Color::rgb(0.94, 0.94, 0.94),
];
/// The pen's walls: centre and half extents.
const WALLS: [(Vec3, Vec3); 4] = [
    (Vec3::new(12.0, 1.5, 0.0), Vec3::new(0.5, 1.5, 12.0)),
    (Vec3::new(-12.0, 1.5, 0.0), Vec3::new(0.5, 1.5, 12.0)),
    (Vec3::new(0.0, 1.5, 12.0), Vec3::new(12.0, 1.5, 0.5)),
    (Vec3::new(0.0, 1.5, -12.0), Vec3::new(12.0, 1.5, 0.5)),
];

struct Cube {
    body: BodyId,
    material: Material,
}

struct Physics3d {
    world: World3D,
    cubes: Vec<Cube>,
    library: Library,
    mesh: Mesh,
    rng: Rng,
    orbit: OrbitCamera,
    hover: Option<BodyId>,
    debug: bool,
    font: Font,
}

impl Physics3d {
    /// Removes any cubes and drops a fresh set in a loose column.
    fn drop_cubes(&mut self) -> GameResult {
        for c in self.cubes.drain(..) {
            self.world.remove_body(c.body)?;
        }
        let r = &mut self.rng;
        for i in 0..COUNT {
            let x = (i % 8) as f32 * 1.3 - 4.5 + r.between(-0.2, 0.2);
            let z = ((i / 8) % 8) as f32 * 1.3 - 4.5 + r.between(-0.2, 0.2);
            let y = 3.0 + (i / 64) as f32 * 2.5 + r.between(0.0, 1.0);
            let axis = Vec3::new(r.float(), r.float(), r.float()).normalize_or(Vec3::Y);
            let rotation = Quat::from_axis_angle(axis, r.float() * 3.0);
            let body = self.world.add_body(
                &BodyDesc3D::dynamic()
                    .at(Vec3::new(x, y, z))
                    .rotated(rotation),
            );
            let collider = ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5)))
                .with_friction(0.6)
                .with_restitution(0.05);
            self.world.add_collider(body, &collider)?;
            let color = PALETTE[r.below(PALETTE.len() as u64) as usize];
            let kind = KINDS[r.below(KINDS.len() as u64) as usize];
            self.cubes.push(Cube {
                body,
                material: self.library.cube(kind, color),
            });
        }
        Ok(())
    }

    fn camera(&self) -> Camera3D {
        self.orbit.camera()
    }
}

impl Game for Physics3d {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let mut world = World3D::new(Vec3::new(0.0, -9.8, 0.0));
        // The ground and four low walls are static colliders.
        let ground = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(30.0, 0.5, 30.0)))
            .with_offset(Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)));
        world.add_static_collider(&ground)?;
        for (centre, half) in WALLS {
            let wall = ColliderDesc3D::new(Shape3D::cuboid(half))
                .with_offset(Transform::from_translation(centre));
            world.add_static_collider(&wall)?;
        }
        let mut orbit = OrbitCamera::new(Vec3::new(0.0, 3.0, 0.0), 0.7, 0.45, 38.0);
        orbit.min_distance = 8.0;
        orbit.max_distance = 120.0;
        orbit.lens.far = 500.0;
        let mut game = Physics3d {
            world,
            cubes: Vec::new(),
            library: Library::new(&ctx.gfx)?,
            mesh: Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?,
            rng: Rng::new(3),
            orbit,
            hover: None,
            debug: false,
            font: ctx.gfx.default_font(),
        };
        game.drop_cubes()?;
        Ok(game)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::R) {
            self.drop_cubes()?;
        }
        if ctx.input.key_pressed(Key::D) {
            self.debug = !self.debug;
        }
        let dt = ctx.time.delta();
        self.orbit.update(&ctx.input, dt);
        self.orbit.pitch = self.orbit.pitch.clamp(0.05, 1.5);
        self.world.step(dt);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let cam = self.camera();
        // The cube under the pointer, found by a ray cast into the world.
        let ray = cam.screen_ray(ctx.input.mouse(), ctx.gfx.view_size());
        self.hover = self
            .world
            .ray_cast(ray.origin, ray.direction, 200.0, &QueryFilter::default())
            .and_then(|hit| hit.body);
        let alpha = ctx.time.alpha();
        let mut ground = self.library.concrete.clone();
        ground.uv_transform.scale = vec2(20.0, 20.0);
        let mut wall_material = self.library.concrete.clone();
        wall_material.base_color = Color::rgb8(190, 190, 200);
        wall_material.uv_transform.scale = vec2(8.0, 1.0);
        ctx.gfx.scene(&cam, |s| {
            s.set_sun(
                DirectionalLight::new(Vec3::new(-0.5, -1.0, -0.3), Color::rgb(1.0, 0.96, 0.9), 3.2)
                    .with_shadows(60.0),
            );
            s.set_sky(Sky {
                zenith: Color::rgb8(80, 110, 170),
                horizon: Color::rgb8(170, 185, 205),
                ground: Color::rgb8(70, 62, 55),
                ..Sky::default()
            });
            s.draw(
                &self.mesh,
                &ground,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(60.0, 1.0, 60.0),
                    Quat::IDENTITY,
                    Vec3::new(0.0, -0.5, 0.0),
                ),
            );
            for (centre, half) in WALLS {
                s.draw(
                    &self.mesh,
                    &wall_material,
                    Mat4::from_scale_rotation_translation(half * 2.0, Quat::IDENTITY, centre),
                );
            }
            for c in &self.cubes {
                let Some(t) = self.world.interpolated_transform(c.body, alpha) else {
                    continue;
                };
                if self.hover == Some(c.body) {
                    let lit = Material {
                        emissive: Color::rgb(1.0, 0.85, 0.4),
                        emissive_strength: 1.5,
                        ..c.material.clone()
                    };
                    s.draw(&self.mesh, &lit, t);
                } else {
                    s.draw(&self.mesh, &c.material, t);
                }
            }
            if self.debug {
                self.world.debug_draw(&mut SceneDebugDraw::new(s));
            }
        });

        let g = &mut ctx.gfx;
        let st = g.stats();
        let h = g.view_size().y;
        g.fill_rect(
            Rect::new(12.0, h - 72.0, 420.0, 60.0),
            Color::rgba(0.03, 0.04, 0.06, 0.75),
        );
        let style = TextStyle::new(14.0).color(Color::rgb8(225, 228, 238));
        let sleeping = self
            .cubes
            .iter()
            .filter(|c| self.world.is_sleeping(c.body))
            .count();
        g.draw_text(
            &self.font,
            &format!(
                "{COUNT} cubes, {sleeping} asleep; {} draws in {} calls",
                st.draws_3d, st.draw_calls_3d
            ),
            vec2(22.0, h - 64.0),
            &style,
        );
        g.draw_text(
            &self.font,
            "drag orbits, wheel zooms; R drops again, D debug lines",
            vec2(22.0, h - 42.0),
            &style,
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config::new("Yowie physics: 500 cubes").with_size(1024, 680);
    yowie::run::<Physics3d>(config)
}
