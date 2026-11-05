---
title: Physics
group: Simulation
order: 1
summary: rigid bodies in 2D and 3D, shapes, materials, collision events, queries, joints, ragdolls and character controllers, and the cloth, soft bodies and fluids beside them
---

# Physics

`yowie::phys` simulates rigid bodies in 2D and 3D, with joints, sleeping,
continuous collision, scene queries and character controllers. The rigid
bodies run on rapier behind Yowie's own types: positions are `Vec2` and
`Vec3`, rotations are radians in 2D and `Quat` in 3D, and no rapier type
appears in the API. The `phys::soft` module adds cloth, volumetric soft
bodies and 2D fluids that collide with the same colliders.

The module is on by default (the `phys` feature). Each name carries its
dimension: `World2D` and `World3D`, `BodyDesc2D` and `BodyDesc3D`,
`Shape2D` and `Shape3D`, and so on. Types shared by both, such as
`BodyId`, `Event` and `QueryFilter`, have no suffix.

A world is a plain value the game owns, outside the ECS. Keep it in your
`Game` struct (or in an ECS resource) and call `step` from `update`. To
see each topic running, use the examples:

| Example | What it shows |
|---|---|
| `cargo run --example physics2d` | Falling shapes, a kinematic paddle, a car on wheel joints, a trigger zone and a ray cast. Click drops a ball, D toggles debug lines. |
| `cargo run --example physics3d` | 500 cubes in a pen, interpolated drawing, sleeping, and picking the cube under the pointer with a ray. R drops again, D toggles debug lines. |
| `cargo run --example physics_lab` | Hulls, capsules and spheres on a mesh terrain, a hinge chain, a motor-driven paddle wheel, a ragdoll and a character walking stairs. The console (`` ` ``) has `spawn`, `reset`, `ragdoll`, `kick`, `pause`, `debug` and `gravity`; `--exec "spawn 20; ragdoll"` runs commands at start. |
| `cargo run --example softbody` | A cloth flag in a gust, a jelly cube beside a rigid crate, and a tank of 2D fluid. Space or a click kicks the jelly; `--kick <seconds>` kicks once at that game time. |

For the full API, run `cargo doc --open` and open `yowie::phys`.

## A first world

To create a world, call `World3D::new` or `World2D::new` with the gravity
in units per second squared. `Default` gives a gravity of 9.81 down the
Y axis. Add bodies with `add_body`, attach colliders with `add_collider`,
and advance time with `step`:

```rust
use yowie::Vec3;
use yowie::phys::{BodyDesc3D, ColliderDesc3D, Shape3D, World3D};

fn main() {
    let mut world = World3D::default();

    // Level geometry: a collider with no body.
    let floor = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(10.0, 0.5, 10.0)));
    world.add_static_collider(&floor).unwrap();

    // A ball three units up.
    let ball = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, 3.0, 0.0)));
    world
        .add_collider(ball, &ColliderDesc3D::new(Shape3D::sphere(0.5)))
        .unwrap();

    for _ in 0..120 {
        world.step(1.0 / 60.0);
    }
    // The ball rests on the floor, whose top is at y = 0.5.
    let y = world.position(ball).unwrap().y;
    assert!((y - 1.0).abs() < 0.05);
}
```

The default gravity of 9.81 assumes metres. `physics2d` models its world
in metres and draws it at 40 pixels per metre through a `Camera2D`. To
work in another unit, scale gravity to match: the fluid in `softbody`
runs in view pixels with a gravity of 900.

### Up in 2D

`World2D::default()` pulls toward -Y, which is down in a Y-up world. The
2D view has its origin at the top-left with +Y pointing down, so a world
drawn straight into view coordinates needs gravity toward +Y, as
`physics2d` does:

```rust
use yowie::phys::World2D;
use yowie::vec2;

/// A world whose +Y matches the 2D view: down the screen.
fn screen_world() -> World2D {
    World2D::new(vec2(0.0, 9.81))
}
```

Rotations in 2D are radians, counter-clockwise when +Y is up. In a Y-down
view the same angle turns clockwise on screen. Draw with the same
transform the world reports and the picture matches the simulation.

## Stepping in a game

To advance the world, call `step` once per update with
`ctx.time.delta()`. In the default fixed-step loop every update covers
the same time, which is what the solver needs. `step` ignores a zero,
negative or non-finite `dt`.

The loop runs updates at a fixed rate and draws once per displayed
frame, so a frame usually falls between two updates. `Time::alpha` says
how far, from 0 to 1. To draw a body without stutter, pass it to
`interpolated_transform`, which blends the body's pose from the step
before the last to the last:

```rust,no_run
use yowie::gfx::Camera2D;
use yowie::phys::{BodyDesc2D, BodyId, ColliderDesc2D, Shape2D, World2D};
use yowie::{Color, Config, Context, Game, GameResult, Rect, Transform2D, vec2};

/// Pixels per metre.
const PPM: f32 = 40.0;

struct Drop {
    world: World2D,
    ball: BodyId,
}

impl Game for Drop {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        // +Y points down the screen, so gravity is positive.
        let mut world = World2D::new(vec2(0.0, 9.81));
        let floor = ColliderDesc2D::new(Shape2D::cuboid(vec2(12.0, 0.5)))
            .with_offset(Transform2D::from_position(vec2(12.0, 13.0)));
        world.add_static_collider(&floor)?;
        let ball = world.add_body(&BodyDesc2D::dynamic().at(vec2(12.0, 2.0)));
        let shape = ColliderDesc2D::new(Shape2D::circle(0.5)).with_restitution(0.6);
        world.add_collider(ball, &shape)?;
        Ok(Drop { world, ball })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.world.step(ctx.time.delta());
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // A 960 by 540 view at 40 pixels per metre shows 24 by 13.5 metres.
        let camera = Camera2D::new(vec2(12.0, 6.75)).with_zoom(PPM);
        let ball = self.world.interpolated_transform(self.ball, ctx.time.alpha());
        ctx.gfx.with_camera(&camera, |g| {
            g.fill_rect(Rect::new(0.0, 12.5, 24.0, 1.0), Color::rgb8(86, 92, 112));
            if let Some(t) = ball {
                g.fill_circle(t.position, 0.5, Color::rgb8(240, 120, 90));
            }
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Drop>(Config::new("Drop").with_size(960, 540))
}
```

`PhysError` converts into the error type of `GameResult`, so `?` works
in `init`, `update` and `draw`. Under
`YOWIE_FIXED_CLOCK=1` and in turn-based loops `Time::alpha` is 0, so
interpolation draws the previous pose.

### Your own fixed step

To run physics at a rate other than the update rate, or from a loop
whose delta varies (a turn-based loop), use `FixedStepper`.
`FixedStepper::new(fixed_dt, max_steps)` accumulates frame time.
`advance(frame_dt)` returns how many fixed steps are due, never more than
`max_steps`, and drops the rest so a long frame cannot stall the game.
`alpha()` is the fraction of a step left over, for
`interpolated_transform`. `FixedStepper::default()` steps at 1/60 second
with at most 8 steps per frame.

```rust
use yowie::Transform2D;
use yowie::phys::{BodyId, FixedStepper, World2D};

/// Runs the fixed steps `frame_dt` seconds owe and returns where to draw
/// `body` this frame.
fn advance(
    world: &mut World2D,
    stepper: &mut FixedStepper,
    body: BodyId,
    frame_dt: f32,
) -> Option<Transform2D> {
    for _ in 0..stepper.advance(frame_dt) {
        world.step(stepper.fixed_dt());
    }
    world.interpolated_transform(body, stepper.alpha())
}
```

## Bodies

A body is a position, a rotation and velocities. It has no shape until
you attach colliders. To create one, fill a `BodyDesc2D` or `BodyDesc3D`
and pass it to `add_body`, which returns a `BodyId`. The descriptor
starts from one of three kinds:

| Constructor | Kind | Behaviour |
|---|---|---|
| `dynamic()` | `BodyKind::Dynamic` | Moved by gravity, forces, contacts and joints |
| `kinematic()` | `BodyKind::Kinematic` | Moved only by the game; pushes dynamic bodies and is never pushed |
| `fixed()` | `BodyKind::Static` | Never moves; the cheapest to collide against |

Builder methods set the other fields; every field is also public:

| Method | Field | Default |
|---|---|---|
| `at(position)` | `position` | origin |
| `rotated(angle)` (2D radians, 3D `Quat`) | `rotation` | 0, identity |
| `moving(velocity)` | `linear_velocity` | zero |
| | `angular_velocity` (2D radians per second; 3D an axis scaled by the rate) | zero |
| `damped(linear, angular)` | `linear_damping`, `angular_damping` (fraction of velocity lost per second) | 0, 0 |
| | `gravity_scale` | 1 |
| | `can_sleep` | true |
| `with_ccd()` | `ccd` | false |
| `without_rotation()` | `lock_rotation` | false |

```rust
use yowie::phys::{BodyDesc3D, BodyId, ColliderDesc3D, PhysError, Shape3D, World3D};
use yowie::{Quat, Vec3};

/// Drops a crate one unit on a side, turned a little about Y.
fn spawn_crate(world: &mut World3D, at: Vec3) -> Result<BodyId, PhysError> {
    let body = world.add_body(
        &BodyDesc3D::dynamic()
            .at(at)
            .rotated(Quat::from_rotation_y(0.3))
            .damped(0.1, 0.1),
    );
    let shape = ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5)));
    world.add_collider(body, &shape)?;
    Ok(body)
}
```

To remove a body with its colliders and joints, call `remove_body`.
Ids are generational: after a removal the old id never names another
body. Reading through a stale id returns `None`, and changing through
one returns `PhysError::NoSuchBody`.

### Reading and moving bodies

| Call | Returns or does |
|---|---|
| `position(id)`, `rotation(id)`, `transform(id)` | The pose after the last step (`Transform2D` or `Transform`, scale 1) |
| `interpolated_transform(id, alpha)` | The pose blended between the last two steps |
| `linear_velocity(id)`, `angular_velocity(id)` | Units per second; radians per second |
| `mass(id)` | The sum over the colliders of density times area (2D) or volume (3D) |
| `kind(id)` | The `BodyKind` |
| `set_transform(id, position, rotation)` | Teleports the body and keeps its velocities |
| `set_linear_velocity`, `set_angular_velocity` | Sets the velocity and wakes the body |
| `apply_impulse(id, impulse)` | Changes the velocity by `impulse / mass` at once |
| `apply_impulse_at_point(id, impulse, point)` | The same at a world point, which also spins the body |
| `apply_force(id, force)` | Adds a force that stays on every step until `reset_forces` |

To make a body jump, apply an impulse of its mass times the speed you
want:

```rust
use yowie::phys::{BodyId, PhysError, World2D};
use yowie::vec2;

/// Launches `body` upward (toward -Y in a Y-down world) at `speed` units
/// per second.
fn jump(world: &mut World2D, body: BodyId, speed: f32) -> Result<(), PhysError> {
    let mass = world.mass(body).unwrap_or(1.0);
    world.apply_impulse(body, vec2(0.0, -mass * speed))
}
```

### Kinematic bodies

To move a kinematic body, call `set_kinematic_target` with where it
should be after the next step. The world derives the body's velocity from
the move, so the body pushes and carries the dynamic bodies it touches.
Use it for moving platforms, doors and paddles:

```rust
use yowie::phys::{BodyId, PhysError, World2D};
use yowie::vec2;

/// Swings a paddle three units either side of x = 5 along the floor.
fn move_paddle(world: &mut World2D, paddle: BodyId, time: f32) -> Result<(), PhysError> {
    let x = 5.0 + 3.0 * (time * 0.8).sin();
    world.set_kinematic_target(paddle, vec2(x, 12.0), 0.0)
}
```

Call it each update before `step`.

## Shapes and colliders

A collider gives a body its shape and surface. To create one, fill a
`ColliderDesc2D` or `ColliderDesc3D` with a shape and pass it to
`add_collider` with the body. A body can have several colliders; its
mass is the sum of theirs. To add level geometry with no body, call
`add_static_collider`: the collider never moves and its `offset` is its
world placement.

The 2D shapes:

| `Shape2D` | Notes |
|---|---|
| `circle(radius)` | |
| `cuboid(half_extents)` | A box, axis-aligned in the collider's space |
| `capsule(half_height, radius)` | Along Y: a segment from `-half_height` to `half_height` swept by `radius` |
| `ConvexPolygon(points)` | The convex hull of at least three points that are not collinear |
| `Segment { a, b }` | No inside; use for static geometry |
| `Polyline { points, closed }` | A chain of segments; for static outlines |
| `Heightfield { heights, scale }` | Samples spaced evenly over `scale.x` (centred on the origin), heights times `scale.y`, filled below; for static terrain |
| `Compound(parts)` | Several `(Transform2D, Shape2D)` parts acting as one collider |

The 3D shapes:

| `Shape3D` | Notes |
|---|---|
| `sphere(radius)` | |
| `cuboid(half_extents)` | |
| `capsule(half_height, radius)` | Along Y |
| `Cylinder { half_height, radius }` | Along Y |
| `ConvexHull(points)` | The hull of at least four points that are not coplanar |
| `TriMesh { vertices, indices }` | Hollow: collides on its surface only, so use it for static geometry |
| `Heightfield { rows, cols, heights, scale }` | `heights[row * cols + col]`, rows along Z and columns along X, centred on the origin; `scale` is the total X extent, a height multiplier and the total Z extent |
| `grid_heightfield(cols, rows, cell, heights)` | A `Heightfield` from a grid with `cell` units between samples |
| `Compound(parts)` | Several `(Transform, Shape3D)` parts |

A shape that cannot be built (a negative radius, a hull of too few
points, a mesh index out of range) makes `add_collider` return
`PhysError::InvalidShape`.

To place a collider away from the body's origin, set its offset with
`with_offset`. To build an L-shaped body, attach two boxes, or use one
`Compound` collider:

```rust
use yowie::phys::{BodyDesc2D, BodyId, ColliderDesc2D, PhysError, Shape2D, World2D};
use yowie::{Transform2D, Vec2, vec2};

/// A dynamic L made of two boxes on one body.
fn spawn_l(world: &mut World2D, at: Vec2) -> Result<BodyId, PhysError> {
    let body = world.add_body(&BodyDesc2D::dynamic().at(at));
    let upright = ColliderDesc2D::new(Shape2D::cuboid(vec2(0.25, 1.0)));
    let foot = ColliderDesc2D::new(Shape2D::cuboid(vec2(0.5, 0.25)))
        .with_offset(Transform2D::from_position(vec2(0.25, 0.75)));
    world.add_collider(body, &upright)?;
    world.add_collider(body, &foot)?;
    Ok(body)
}
```

To collide with a terrain drawn by `yowie::gfx::scene::Terrain`, build a
heightfield from its height map and put it on a fixed body at the
terrain's centre. `grid_heightfield` splits each cell along the same
diagonal the terrain draws, so bodies rest on the surface the player
sees:

```rust
use yowie::gfx::scene::Terrain;
use yowie::phys::{BodyDesc3D, BodyId, ColliderDesc3D, PhysError, Shape3D, World3D};

/// Adds a static collider that matches `terrain`.
fn terrain_collider(world: &mut World3D, terrain: &Terrain) -> Result<BodyId, PhysError> {
    let map = terrain.height_map();
    let shape =
        Shape3D::grid_heightfield(map.cols(), map.rows(), terrain.cell(), map.heights().to_vec());
    let body = world.add_body(&BodyDesc3D::fixed().at(terrain.center()));
    world.add_collider(body, &ColliderDesc3D::new(shape))?;
    Ok(body)
}
```

To remove one collider, call `remove_collider`. `body_of(collider)` and
`colliders_of(body)` map between the two.

## Materials

`ColliderDesc2D::new` and `ColliderDesc3D::new` make a solid collider
with friction 0.5, no bounce and density 1. These fields set how it
behaves on contact:

| Method | Field | Meaning |
|---|---|---|
| `with_friction(f)` | `friction` | Coulomb friction coefficient, at least 0 |
| `with_restitution(r)` | `restitution` | Bounce, from 0 (none) to 1 (no energy lost) |
| `with_density(d)` | `density` | Mass per unit area (2D) or volume (3D) |
| | `friction_rule`, `restitution_rule` | How the values of two touching colliders combine: `CombineRule::Average` (default), `Min`, `Multiply` or `Max` |

`with_restitution` also sets `restitution_rule` to `CombineRule::Max`, so
one bouncy collider is enough to bounce off any surface. To make ice,
set a low friction and the `Min` rule, so a contact with the ice uses the
ice's friction whatever the other collider's is:

```rust
use yowie::phys::{ColliderDesc2D, CombineRule, Shape2D};
use yowie::vec2;

/// A slippery floor tile.
fn ice(half_extents: yowie::Vec2) -> ColliderDesc2D {
    ColliderDesc2D {
        friction_rule: CombineRule::Min,
        ..ColliderDesc2D::new(Shape2D::cuboid(half_extents)).with_friction(0.02)
    }
}

fn main() {
    let tile = ice(vec2(2.0, 0.5));
    assert_eq!(tile.friction_rule, CombineRule::Min);
}
```

## Collision groups

To decide which colliders touch, set `CollisionLayers` on them. Each
collider has `memberships`, the groups it belongs to, and `mask`, the
groups it can touch, one bit per group. Two colliders interact only when
each one's memberships intersect the other's mask. The default,
`CollisionLayers::ALL`, belongs to every group and touches every group.

```rust
use yowie::phys::{ColliderDesc2D, CollisionLayers, Shape2D};

const PLAYER: u32 = 1 << 0;
const ENEMY: u32 = 1 << 1;
const PLAYER_SHOT: u32 = 1 << 2;
const WALL: u32 = 1 << 3;

/// Player shots hit enemies and walls, and pass through the player and
/// each other.
fn player_shot() -> ColliderDesc2D {
    ColliderDesc2D::new(Shape2D::circle(0.1))
        .with_layers(CollisionLayers::new(PLAYER_SHOT, ENEMY | WALL))
}

/// Enemies touch the player, player shots, walls and each other.
fn enemy() -> ColliderDesc2D {
    ColliderDesc2D::new(Shape2D::circle(0.4))
        .with_layers(CollisionLayers::new(ENEMY, PLAYER | PLAYER_SHOT | WALL | ENEMY))
}
```

Both sides must agree: a shot whose mask names `ENEMY` still passes
through an enemy whose mask leaves out `PLAYER_SHOT`. Layers also filter
queries, through `QueryFilter::with_layers`, and soft bodies, through
their `layers` field.

## Contact and trigger events

Each `step` records what happened as a list of `Event`s. Read them with
`events()` after the step, or move them out with `take_events()`. The
next step discards them.

| Event | When |
|---|---|
| `ContactStarted { a, b }` | Two solid colliders began touching |
| `ContactStopped { a, b }` | They stopped touching |
| `SensorEntered { sensor, other }` | A collider began overlapping a sensor |
| `SensorExited { sensor, other }` | It stopped overlapping, or was removed |
| `ContactImpulse { a, b, total, max }` | Two colliders pushed on each other this step; impulses in newton-seconds |

Events name colliders. Use `body_of` to find the body. The start and stop
events are on by default (the `events` field of `ColliderDesc2D` and
`ColliderDesc3D`); set `events` to false on colliders whose contacts you
never read. `ContactImpulse` is reported for each step in which the
colliders push on each other, and only for colliders made with
`with_contact_events()`. Use it to pick the volume of an impact sound or
to break something that was hit hard.

A sensor detects overlaps and reports them but does not collide. To make
a trigger zone, call `as_sensor()`:

```rust
use yowie::phys::{BodyDesc2D, ColliderDesc2D, ColliderId, Event, Shape2D, World2D};
use yowie::{Transform2D, vec2};

/// Counts the colliders that entered `zone` during the last step.
fn entered(world: &World2D, zone: ColliderId) -> usize {
    world
        .events()
        .iter()
        .filter(|e| matches!(e, Event::SensorEntered { sensor, .. } if *sensor == zone))
        .count()
}

fn main() {
    let mut world = World2D::default();
    let zone = world
        .add_static_collider(
            &ColliderDesc2D::new(Shape2D::cuboid(vec2(2.0, 1.0)))
                .with_offset(Transform2D::from_position(vec2(0.0, 3.0)))
                .as_sensor(),
        )
        .unwrap();
    let ball = world.add_body(&BodyDesc2D::dynamic().at(vec2(0.0, 6.0)));
    world
        .add_collider(ball, &ColliderDesc2D::new(Shape2D::circle(0.25)))
        .unwrap();

    let mut total = 0;
    for _ in 0..120 {
        world.step(1.0 / 60.0);
        total += entered(&world, zone);
    }
    assert_eq!(total, 1);
}
```

Sensor events and contact events are reported for pairs that include a
dynamic body. A kinematic body, such as a character moved by a
[character controller](#character-controllers), produces no
`SensorEntered` for a sensor with no body or on a fixed or kinematic
body. To find out whether a kinematic body is inside a zone, query for
it each update:

```rust
use yowie::phys::{BodyDesc2D, BodyId, ColliderDesc2D, ColliderId, QueryFilter, Shape2D, World2D};
use yowie::{Transform2D, vec2};

/// Whether the kinematic `hero`, a circle of `radius`, overlaps `zone`.
fn hero_in_zone(world: &World2D, hero: BodyId, radius: f32, zone: ColliderId) -> bool {
    let Some(at) = world.transform(hero) else {
        return false;
    };
    world
        .overlap_shape(&Shape2D::circle(radius), &at, &QueryFilter::new())
        .is_ok_and(|hits| hits.contains(&zone))
}

fn main() {
    let mut world = World2D::new(vec2(0.0, 0.0));
    let zone = ColliderDesc2D::new(Shape2D::cuboid(vec2(1.0, 1.0)))
        .with_offset(Transform2D::from_position(vec2(5.0, 0.0)))
        .as_sensor();
    let zone = world.add_static_collider(&zone).unwrap();
    let hero = world.add_body(&BodyDesc2D::kinematic());
    world
        .add_collider(hero, &ColliderDesc2D::new(Shape2D::circle(0.25)))
        .unwrap();

    let (mut inside, mut entered) = (false, 0);
    for i in 0..120 {
        world
            .set_kinematic_target(hero, vec2(i as f32 * 0.1, 0.0), 0.0)
            .unwrap();
        world.step(1.0 / 60.0);
        let now = hero_in_zone(&world, hero, 0.25, zone);
        if now && !inside {
            entered += 1;
        }
        inside = now;
    }
    assert_eq!(entered, 1);
    assert!(!inside);
}
```

## Queries

Queries ask the world about its colliders without changing it. They see
the colliders as of the last `step` or `refresh_queries`. After you add,
remove or move bodies, and before the next step, call `refresh_queries`
if you need queries to see the change. It updates collision detection
without advancing time. Any order of adding, removing, refreshing,
querying and stepping is valid. Events produced by a refresh are added
to `events()`, and the next step discards them.

| Call | Returns |
|---|---|
| `ray_cast(origin, direction, max_distance, filter)` | The nearest `RayHit` (collider, body, point, normal, distance), or `None` |
| `ray_cast_all(...)` | Every hit along the ray, nearest first |
| `shape_cast(shape, at, direction, max_distance, filter)` | The first `ShapeHit` of a shape swept along a direction |
| `overlap_shape(shape, at, filter)` | Every collider overlapping a placed shape |
| `overlap_point(point, filter)` | Every collider containing a point |
| `collider_distance(collider, point)` | A `SurfaceDistance`: signed distance (negative inside), closest point and normal |
| `signed_distance_2d(shape, position, angle, point)`, `signed_distance_3d(shape, position, rotation, point)` | The same for a shape that is not in a world |

Directions do not need to be normalised; a zero direction finds nothing.
A ray that starts inside a solid collider reports it at distance 0.

A `QueryFilter` limits what a query reports. `QueryFilter::new()` reports
everything. `with_layers(layers)` keeps colliders that interact with the
layers, `without_sensors()` skips sensors, and `without_body(id)` skips a
body's colliders, such as the caster's own. The field `exclude_collider`
skips one collider, and `exclude_static` skips colliders on fixed bodies
and colliders with no body.

```rust
use yowie::Vec3;
use yowie::phys::{BodyId, ColliderDesc3D, QueryFilter, RayHit, Shape3D, World3D};

/// The first thing below `from`, ignoring the caster's own body.
fn ground_below(world: &World3D, from: Vec3, own: BodyId) -> Option<RayHit<Vec3>> {
    world.ray_cast(from, Vec3::NEG_Y, 100.0, &QueryFilter::new().without_body(own))
}

fn main() {
    let mut world = World3D::default();
    let floor = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(10.0, 0.5, 10.0)));
    let floor = world.add_static_collider(&floor).unwrap();
    let probe = world.add_body(&yowie::phys::BodyDesc3D::kinematic());
    // The floor is new, so bring the query structures up to date first.
    world.refresh_queries();

    let hit = ground_below(&world, Vec3::new(1.0, 5.0, 2.0), probe).unwrap();
    assert_eq!(hit.collider, floor);
    assert!((hit.point.y - 0.5).abs() < 1e-3);
    assert!((hit.distance - 4.5).abs() < 1e-3);
}
```

To pick what is under the mouse in 3D, make a ray from the camera with
`Camera3D::screen_ray(mouse, view_size)` and cast it, as `physics3d`
does:

```rust,no_run
use yowie::Context;
use yowie::gfx::scene::Camera3D;
use yowie::phys::{BodyId, QueryFilter, World3D};

/// The body under the pointer, if any.
fn pick(ctx: &Context, camera: &Camera3D, world: &World3D) -> Option<BodyId> {
    let ray = camera.screen_ray(ctx.input.mouse(), ctx.gfx.view_size());
    world
        .ray_cast(ray.origin, ray.direction, 200.0, &QueryFilter::new())
        .and_then(|hit| hit.body)
}
```

An explosion is an overlap query followed by impulses. A body with
several colliders appears once per collider, so sort and remove
duplicates:

```rust
use yowie::phys::{BodyId, PhysError, QueryFilter, Shape3D, World3D};
use yowie::{Transform, Vec3};

/// Pushes every movable body within `radius` of `centre` away from it.
fn explode(world: &mut World3D, centre: Vec3, radius: f32, push: f32) -> Result<(), PhysError> {
    let filter = QueryFilter {
        exclude_static: true,
        ..QueryFilter::new().without_sensors()
    };
    let at = Transform::from_translation(centre);
    let hits = world.overlap_shape(&Shape3D::sphere(radius), &at, &filter)?;
    let mut bodies: Vec<BodyId> = hits.iter().filter_map(|c| world.body_of(*c)).collect();
    bodies.sort();
    bodies.dedup();
    for body in bodies {
        if let Some(p) = world.position(body) {
            let away = (p - centre).normalize_or(Vec3::Y);
            world.apply_impulse(body, away * push)?;
        }
    }
    Ok(())
}
```

To find how far a shape can move before it hits something, sweep it
with `shape_cast`:

```rust
use yowie::phys::{PhysError, QueryFilter, Shape3D, World3D};
use yowie::{Transform, Vec3};

/// How far a ball of `radius` can travel from `from` along `direction`,
/// up to `max`.
fn free_distance(
    world: &World3D,
    from: Vec3,
    direction: Vec3,
    radius: f32,
    max: f32,
) -> Result<f32, PhysError> {
    let at = Transform::from_translation(from);
    let hit = world.shape_cast(&Shape3D::sphere(radius), &at, direction, max, &QueryFilter::new())?;
    Ok(hit.map_or(max, |h| h.distance))
}
```

## Joints

A joint constrains two bodies. To add one, fill a `JointDesc2D` or
`JointDesc3D` and call `add_joint(a, b, &desc)`, which returns a
`JointId`. `with_anchors(a, b)` sets the attachment point in each body's
local space; both default to the body origins. The bodies stop colliding
with each other unless you set `collide_connected`.

| Kind | 2D (`JointKind2D`) | 3D (`JointKind3D`) | Limits | Motor |
|---|---|---|---|---|
| Hinge (revolute) | `Hinge` | `Hinge { axis }` | Angle | Yes |
| Prismatic (slider) | `Prismatic { axis }` | `Prismatic { axis }` | Distance | Yes |
| Fixed | `Fixed` | `Fixed` | | |
| Ball and socket | | `Spherical { limits }` (per local axis) | Angles | |
| Spring | `Distance { rest_length, stiffness, damping }` | the same | | |
| Rope | `Rope { max_length }` | the same | | |
| Wheel with suspension | `Wheel { axis, stiffness, damping, motor }` | | | Spin |

Axes are in the local space of both bodies. To limit a hinge (radians)
or a slider (units), call `with_limits` with a `Limits::new(min, max)`.
To drive a hinge, slider or 2D wheel, call `with_motor` with a `Motor`.
Both are ignored on the other kinds.

A chain of hinges hanging from a fixed body:

```rust
use yowie::Vec3;
use yowie::phys::{BodyDesc3D, BodyId, ColliderDesc3D, JointDesc3D, PhysError, Shape3D, World3D};

/// Hangs `links` one-unit links from `top`, hinged about Z.
fn chain(world: &mut World3D, top: Vec3, links: usize) -> Result<Vec<BodyId>, PhysError> {
    let anchor = world.add_body(&BodyDesc3D::fixed().at(top));
    let hinge = JointDesc3D::hinge(Vec3::Z);
    let (mut prev, mut prev_end) = (anchor, Vec3::ZERO);
    let mut bodies = Vec::new();
    for i in 0..links {
        let at = top + Vec3::new(0.5 + i as f32, 0.0, 0.0);
        let link = world.add_body(&BodyDesc3D::dynamic().at(at).damped(0.5, 0.5));
        let shape = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(0.5, 0.12, 0.12)));
        world.add_collider(link, &shape)?;
        let joint = hinge.clone().with_anchors(prev_end, Vec3::new(-0.5, 0.0, 0.0));
        world.add_joint(prev, link, &joint)?;
        (prev, prev_end) = (link, Vec3::new(0.5, 0.0, 0.0));
        bodies.push(link);
    }
    Ok(bodies)
}

fn main() {
    let mut world = World3D::default();
    let links = chain(&mut world, Vec3::new(0.0, 10.0, 0.0), 5).unwrap();
    for _ in 0..240 {
        world.step(1.0 / 60.0);
    }
    // The chain swung down and still hangs from its anchor.
    let end = world.position(links[4]).unwrap();
    assert!(end.y < 10.0);
    assert!(end.distance(Vec3::new(0.0, 10.0, 0.0)) < 5.2);
}
```

### Motors

A `Motor` drives a joint axis. `Motor::velocity(speed, max_force)` turns
the axis at `speed` (radians or units per second) with at most
`max_force`. `Motor::position(target, stiffness, damping)` holds the
axis at `target` with the given stiffness and damping. The fields are
public for other combinations.

```rust
use yowie::Vec3;
use yowie::phys::{BodyId, JointDesc3D, JointId, Motor, PhysError, World3D};

/// Turns `wheel` about Z on a hinge at `axle`'s origin.
fn drive(world: &mut World3D, axle: BodyId, wheel: BodyId) -> Result<JointId, PhysError> {
    let joint = JointDesc3D::hinge(Vec3::Z).with_motor(Motor::velocity(1.5, 400.0));
    world.add_joint(axle, wheel, &joint)
}

/// Changes the wheel's speed and torque while it runs.
fn set_speed(world: &mut World3D, joint: JointId, speed: f32) -> Result<(), PhysError> {
    world.set_joint_motor(joint, Motor::velocity(speed, 400.0))
}
```

`World3D::set_joint_motor` replaces the motor of a hinge or slider while
the game runs. `World2D` has no such call; to change a 2D motor, remove
the joint and add it again. To remove a joint, call `remove_joint`.

The 2D `Wheel` joint is for vehicles: the second body spins freely and
slides along `axis` against a spring suspension, and the motor drives
the spin. `physics2d` builds its car from a chassis and two wheels:

```rust
use yowie::phys::{BodyId, JointDesc2D, JointId, JointKind2D, Motor, PhysError, World2D};
use yowie::{Vec2, vec2};

/// Hangs `wheel` under `chassis` at `offset` on a sprung, driven axle.
fn axle(
    world: &mut World2D,
    chassis: BodyId,
    wheel: BodyId,
    offset: Vec2,
) -> Result<JointId, PhysError> {
    let joint = JointDesc2D::new(JointKind2D::Wheel {
        axis: Vec2::Y,
        stiffness: 900.0,
        damping: 60.0,
        motor: Some(Motor::velocity(3.0, 40.0)),
    })
    .with_anchors(offset, vec2(0.0, 0.0));
    world.add_joint(chassis, wheel, &joint)
}
```

## Ragdolls

To drop a humanoid ragdoll into a 3D world, call `Ragdoll3D::spawn` with
a `RagdollSpec`. It builds eleven capsules (named in `RAGDOLL_PARTS`:
pelvis, spine, head, upper arms, forearms, thighs and shins) joined by
ball joints at the neck, shoulders, waist and hips and limited hinges at
the elbows and knees. A zero field in the spec takes its default:

| Field | Default |
|---|---|
| `position` | The point on the ground between the feet |
| `rotation` | The figure faces -Z with +X on its right |
| `height` | 1.8 units; the parts scale with it |
| `mass` | 70, shared between the parts |
| `friction` | 0.6 |
| `damping` | 0.5, linear and angular |
| `layers` | `None`: membership of group bit 31 only, touching every group except bit 31, so parts touch the world but not each other or other ragdolls |
| `bones` | Per-part length and radius overrides |

```rust
use yowie::phys::{ColliderDesc3D, Ragdoll3D, RagdollSpec, Shape3D, World3D};
use yowie::{Quat, Vec3};

fn main() {
    let mut world = World3D::default();
    let floor = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(20.0, 0.5, 20.0)))
        .with_offset(yowie::Transform::from_xyz(0.0, -0.5, 0.0));
    world.add_static_collider(&floor).unwrap();

    let spec = RagdollSpec {
        position: Vec3::new(0.0, 2.0, 0.0),
        rotation: Quat::from_rotation_x(0.7),
        ..RagdollSpec::default()
    };
    let doll = Ragdoll3D::spawn(&mut world, &spec).unwrap();
    for _ in 0..180 {
        world.step(1.0 / 60.0);
    }
    let head = world.position(doll.parts["head"]).unwrap();
    assert!(head.y < 1.5);

    doll.despawn(&mut world);
    assert_eq!(world.body_count(), 0);
}
```

`Ragdoll3D` holds the `parts`, `joints` and `bones` maps by part name.
To hand an animated character over to physics, call `pose` with the
world positions and rotations of the parts by name; it places them and
zeroes their velocities. `despawn` removes every part and joint.

## Fast bodies and sleeping

A small, fast body can pass through a thin wall between two steps. To
stop that, turn on continuous collision detection with `with_ccd()` on
the body. It costs more, so use it for bullets and fast balls only.
`physics_lab` turns it on for its spheres.

A body that stops moving falls asleep and costs almost nothing until
something touches it, a joint pulls it or you change its velocity.
`is_sleeping(id)` reports it; `sleep(id)` and `wake(id)` force it. Set
`can_sleep` to false on a body that must never sleep, such as a body the
player drives with forces. `physics3d` counts its sleeping cubes, and
sleeping bodies are drawn darker in debug drawing.

## Character controllers

A character controller moves a shape through the world and stops it at
walls, climbs steps and slopes and keeps it on the ground. It moves a
kinematic body, so gravity and jumping are up to you. Configure it with
`CharacterController2D` or `CharacterController3D`:

| Field | Default from `new(shape)` | Meaning |
|---|---|---|
| `shape` | | The character's shape, usually a capsule |
| `up` | +Y | The direction that counts as up |
| `skin` | 0.01 | Gap kept between the shape and surfaces |
| `max_climb_angle` | 45 degrees (radians) | Steepest slope it walks up |
| `min_slide_angle` | 45 degrees | Slope above which it slides down |
| `step_height` | `Some(0.3)` | Tallest step climbed; `None` turns stepping off |
| `min_step_width` | 0.1 | Narrowest ledge that counts as a step |
| `snap_to_ground` | `Some(0.2)` | Distance within which it is pulled onto the ground; `None` turns snapping off |
| `slide` | true | Slide along walls instead of stopping |

To move a character, give it a kinematic body with a collider of the same
shape, and call `move_character_body(&controller, body, desired, dt)`
each update before `step`. `desired` is the move you want this update, in
world units. The call computes the allowed move, sets it as the body's
kinematic target and returns a `CharacterMove2D` or `CharacterMove3D`
with the `translation`
applied, whether the character is `grounded`, and whether it is
`sliding` down a steep slope. Sensors do not block it.

```rust
use yowie::phys::{
    BodyDesc3D, BodyId, CharacterController3D, ColliderDesc3D, PhysError, Shape3D, World3D,
};
use yowie::{Vec2, Vec3};

/// Moves the hero at 4 units per second in the direction of `stick`
/// (x along X, y along Z), with gravity. `fall` is the vertical speed,
/// kept between updates. Returns whether the hero stands on ground.
fn walk(
    world: &mut World3D,
    ctrl: &CharacterController3D,
    hero: BodyId,
    stick: Vec2,
    fall: &mut f32,
    dt: f32,
) -> Result<bool, PhysError> {
    *fall -= 9.81 * dt;
    let desired = Vec3::new(stick.x * 4.0, *fall, stick.y * 4.0) * dt;
    let m = world.move_character_body(ctrl, hero, desired, dt)?;
    if m.grounded {
        *fall = 0.0;
    }
    Ok(m.grounded)
}

fn main() {
    let mut world = World3D::default();
    let floor = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(20.0, 0.5, 20.0)))
        .with_offset(yowie::Transform::from_xyz(0.0, -0.5, 0.0));
    world.add_static_collider(&floor).unwrap();

    let capsule = Shape3D::capsule(0.45, 0.35);
    let hero = world.add_body(&BodyDesc3D::kinematic().at(Vec3::new(0.0, 1.5, 0.0)));
    world
        .add_collider(hero, &ColliderDesc3D::new(capsule.clone()))
        .unwrap();
    let ctrl = CharacterController3D::new(capsule);

    // The controller queries the floor before the first step.
    world.refresh_queries();
    let mut fall = 0.0;
    let mut grounded = false;
    for _ in 0..60 {
        grounded = walk(&mut world, &ctrl, hero, Vec2::X, &mut fall, 1.0 / 60.0).unwrap();
        world.step(1.0 / 60.0);
    }
    assert!(grounded);
    assert!(world.position(hero).unwrap().x > 3.0);
}
```

The controller reads the query structures, so in `init` call
`refresh_queries` after adding the level and before the first move, as
`physics_lab` does. To ask what a move would do without a body, call
`move_character(&controller, position, rotation, desired, dt, exclude)`;
it changes nothing.

## Debug drawing

To see colliders, joints and contact points, draw the world's debug
lines. Sleeping bodies are drawn darker.

In 2D, call `World2D::debug_draw_to(gfx)` inside the camera the world is
drawn through, so the lines line up with the sprites. Lines are one
output pixel thick at any zoom.

```rust,no_run
use yowie::gfx::{Camera2D, Graphics};
use yowie::phys::World2D;

/// Draws the world's colliders over whatever was drawn with `camera`.
fn draw_debug(gfx: &mut Graphics, camera: &Camera2D, world: &mut World2D) {
    gfx.with_camera(camera, |g| world.debug_draw_to(g));
}
```

In 3D, pass a `yowie::SceneDebugDraw` to `World3D::debug_draw` inside
`Graphics::scene`. Lines are hidden behind geometry unless you set its
`on_top` field. `World3D::set_debug_contacts(true)` adds contact points
and normals:

```rust,no_run
use yowie::SceneDebugDraw;
use yowie::gfx::Graphics;
use yowie::gfx::scene::Camera3D;
use yowie::phys::World3D;

/// Draws the world's colliders and contacts over the scene.
fn draw_debug_3d(gfx: &mut Graphics, camera: &Camera3D, world: &mut World3D) {
    world.set_debug_contacts(true);
    gfx.scene(camera, |s| world.debug_draw(&mut SceneDebugDraw::new(s)));
}
```

`debug_draw` takes any `DebugDraw`, a trait with `line(a, b, color)` and
`point(p, color)` in world space (2D worlds pass `z = 0`). Implement it to
send the lines elsewhere, such as a log or a test:

```rust
use yowie::Color;
use yowie::Vec3;
use yowie::phys::{ColliderDesc3D, DebugDraw, Shape3D, World3D};

/// Counts debug lines instead of drawing them.
struct LineCount(usize);

impl DebugDraw for LineCount {
    fn line(&mut self, _a: Vec3, _b: Vec3, _color: Color) {
        self.0 += 1;
    }

    fn point(&mut self, _p: Vec3, _color: Color) {}
}

fn main() {
    let mut world = World3D::default();
    world
        .add_static_collider(&ColliderDesc3D::new(Shape3D::sphere(1.0)))
        .unwrap();
    let mut count = LineCount(0);
    world.debug_draw(&mut count);
    assert!(count.0 > 0);
}
```

With the debug console on (`Config::console`), call
`ctx.console.feed_physics_2d("world", &world)` or `feed_physics_3d` each
update to show the world's body and collider counts in the Physics panel.
See [the debug console](console.md).

## Cloth, soft bodies and fluids

`yowie::phys::soft` simulates objects made of particles with extended
position-based dynamics (XPBD): `Cloth`, `SoftBody` (a closed volume
that squashes and springs back) and `Fluid2D`. Each `step` splits the
time into substeps, moves the particles, satisfies the constraints
between them, pushes them out of colliders, and derives the velocities
from how far each particle moved. Softness is given as compliance in
metres per newton: 0 is rigid and larger values are softer.

Soft objects collide with the static and kinematic colliders of a world:
cloth and soft bodies with a `World3D`, fluids with a `World2D`. They
read the colliders through the query structures, so step the world (or
call `refresh_queries`) before stepping them. They do not collide with
dynamic bodies, they never push a rigid body, and sensors are ignored.
Pass `&()` instead of a world to simulate with no colliders.

### Settings

`Cloth::step` and `SoftBody::step` take a `SoftSettings`:

| Field | Default | Meaning |
|---|---|---|
| `gravity` | (0, -9.81, 0) | Acceleration on every particle |
| `substeps` | 4 | Substeps per `step`; more are stiffer and steadier, at a proportional cost |
| `iterations` | 4 | Constraint passes per substep |
| `ground` | `None` | An optional floor plane, `Ground { y, friction }`, for scenes with no collider below |

### Cloth

To make a cloth, fill a `ClothDesc` and call `Cloth::new`. The cloth is
a grid of `cols` by `rows` particles. Particle `(col, row)` has index
`row * cols + col` and starts at
`origin + u * col * spacing + v * row * spacing`. The defaults hang the
cloth down from `origin`.

| Field | Default | Meaning |
|---|---|---|
| `cols`, `rows` | 10, 10 | Particles across and down; at least 2 each |
| `spacing` | 0.1 | Rest distance between neighbours |
| `mass` | 1 | Total mass, shared equally |
| `origin`, `u`, `v` | origin, +X, -Y | Where particle 0 starts and the directions of increasing column and row |
| `pinned` | none | Indices of particles that never move on their own |
| `stretch_compliance` | 0 | Softness of the edges between neighbours; 0 does not stretch |
| `shear_compliance` | 1e-6 | Softness of the diagonals |
| `bend_compliance` | 1e-3 | Softness against folding |
| `damping` | 0.05 | Fraction of velocity lost per second |
| `radius`, `friction` | 0.02, 0.5 | Collision radius of each particle and friction against colliders |
| `wind` | none | `Wind { velocity, drag }` |
| `layers` | all | Which colliders it touches |

```rust
use yowie::Vec3;
use yowie::phys::soft::{Cloth, ClothDesc, SoftSettings, Wind};

fn main() {
    // A 20 by 12 flag pinned along its left edge.
    let (cols, rows) = (20, 12);
    let desc = ClothDesc {
        cols,
        rows,
        spacing: 0.1,
        origin: Vec3::new(0.0, 3.0, 0.0),
        pinned: (0..rows).map(|row| row * cols).collect(),
        bend_compliance: 0.02,
        damping: 0.4,
        ..ClothDesc::default()
    };
    let mut flag = Cloth::new(&desc).unwrap();
    flag.set_wind(Wind {
        velocity: Vec3::new(0.0, 0.0, 6.0),
        drag: 1.5,
    });

    let settings = SoftSettings::default();
    for _ in 0..120 {
        flag.step(1.0 / 60.0, &settings, &());
    }
    // The pinned edge stays put while the rest hangs and blows.
    assert_eq!(flag.positions()[0], Vec3::new(0.0, 3.0, 0.0));
    assert!(flag.max_strain() < 0.2);
}
```

While it runs, `pin(i)` and `free(i)` pin and release particles,
`move_pinned(i, p)` drags a pinned particle (the rest follows on the next
step), `translate(delta)` moves the whole cloth, and `max_strain()`
reports the largest stretch of any edge (0.1 is 10% longer than at rest).

### Drawing a soft object

A soft object's particles are its mesh vertices, already in world space.
To draw one, keep a `SurfaceMesh` and a renderer `MeshData` beside it:

1. Call `cloth.mesh()` (or `soft_body.mesh()`) once for a `SurfaceMesh`,
   then `to_mesh_data()` for `MeshData`, and create a `Mesh` from it.
2. After each step, call `update_mesh(&mut surface)` to copy the
   positions and recompute the normals, then
   `surface.write_to(&mut data)`, which keeps the colours and texture
   coordinates you set.
3. In `draw`, call `mesh.update(&ctx.gfx, &data)` and draw the mesh with
   `Transform::IDENTITY`. Give a cloth a `double_sided` material so both
   faces show.

```rust,no_run
use yowie::gfx::Graphics;
use yowie::gfx::scene::{Material, Mesh, MeshData, Scene};
use yowie::phys::soft::{Cloth, SoftSettings, SurfaceMesh};
use yowie::{Color, GameResult, Transform};

/// A cloth and the geometry that follows it.
struct Flag {
    cloth: Cloth,
    surface: SurfaceMesh,
    data: MeshData,
    mesh: Mesh,
}

impl Flag {
    fn new(gfx: &Graphics, cloth: Cloth) -> GameResult<Self> {
        let surface = cloth.mesh();
        let data = surface.to_mesh_data();
        let mesh = Mesh::new(gfx, &data)?;
        Ok(Flag { cloth, surface, data, mesh })
    }

    /// Call from `update`.
    fn step(&mut self, dt: f32, settings: &SoftSettings) {
        self.cloth.step(dt, settings, &());
        self.cloth.update_mesh(&mut self.surface);
        self.surface.write_to(&mut self.data);
    }

    /// Call from `draw`, inside `Graphics::scene`, after `upload`.
    fn draw(&self, scene: &mut Scene) {
        let cloth = Material {
            double_sided: true,
            ..Material::color(Color::WHITE, 0.0, 0.85)
        };
        scene.draw(&self.mesh, &cloth, Transform::IDENTITY);
    }

    /// Call from `draw`, before `Graphics::scene`.
    fn upload(&self, gfx: &Graphics) -> GameResult {
        self.mesh.update(gfx, &self.data)?;
        Ok(())
    }
}
```

### Soft bodies

A `SoftBody` is a volume of tetrahedra that keeps its volume and springs
back. `SoftBody::lattice(centre, size, cells, &desc)` builds a box split
into `cells[0] * cells[1] * cells[2]` cubes of six tetrahedra each.
`SoftBody::from_tets(vertices, tets, &desc)` builds one from your own
tetrahedral mesh. `SoftBodyDesc` sets the material:

| Field | Default | Meaning |
|---|---|---|
| `mass` | 1 | Total mass |
| `edge_compliance` | 1e-5 | Softness of the edges |
| `volume_compliance` | 0 | Softness of each tetrahedron's volume |
| `pressure_compliance` | `Some(1e-4)` | Softness of the whole-body volume; `None` turns it off and lets the body deflate |
| `pressure_scale` | 1 | Target volume as a multiple of the rest volume; above 1 inflates |
| `shape_matching` | 0 | 0 to 1; 0 is off, and values near 1 pull the body back to its rest shape quickly |
| `damping` | 0.2 | Fraction of velocity lost per second |
| `radius`, `friction`, `wind`, `layers` | 0.02, 0.5, none, all | As for cloth |

```rust
use yowie::phys::soft::{Ground, SoftBody, SoftBodyDesc, SoftSettings};
use yowie::Vec3;

fn main() {
    let desc = SoftBodyDesc {
        mass: 3.0,
        edge_compliance: 1.0e-4,
        shape_matching: 0.04,
        damping: 0.6,
        ..SoftBodyDesc::default()
    };
    let mut jelly =
        SoftBody::lattice(Vec3::new(0.0, 2.0, 0.0), Vec3::splat(1.0), [4, 4, 4], &desc).unwrap();
    let settings = SoftSettings {
        ground: Some(Ground::default()),
        ..SoftSettings::default()
    };
    for _ in 0..120 {
        jelly.step(1.0 / 60.0, &settings, &());
    }
    // It landed on the floor at y = 0 and kept most of its volume.
    let lowest = jelly.positions().iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(lowest > -0.05 && lowest < 0.1);
    assert!(jelly.volume() / jelly.rest_volume() > 0.8);
}
```

To kick a soft body, call `add_velocity(dv)`; it adds to every free
particle. `pin`, `free`, `translate` and `set_wind` work as on a cloth,
and `surface()` lists the outer triangles.

### 2D fluids

A `Fluid2D` is liquid in the plane, simulated as particles. To make one,
call `Fluid2D::new` with a `FluidDesc2D`, add particles with
`fill(area)` (a square lattice at the fluid's spacing) or `add(point)`,
and call `step(dt, gravity, &world)` each update. Gravity is a parameter,
so the fluid works in a Y-up world or in Y-down view units. It collides
with the static and kinematic colliders of a `World2D`.

| Field | Default | Meaning |
|---|---|---|
| `bounds` | `None` | A `Rect` tank that keeps the particles in |
| `spacing` | 0.05 | Distance between particles at rest; sets the density the fluid settles at |
| `radius` | 0 (2.5 times `spacing`) | How far a particle feels its neighbours |
| `substeps` | 1 | Solves per `step`; raise it for fast, thin flows that must not cross walls |
| `iterations` | 4 | Density passes per solve |
| `viscosity` | 15 | 0 churns freely, 15 is water, 60 is syrup |
| `surface_tension` | 0.02 | The close-range push that rounds off drops |
| `relaxation` | 0.05 | Softens the density solve for sparse particles |
| `damping` | 0.1 | Fraction of velocity lost per second |
| `friction` | 0.1 | Against the tank and colliders |
| `layers` | all | Which colliders it touches |

The `softbody` example runs its tank in view pixels, with a post in the
middle as a static 2D collider:

```rust
use yowie::phys::soft::{Fluid2D, FluidDesc2D};
use yowie::phys::{ColliderDesc2D, Shape2D, World2D};
use yowie::{Rect, Transform2D, vec2};

fn main() {
    let tank = Rect::new(0.0, 0.0, 280.0, 300.0);
    let mut posts = World2D::new(vec2(0.0, 0.0));
    let post = ColliderDesc2D::new(Shape2D::circle(34.0))
        .with_offset(Transform2D::from_position(vec2(140.0, 210.0)));
    posts.add_static_collider(&post).unwrap();
    // The fluid reads the post through the query structures.
    posts.refresh_queries();

    let desc = FluidDesc2D {
        bounds: Some(tank),
        spacing: 7.0,
        substeps: 2,
        ..FluidDesc2D::default()
    };
    let mut water = Fluid2D::new(&desc).unwrap();
    water.fill(Rect::new(8.0, 8.0, 132.0, 284.0));
    let count = water.count();

    // Gravity in pixels per second squared, down the screen.
    for _ in 0..60 {
        water.step(1.0 / 60.0, vec2(0.0, 900.0), &posts);
    }
    assert_eq!(water.count(), count);
    assert!(water.positions().iter().all(|p| tank.contains(*p)));
}
```

To draw the fluid, draw a disc at each of `positions()`. `density(i)`
divided by `rest_density()` tells how packed a particle is, which the
example uses to shade deep water darker than spray. `clear()` empties
the fluid.

## Where to go next

- [Orbits and space](space.md) for gravity between planets and ships.
- [3D graphics](graphics-3d.md) for meshes, materials and the scene the
  bodies are drawn into.
- [2D graphics](graphics-2d.md) for cameras and shapes.
- [Entities and systems](ecs.md) to keep body ids on entities.
