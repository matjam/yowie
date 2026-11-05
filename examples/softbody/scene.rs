//! The soft-body simulation: a flag on a pole, a jelly cube beside a rigid
//! crate, and a tank of fluid around a post.

use yowie::gfx::scene::MeshData;
use yowie::phys::soft::{
    Cloth, ClothDesc, Fluid2D, FluidDesc2D, SoftBody, SoftBodyDesc, SoftSettings, SurfaceMesh, Wind,
};
use yowie::phys::{BodyDesc3D, BodyId, ColliderDesc3D, Shape2D, Shape3D, World2D, World3D};
use yowie::{Color, GameResult, Rect, Transform, Transform2D, Vec2, Vec3, vec2};

/// The flag, in particles across and down, and how far apart they sit.
pub const FLAG_COLS: usize = 26;
pub const FLAG_ROWS: usize = 16;
const FLAG_SPACING: f32 = 0.14;
const FLAG_ORIGIN: Vec3 = Vec3::new(-2.1, 3.6, 0.0);

/// Static scenery: the floor, the pole and the ball the flag blows over.
pub const FLOOR: (Vec3, Vec3) = (Vec3::new(0.0, -0.5, 0.0), Vec3::new(12.0, 0.5, 12.0));
pub const POLE: (Vec3, Vec3) = (Vec3::new(-2.2, 2.0, 0.0), Vec3::new(0.08, 2.0, 0.08));
pub const BALL: (Vec3, f32) = (Vec3::new(0.9, 0.75, 0.9), 0.75);
/// The crate's half extent.
pub const CRATE_HALF: f32 = 0.7;

/// The tank, in view pixels, and the post inside it.
pub const TANK: Rect = Rect::new(24.0, 24.0, 280.0, 300.0);
pub const POST_RADIUS: f32 = 34.0;

/// Where the post stands: below the middle, so the liquid breaks over it.
pub fn post_centre() -> Vec2 {
    TANK.center() + vec2(0.0, 60.0)
}

/// Gravity for the fluid, in pixels per second squared, pointing down the
/// screen.
const FLUID_GRAVITY: Vec2 = vec2(0.0, 900.0);

/// What the example simulates.
pub struct SoftScene {
    pub world3: World3D,
    world2: World2D,
    pub cloth: Cloth,
    cloth_surface: SurfaceMesh,
    pub cloth_data: MeshData,
    pub jelly: SoftBody,
    jelly_surface: SurfaceMesh,
    pub jelly_data: MeshData,
    pub crate_body: BodyId,
    pub fluid: Fluid2D,
    settings: SoftSettings,
    /// Mean wind speed along +Z; the gust swings around it.
    pub gust: f32,
    time: f32,
}

impl SoftScene {
    pub fn new() -> GameResult<Self> {
        let mut world3 = World3D::new(Vec3::new(0.0, -9.8, 0.0));
        for (centre, half) in [FLOOR, POLE] {
            let desc = ColliderDesc3D::new(Shape3D::cuboid(half))
                .with_offset(Transform::from_translation(centre));
            world3.add_static_collider(&desc)?;
        }
        let ball = ColliderDesc3D::new(Shape3D::sphere(BALL.1))
            .with_offset(Transform::from_translation(BALL.0));
        world3.add_static_collider(&ball)?;
        let crate_body = world3.add_body(&BodyDesc3D::dynamic());
        // The post in the tank is a static 2D collider the fluid flows around.
        let mut world2 = World2D::new(Vec2::ZERO);
        let post = yowie::phys::ColliderDesc2D::new(Shape2D::circle(POST_RADIUS))
            .with_offset(Transform2D::from_position(post_centre()));
        world2.add_static_collider(&post)?;
        // One step puts the static colliders into the query structures the
        // soft solvers read.
        world2.step(1.0 / 60.0);
        world3.step(1.0 / 60.0);

        let (cloth, cloth_surface) = new_flag()?;
        let (jelly, jelly_surface) = new_jelly()?;
        let mut cloth_data = cloth_surface.to_mesh_data();
        paint_flag(&mut cloth_data);
        let jelly_data = jelly_surface.to_mesh_data();
        let mut scene = SoftScene {
            world3,
            world2,
            cloth,
            cloth_surface,
            cloth_data,
            jelly,
            jelly_surface,
            jelly_data,
            crate_body,
            fluid: new_fluid()?,
            settings: SoftSettings {
                substeps: 6,
                iterations: 4,
                ..SoftSettings::default()
            },
            gust: 6.0,
            time: 0.0,
        };
        scene.reset()?;
        Ok(scene)
    }

    /// Hangs the flag again, drops the jelly and the crate, refills the tank.
    pub fn reset(&mut self) -> GameResult {
        self.time = 0.0;
        (self.cloth, self.cloth_surface) = new_flag()?;
        (self.jelly, self.jelly_surface) = new_jelly()?;
        self.cloth_surface.write_to(&mut self.cloth_data);
        self.jelly_surface.write_to(&mut self.jelly_data);
        self.world3.remove_body(self.crate_body)?;
        self.crate_body = self
            .world3
            .add_body(&BodyDesc3D::dynamic().at(Vec3::new(3.6, 2.4, -1.4)));
        let crate_shape = ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(CRATE_HALF)))
            .with_friction(0.6)
            .with_restitution(0.1);
        self.world3.add_collider(self.crate_body, &crate_shape)?;
        self.fluid = new_fluid()?;
        Ok(())
    }

    /// Knocks the jelly up and to the left, away from the crate.
    pub fn kick(&mut self) {
        self.jelly.add_velocity(Vec3::new(-3.0, 5.5, 0.0));
    }

    /// Advances everything by one fixed step of `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.time += dt;
        // A gust that swings around, so the flag flaps rather than sitting
        // out stiffly in a steady wind.
        let t = self.time;
        let wind = Wind {
            velocity: Vec3::new(
                0.3 * self.gust * (t * 1.7).sin(),
                0.12 * self.gust * (t * 2.3).sin(),
                self.gust * (1.0 + 0.5 * (t * 1.1).sin()),
            ),
            drag: 1.5,
        };
        self.cloth.set_wind(wind);
        self.world3.step(dt);
        self.cloth.step(dt, &self.settings, &self.world3);
        self.jelly.step(dt, &self.settings, &self.world3);
        self.fluid.step(dt, FLUID_GRAVITY, &self.world2);
        self.cloth.update_mesh(&mut self.cloth_surface);
        self.jelly.update_mesh(&mut self.jelly_surface);
        self.cloth_surface.write_to(&mut self.cloth_data);
        self.jelly_surface.write_to(&mut self.jelly_data);
    }
}

fn new_flag() -> GameResult<(Cloth, SurfaceMesh)> {
    let desc = ClothDesc {
        cols: FLAG_COLS,
        rows: FLAG_ROWS,
        spacing: FLAG_SPACING,
        mass: 0.4,
        origin: FLAG_ORIGIN,
        pinned: (0..FLAG_ROWS).map(|row| row * FLAG_COLS).collect(),
        bend_compliance: 0.02,
        damping: 0.4,
        radius: 0.03,
        ..ClothDesc::default()
    };
    let cloth = Cloth::new(&desc)?;
    let surface = cloth.mesh();
    Ok((cloth, surface))
}

/// Colours the flag in three bands with a white one between.
fn paint_flag(data: &mut MeshData) {
    for (i, v) in data.vertices.iter_mut().enumerate() {
        let row = i / FLAG_COLS;
        let band = row * 3 / FLAG_ROWS;
        v.color = match band {
            1 => Color::rgb8(240, 240, 240),
            _ => Color::rgb8(210, 50, 60),
        };
    }
}

fn new_jelly() -> GameResult<(SoftBody, SurfaceMesh)> {
    let desc = SoftBodyDesc {
        mass: 3.0,
        edge_compliance: 1.0e-4,
        shape_matching: 0.04,
        damping: 0.6,
        radius: 0.04,
        ..SoftBodyDesc::default()
    };
    let body = SoftBody::lattice(
        Vec3::new(1.6, 2.4, -1.4),
        Vec3::splat(1.4),
        [4, 4, 4],
        &desc,
    )?;
    let surface = body.mesh();
    Ok((body, surface))
}

fn new_fluid() -> GameResult<Fluid2D> {
    let desc = FluidDesc2D {
        bounds: Some(TANK),
        spacing: 7.0,
        substeps: 2,
        iterations: 4,
        ..FluidDesc2D::default()
    };
    let mut fluid = Fluid2D::new(&desc)?;
    fluid.fill(Rect::new(
        TANK.x + 8.0,
        TANK.y + 8.0,
        TANK.w / 2.0 - 8.0,
        TANK.h - 16.0,
    ));
    Ok(fluid)
}
