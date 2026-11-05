//! The solar example's components, systems and world setup. The scene
//! document and the moon prefab are embedded files in
//! `examples/assets/solar/`; a game would read them through
//! `Context::assets`, and the calls after reading are the same.

use serde::{Deserialize, Serialize};
use yowie::ecs::{
    Entity, Name, Prefab, PrefabLibrary, Scene, SceneInstance, SceneOptions, Schedule, Stage,
    World, propagate_transforms,
};
use yowie::ecs_glue::DeltaTime;
use yowie::rng::Rng;
use yowie::{Quat, Transform};

const SYSTEM: &str = include_str!("../assets/solar/system.json");
const MOON: &str = include_str!("../assets/solar/moon.json");

/// How to draw a body. The scene document names this component
/// `solar.body`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    /// Sphere radius in world units.
    pub radius: f32,
    /// sRGB base colour.
    pub color: [f32; 3],
    /// Glow strength; 0 for a body that only reflects light.
    #[serde(default)]
    pub emissive: f32,
}

/// A circular orbit around the parent entity, in the parent's XZ plane.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Orbit {
    /// Distance from the parent.
    pub radius: f32,
    /// Radians per second.
    pub speed: f32,
    /// Current angle in radians.
    pub angle: f32,
}

/// Turns the body about its own Y axis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spin {
    /// Radians per second.
    pub speed: f32,
}

/// Marks a belt rock, which draws as part of one instanced call.
pub struct Asteroid;

/// Seconds the spin system has run, so rotations follow the clock.
pub struct Clock(pub f32);

fn clock(world: &mut World) {
    let dt = world.resource::<DeltaTime>().map_or(0.0, |d| d.0);
    if let Some(c) = world.resource_mut::<Clock>() {
        c.0 += dt;
    }
}

fn orbits(world: &mut World) {
    let dt = world.resource::<DeltaTime>().map_or(0.0, |d| d.0);
    for (o, t) in world.query_mut::<(&mut Orbit, &mut Transform)>() {
        o.angle += o.speed * dt;
        t.translation.x = o.radius * o.angle.cos();
        t.translation.z = o.radius * o.angle.sin();
    }
}

fn spins(world: &mut World) {
    let now = world.resource::<Clock>().map_or(0.0, |c| c.0);
    for (s, t) in world.query_mut::<(&Spin, &mut Transform)>() {
        t.rotation = Quat::from_rotation_y(now * s.speed);
    }
}

/// The world, its schedule and the entity the scene named `Sun`.
pub struct Solar {
    /// All entities.
    pub world: World,
    /// The systems: clock, spin and orbits, then transform propagation.
    pub schedule: Schedule,
    /// The scene's root.
    pub sun: Entity,
}

impl Solar {
    /// Registers the components, loads the scene and the prefab it refers
    /// to, spawns the belt and builds the schedule.
    pub fn load() -> Result<Self, yowie::BoxError> {
        let mut world = World::new();
        world.register::<Body>("solar.body");
        world.register::<Orbit>("solar.orbit");
        world.register::<Spin>("solar.spin");
        world.insert_resource(Clock(0.0));
        world.insert_resource(DeltaTime(0.0));

        let scene = Scene::from_json(SYSTEM)?;
        let mut prefabs = PrefabLibrary::new();
        prefabs.insert("moon", Prefab::from_json(MOON)?);
        let options = SceneOptions {
            prefabs: Some(&prefabs),
            ..SceneOptions::default()
        };
        let instance: SceneInstance = world.instantiate_scene(&scene, &options)?;
        let sun = instance.entity("Sun").ok_or("the scene names no Sun")?;

        // The belt is procedural, so it is spawned in code and hung under
        // the sun like the scene's own planets.
        let count = scene
            .properties
            .get("belt")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let seed = scene
            .properties
            .get("belt_seed")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        let mut rng = Rng::new(seed);
        for _ in 0..count {
            let shade = rng.between(0.45, 0.62);
            let rock = world.spawn((
                Body {
                    radius: rng.between(0.05, 0.11),
                    color: [shade, shade * 0.95, shade * 0.88],
                    emissive: 0.0,
                },
                Asteroid,
                Orbit {
                    radius: rng.between(13.5, 16.0),
                    speed: rng.between(0.1, 0.15),
                    angle: rng.between(0.0, std::f32::consts::TAU),
                },
                Transform::from_xyz(0.0, rng.between(-0.4, 0.4), 0.0),
            ));
            world.set_parent(rock, sun)?;
        }

        let mut schedule = Schedule::new();
        schedule.add_system(Stage::Update, "clock", clock);
        schedule.add_system(Stage::Update, "orbits", orbits);
        schedule.add_system(Stage::Update, "spin", spins);
        schedule.add_system(Stage::PostUpdate, "transforms", propagate_transforms);
        Ok(Solar {
            world,
            schedule,
            sun,
        })
    }

    /// The name of an entity, or `"?"`.
    pub fn name(&self, e: Entity) -> String {
        self.world
            .get::<&Name>(e)
            .map_or_else(|_| "?".to_owned(), |n| n.0.clone())
    }
}
