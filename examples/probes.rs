//! Light a scene gets from its surroundings rather than from one
//! environment: a reflection probe baked inside a glowing room, so the
//! chrome ball mirrors its walls; and a grid of light probes, so the matte
//! balls take the colour of the wall they stand by. Each has a checkbox;
//! with both off the room has only its ambient colour and a dim sun. The
//! room itself is a static group, drawn from render bundles and culled on
//! the GPU.
//!
//! `--show-probes` starts with the grid's cells drawn as small spheres.
//! Dragging with the left mouse button turns the camera, the wheel zooms;
//! Escape quits.

use yowie::gfx::scene::{
    Aabb, Camera3D, DirectionalLight, LightProbeGrid, Material, Mesh, MeshData, Projection,
    ReflectionProbe, Scene, StaticGroup,
};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Mat4, OrbitCamera, Quat, Rect, UVec3, Vec3};

/// The room's half-width and height in world units.
const ROOM: f32 = 9.0;
const TALL: f32 = 5.0;

struct Probes {
    orbit: OrbitCamera,
    sphere: Mesh,
    small_sphere: Mesh,
    room: StaticGroup,
    probe: ReflectionProbe,
    grid: LightProbeGrid,
    baked: String,
    use_probe: bool,
    use_grid: bool,
    show_cells: bool,
    spinning: bool,
}

/// A box of `size` world units centred on `at`.
fn slab(at: Vec3, size: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, at)
}

/// A rough surface of `color` that also glows `color` at `strength`.
fn glowing_wall(color: Color, strength: f32) -> Material {
    Material {
        emissive: color,
        emissive_strength: strength,
        ..Material::color(color, 0.0, 0.8)
    }
}

/// The walls, floor, ceiling and lamps: geometry that never moves, so
/// one static group holds all of it.
fn build_room(cube: &Mesh) -> StaticGroup {
    let mut room = StaticGroup::new();
    // A polished floor and a closed ceiling with a dim glowing panel.
    let floor = Material::color(Color::rgb8(40, 42, 48), 0.1, 0.06);
    room.add(
        cube,
        &floor,
        slab(
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(2.0 * ROOM, 0.2, 2.0 * ROOM),
        ),
    );
    let ceiling = Material::color(Color::rgb8(70, 70, 76), 0.0, 0.9);
    room.add(
        cube,
        &ceiling,
        slab(
            Vec3::new(0.0, TALL + 0.1, 0.0),
            Vec3::new(2.0 * ROOM, 0.2, 2.0 * ROOM),
        ),
    );
    room.add(
        cube,
        &glowing_wall(Color::rgb8(220, 220, 230), 0.6),
        slab(Vec3::new(0.0, TALL - 0.05, 0.0), Vec3::new(ROOM, 0.1, ROOM)),
    );
    // One wall glows red and one blue; that glow is most of the light in
    // the room, so only a bake carries it onto the balls.
    let side = Vec3::new(0.2, TALL, 2.0 * ROOM);
    room.add(
        cube,
        &glowing_wall(Color::rgb8(230, 40, 40), 2.5),
        slab(Vec3::new(-ROOM, TALL / 2.0, 0.0), side),
    );
    room.add(
        cube,
        &glowing_wall(Color::rgb8(40, 90, 230), 2.5),
        slab(Vec3::new(ROOM, TALL / 2.0, 0.0), side),
    );
    let grey = Material::color(Color::rgb8(120, 120, 125), 0.0, 0.8);
    let end = Vec3::new(2.0 * ROOM, TALL, 0.2);
    for z in [-ROOM, ROOM] {
        room.add(cube, &grey, slab(Vec3::new(0.0, TALL / 2.0, z), end));
    }
    // A bright bar on the far wall, which the floor and the ball reflect.
    room.add(
        cube,
        &glowing_wall(Color::rgb8(255, 210, 120), 4.0),
        slab(Vec3::new(0.0, 2.6, -ROOM + 1.0), Vec3::new(5.0, 0.24, 0.24)),
    );
    room
}

impl Probes {
    /// The room, the light in it and the balls. The bakes and every frame
    /// draw the same scene, so what the probes hold is what the camera
    /// sees.
    fn draw_scene(&self, s: &mut Scene) {
        s.set_sun(DirectionalLight::new(
            Vec3::new(-0.3, -1.0, -0.2),
            Color::rgb(0.15, 0.15, 0.17),
            1.0,
        ));
        s.set_ambient(Color::rgb(0.12, 0.13, 0.15));
        s.draw_static(&self.room);
        // A chrome ball in the middle and two matte balls by the coloured
        // walls. They are left out of the captures: the chrome ball would
        // mirror itself, a grid cell sits inside it, and a reflection
        // projected onto the walls would show the balls' outlines there.
        let chrome = Material::color(Color::WHITE, 1.0, 0.05);
        let ball = s.draw(
            &self.sphere,
            &chrome,
            slab(Vec3::new(0.0, 1.3, 0.0), Vec3::splat(1.1)),
        );
        s.hide_in_probes(ball);
        let matte = Material::color(Color::rgb8(230, 230, 230), 0.0, 0.9);
        for x in [-ROOM + 4.0, ROOM - 4.0] {
            let ball = s.draw(
                &self.sphere,
                &matte,
                slab(Vec3::new(x, 0.9, -4.0), Vec3::splat(0.9)),
            );
            s.hide_in_probes(ball);
        }
    }
}

impl Game for Probes {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let cube = Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?;
        let mut orbit = OrbitCamera::new(Vec3::new(0.0, 1.2, 0.0), 0.0, 0.26, 8.0);
        orbit.lens.projection = Projection::Perspective {
            fov_y: 60f32.to_radians(),
        };
        orbit.max_distance = 8.5;
        // One probe for the whole room, captured at eye level. Box
        // projection reflects each wall where the wall is.
        let bounds = Aabb::new(Vec3::new(-ROOM, 0.0, -ROOM), Vec3::new(ROOM, TALL, ROOM));
        let probe = ReflectionProbe::new(Vec3::new(0.0, 1.8, 0.0), bounds);
        // Three by two by three cells is enough for walls this far apart.
        let mut grid = LightProbeGrid::new(
            Vec3::new(-ROOM + 3.0, 0.9, -ROOM + 3.0),
            Vec3::new(ROOM - 3.0, 2.0, ROOM - 3.0),
            UVec3::new(3, 2, 3),
        );
        grid.resolution = 24;
        let mut game = Probes {
            orbit,
            sphere: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 24, 48))?,
            small_sphere: Mesh::new(&ctx.gfx, &MeshData::sphere(1.0, 12, 24))?,
            room: build_room(&cube),
            probe,
            grid: grid.clone(),
            baked: String::new(),
            use_probe: true,
            use_grid: true,
            show_cells: std::env::args().any(|a| a == "--show-probes"),
            spinning: true,
        };
        // The grid first, so the probe's capture shows the walls lit by it.
        ctx.gfx
            .bake_light_probes(&mut grid, |s| game.draw_scene(s))?;
        game.grid = grid;
        ctx.gfx.bake_reflection_probe(&game.probe, |s| {
            s.set_light_probes(Some(&game.grid));
            game.draw_scene(s);
        });
        game.baked = format!(
            "Baked one {}-texel probe and {} grid cells.",
            game.probe.resolution,
            game.grid.cell_count()
        );
        log::info!("probes: {}", game.baked);
        Ok(game)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let dt = ctx.time.delta();
        if !ctx.ui.wants_mouse() {
            self.orbit.update(&ctx.input, dt);
        }
        if self.spinning {
            self.orbit.yaw += dt * 0.25;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let cam: Camera3D = self.orbit.camera();
        ctx.gfx.scene(&cam, |s| {
            if self.use_probe {
                s.add_reflection_probe(&self.probe);
            }
            if self.use_grid {
                s.set_light_probes(Some(&self.grid));
            }
            if self.show_cells {
                s.draw_light_probes(&self.grid, &self.small_sphere, 0.15);
            }
            self.draw_scene(s);
        });

        let st = ctx.gfx.stats();
        let statics = format!(
            "{} bundle draws, {} hidden, {} outside",
            st.bundle_draws, st.occluded_3d, st.culled_3d
        );
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.panel(
                Some("Global illumination"),
                Rect::new(12.0, 360.0, 330.0, 268.0),
                |f| {
                    f.checkbox("Reflection probe (chrome ball)", &mut self.use_probe);
                    f.checkbox("Light probe grid (matte balls)", &mut self.use_grid);
                    f.checkbox("Show light probes", &mut self.show_cells);
                    f.checkbox("Turn the camera", &mut self.spinning);
                    f.label(&self.baked);
                    f.label(&statics);
                    f.label("Drag to look around, Escape quits.");
                },
            );
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config::new("Yowie probes").with_size(1024, 640);
    yowie::run::<Probes>(config)
}
