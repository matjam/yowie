---
title: Entities and systems
group: Engine
order: 3
summary: the entity component system in yowie::ecs, from queries and systems to hierarchy, saves, prefabs and scene documents, how it stores data, and how to structure a game around it
---

# Entities and systems

To keep game state as entities with components, create a
`yowie::ecs::World`. A `World` wraps a `hecs::World` and dereferences to
it, so `spawn`, `get`, `query`, `query_mut`, `insert` and `remove` work as
hecs documents them. Yowie adds the parts a game needs on top:

- Resources, one value per type, and double-buffered events.
- `Commands`, which queue world changes made while a query borrows it.
- `Schedule`, `System` and `Stage`: named systems in ordered stages, with
  ordering constraints, run conditions and per-system timing.
- A hierarchy of parents and children, and transform propagation.
- A component registry that drives saving and loading, cloning, prefabs
  and scene documents.

The engine does not own a world. A game keeps its `World` and `Schedule` in
its `Game` struct and runs the schedule from `update`. The last part of this
guide shows the whole arrangement.

## Entities and components

To create an entity, call `world.spawn` with a tuple of components. A
component is any `Send + Sync + 'static` type; no derive or registration is
needed to store one. `spawn` returns an `Entity`, a generational handle that
stays unique after the entity is despawned.

```rust
use yowie::ecs::World;
use yowie::{Transform2D, vec2};

#[derive(Debug, PartialEq)]
struct Health(i32);
struct Player;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::new();
    let hero = world.spawn((Player, Transform2D::from_position(vec2(10.0, 20.0)), Health(3)));
    let chest = world.spawn((Transform2D::IDENTITY,));

    // Read and change one entity's components.
    world.get::<&mut Health>(hero)?.0 -= 1;
    assert_eq!(*world.get::<&Health>(hero)?, Health(2));

    // Add and remove components.
    world.insert_one(chest, Health(1))?;
    world.remove_one::<Health>(chest)?;

    world.despawn(chest)?;
    assert!(!world.contains(chest));
    assert_eq!(world.len(), 1);
    Ok(())
}
```

| Call | What it does |
|---|---|
| `spawn(bundle)` | Creates an entity with the tuple's components. |
| `get::<&T>(e)`, `get::<&mut T>(e)` | Borrows one component; an error when the entity or component is missing. |
| `insert(e, bundle)`, `insert_one(e, c)` | Adds components, replacing ones of the same type. |
| `remove::<(A, B)>(e)`, `remove_one::<T>(e)` | Removes components and returns them. |
| `despawn(e)` | Despawns the entity and all its descendants, and removes it from its parent's children. |
| `contains(e)` | Whether the entity exists. |
| `len()` | The number of live entities. |
| `clear()` | Despawns every entity. Resources and events stay. |

`World::despawn` replaces the hecs method of the same name: it is
recursive over the hierarchy (below). Tag types such as `Player` cost
nothing to store and are matched by queries like any other component.

## Queries

To visit every entity with a set of components, query the world. With
`&mut World`, use `query_mut`, which returns an iterator directly. With
`&World`, use `query` and call `iter` on the result. Add `Entity` to the
query tuple to get each entity's handle, and `Option<&T>` for a component
that may be missing.

```rust
use yowie::ecs::{Entity, World};
use yowie::{Vec2, vec2};

struct Position(Vec2);
struct Velocity(Vec2);
struct Frozen;

let mut world = World::new();
world.spawn((Position(Vec2::ZERO), Velocity(vec2(1.0, 0.0))));
world.spawn((Position(Vec2::ZERO), Velocity(vec2(0.0, 1.0)), Frozen));

// Exclusive access: no runtime borrow checks.
for (pos, vel) in world.query_mut::<(&mut Position, &Velocity)>().without::<&Frozen>() {
    pos.0 += vel.0;
}

// Shared access, with each entity's handle.
let moved: Vec<Entity> = world
    .query::<(Entity, &Position)>()
    .iter()
    .filter(|(_, p)| p.0 != Vec2::ZERO)
    .map(|(e, _)| e)
    .collect();
assert_eq!(moved.len(), 1);
```

`with::<&T>()` and `without::<&T>()` narrow a query to entities that have,
or lack, a component without borrowing it. `query_one_mut::<Q>(e)` runs a
query on one entity.

`query` checks borrows at run time. Iterating a query that borrows a
component type mutably while another live query borrows the same type, or
the reverse, panics. Use `query_mut` where you hold `&mut World`, and finish
one query before starting a conflicting one.

Entities come out of a query in no particular order.

## Changing the world while iterating

A query borrows the world, so the loop body cannot spawn, despawn, insert or
remove. Use one of two patterns:

- Collect what to change into a `Vec`, end the query, then change the world.
- Record the changes in the world's `Commands` queue and apply it after the
  query.

```rust
use yowie::ecs::{Entity, World};

struct Health(i32);
struct Corpse;

fn reap(world: &mut World) {
    // Collect, then change.
    let dead: Vec<Entity> = world
        .query::<(Entity, &Health)>()
        .iter()
        .filter(|(_, h)| h.0 <= 0)
        .map(|(e, _)| e)
        .collect();
    for e in dead {
        let _ = world.remove_one::<Health>(e);
        let _ = world.insert_one(e, Corpse);
    }
}

fn clean_up(world: &mut World) {
    // Record commands while the query runs.
    {
        let mut commands = world.commands();
        for (e, _) in world.query::<(Entity, &Corpse)>().iter() {
            commands.despawn(e);
        }
    }
    world.flush_commands();
}

let mut world = World::new();
world.spawn((Health(0),));
world.spawn((Health(5),));
reap(&mut world);
clean_up(&mut world);
assert_eq!(world.len(), 1);
```

`world.commands()` takes `&self`, so it works while a query holds the world.
The `Schedule` applies the queue after every system, so inside a system you
only record commands. Outside a schedule, call `world.flush_commands()`.
Commands queued while a flush runs wait for the next flush.

| `Commands` call | Queues |
|---|---|
| `spawn(&world, bundle)` | A spawn. Returns the entity's handle now, reserved, so later commands can use it. The entity exists for queries after the flush. |
| `despawn(e)` | A recursive despawn. A missing entity is skipped. |
| `insert(e, bundle)` | Adding components. |
| `remove::<B>(e)` | Removing components. |
| `set_parent(child, parent)` | `World::set_parent`; errors are logged. |
| `run(closure)` | Any change, as `FnOnce(&mut World)`. |

A separate `Commands::new()` queue works the same way; apply it with
`commands.apply(&mut world)`.

## Systems and the schedule

A system is a function or closure of `&mut World`. To run systems in order
every update, add them to a `Schedule` under a stage and a name, then call
`schedule.run(&mut world)` once per update.

| `Stage` | Runs |
|---|---|
| `Startup` | Once, during the first `run`. |
| `PreUpdate` | Every update, first. Input handling. |
| `Update` | Every update. Gameplay. |
| `PostUpdate` | Every update, last. Transform propagation and clean-up. |

Within a stage, systems run in the order they were added unless constraints
say otherwise. `System::new(name, f)` builds a system, and its builder
methods add constraints and conditions:

| Method | Effect |
|---|---|
| `before(target)` | Runs before the system or set named `target`, within the same stage. |
| `after(target)` | Runs after it. |
| `in_set(label)` | Puts the system in a set, so others can run before or after the whole set. |
| `run_if(condition)` | Runs only when the condition returns `true`. Several calls add several conditions, and all must pass. They are checked each time the schedule reaches the system. |

`add_system(stage, name, f)` adds a system with no constraints;
`add(stage, system)` adds a built one. The run conditions provided are
`resource_exists::<T>()`, `has_events::<T>()` and `not(condition)`; any
`FnMut(&World) -> bool` works.

```rust
use yowie::ecs::{Schedule, Stage, System, World, not, resource_exists};

struct Dt(f32);
struct Paused;
struct Speed(f32);
struct Distance(f32);

fn drive(world: &mut World) {
    let dt = world.resource::<Dt>().map_or(0.0, |d| d.0);
    for (speed, distance) in world.query_mut::<(&Speed, &mut Distance)>() {
        distance.0 += speed.0 * dt;
    }
}

fn friction(world: &mut World) {
    for speed in world.query_mut::<&mut Speed>() {
        speed.0 *= 0.5;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut schedule = Schedule::new();
    // Added first, but ordered after "drive".
    schedule.add(Stage::Update, System::new("friction", friction).after("drive"));
    schedule.add(
        Stage::Update,
        System::new("drive", drive).run_if(not(resource_exists::<Paused>())),
    );
    schedule.validate()?;
    assert_eq!(schedule.system_order(Stage::Update), ["drive", "friction"]);

    let mut world = World::new();
    world.insert_resource(Dt(1.0));
    let car = world.spawn((Speed(4.0), Distance(0.0)));
    schedule.run(&mut world);
    assert_eq!(world.get::<&Distance>(car)?.0, 4.0);
    assert_eq!(world.get::<&Speed>(car)?.0, 2.0);

    world.insert_resource(Paused);
    schedule.run(&mut world);
    assert_eq!(world.get::<&Distance>(car)?.0, 4.0);
    Ok(())
}
```

One `run` does this:

1. On the first call, the `Startup` stage.
2. `PreUpdate`, `Update` and `PostUpdate`, each system followed by a flush
   of the world's command queue, so the next system sees its changes.
3. `world.update_events()`, which ages the event queues (below).

The schedule sorts a stage when systems were added since the last sort. To
catch ordering mistakes at start-up, call `schedule.validate()` after adding
systems. It returns `Error::UnknownOrderTarget` when a `before` or `after`
names no system or set, and `Error::OrderCycle` when constraints in a stage
form a loop. Without `validate`, `run` logs the error and runs the stage in
insertion order.

A system can be a closure that owns state, such as a sound to play or a
counter: `System::new("effects", move |world| { ... })`. Names identify
systems in timings and in `set_enabled`; keep them unique.

| `Schedule` call | Returns or does |
|---|---|
| `set_enabled(name, on)` | Turns every system of that name on or off. Returns `false` when none has it. |
| `stats()` | A `SystemStat` per system in run order: stage, enabled, last, longest and total run time, runs and skips. |
| `system_order(stage)` | The names of a stage's systems in run order. |
| `updates()` | How many times `run` has completed. |

## Resources

To share one value between systems, store it as a resource. A world holds
at most one value of each type, so give each resource its own type.
Resources need only `'static`, not `Send` or `Sync`.

| Call | What it does |
|---|---|
| `insert_resource(value)` | Stores the value, returning the previous one of that type. |
| `resource::<T>()`, `resource_mut::<T>()` | Borrows it, or `None`. |
| `remove_resource::<T>()` | Takes it out. |
| `has_resource::<T>()` | Whether one is stored. |

Systems take everything from the world, so the game writes its inputs as
resources before running the schedule: the update's time step, the player's
controls, the time scale. `yowie::ecs_glue::set_delta(&mut world, ctx)`
stores `ctx.time.delta()` as a `DeltaTime` resource.

## Events

To tell other systems that something happened, send an event:
`world.send_event(value)`. Any `'static` type is an event. To read events of
a type, call `world.read_events::<T>()`, which yields the events sent in the
previous update first, then those sent in this one.

An event lives for two updates. The `Schedule` calls
`world.update_events()` after its last stage, which drops the events sent two
updates ago and makes this update's events the previous update's. So:

- A reader that runs before the sender in the same update still sees the
  event, in the next update.
- A reader that runs every update sees each event twice: in the update
  that sent it, if it runs after the sender, and in the next.

To consume events exactly once, read them and then clear them with
`world.clear_events::<T>()`:

```rust
use yowie::ecs::{Schedule, Stage, World};

#[derive(Clone)]
struct LinesCleared(u32);
struct Score(u32);

fn clear_lines(world: &mut World) {
    world.send_event(LinesCleared(2));
}

fn tally(world: &mut World) {
    let gained: u32 = world.read_events::<LinesCleared>().map(|e| e.0 * 100).sum();
    world.clear_events::<LinesCleared>();
    if let Some(score) = world.resource_mut::<Score>() {
        score.0 += gained;
    }
}

let mut schedule = Schedule::new();
schedule.add_system(Stage::Update, "clear-lines", clear_lines);
schedule.add_system(Stage::Update, "tally", tally);

let mut world = World::new();
world.insert_resource(Score(0));
schedule.run(&mut world);
schedule.run(&mut world);
assert_eq!(world.resource::<Score>().map(|s| s.0), Some(400));
```

Without the `clear_events` call the second update would also count the first
update's event, and the score would be 600. Clearing suits one consumer per
event type. With several readers, have each skip what it has handled, or
send a separate event type per reader.

`world.events::<T>()` returns the `Events<T>` queue itself, with `len`,
`is_empty` and `iter`, or `None` if no event of the type was ever sent. When
you drive systems without a `Schedule`, call `world.update_events()` once per
update yourself.

## Hierarchy

To attach one entity to another, call `world.set_parent(child, parent)`.
The child becomes the parent's last child, after being detached from any
previous parent. The call fails with `Error::NoSuchEntity` when either
entity is missing and `Error::HierarchyCycle` when the parent is the child
or one of its descendants.

| Call | What it does |
|---|---|
| `set_parent(child, parent)` | Attaches the child. |
| `detach(child)` | Makes the child a root again. |
| `parent(e)` | The parent, if any. |
| `children(e)` | The children in attach order. |
| `descendants(e)` | Every descendant, parents before children. |
| `despawn(e)` | Despawns the entity and every descendant. |

The links are stored as `Parent` and `Children` components. They are
maintained by these calls; do not insert or edit them yourself.

## Transforms

Give an entity a `Transform` (3D: translation, rotation, scale) or a
`Transform2D` (position, rotation in radians, scale) for its place relative
to its parent. To compute world-space placements, run
`propagate_transforms`, usually as a `PostUpdate` system. It writes a
`GlobalTransform` (a `Mat4`) for each entity with a `Transform` and a
`GlobalTransform2D` (an `Affine2`) for each entity with a `Transform2D`.

```rust
use yowie::ecs::{GlobalTransform2D, World, propagate_transforms};
use yowie::{Transform2D, Vec2, vec2};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::new();
    let ship = world.spawn((Transform2D::from_position(vec2(100.0, 50.0)),));
    let turret = world.spawn((Transform2D::from_position(vec2(10.0, 0.0)),));
    world.set_parent(turret, ship)?;

    propagate_transforms(&mut world);
    let at = world.get::<&GlobalTransform2D>(turret)?.0.transform_point2(Vec2::ZERO);
    assert_eq!(at, vec2(110.0, 50.0));
    Ok(())
}
```

A root's global transform equals its local one; a child's is its parent's
global transform times its own local one. An entity in the middle of a chain
without a local transform of that kind passes its parent's global transform
through unchanged and gets no global component. 2D and 3D chains are
independent. Draw from the global components, and never save or edit them:
propagation rebuilds them.

## Plugging into the Game loop

Put the world and its schedule in the `Game` struct. Each update, write the
inputs as resources, run the schedule, and hand its timings to the engine.
Each draw, read the world with queries.

```rust,no_run
use yowie::ecs::{GlobalTransform2D, Schedule, Stage, World, propagate_transforms};
use yowie::ecs_glue::{DeltaTime, set_delta};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Transform2D, Vec2, vec2};

struct Velocity(Vec2);
struct Ball(Color);
struct Player;

/// The player's controls this update, written by the game before the
/// schedule runs.
#[derive(Default)]
struct Controls {
    thrust: Vec2,
}

fn steer(world: &mut World) {
    let thrust = world.resource::<Controls>().map_or(Vec2::ZERO, |c| c.thrust);
    for (v, _) in world.query_mut::<(&mut Velocity, &Player)>() {
        v.0 += thrust * 20.0;
    }
}

fn movement(world: &mut World) {
    let dt = world.resource::<DeltaTime>().map_or(0.0, |d| d.0);
    for (t, v) in world.query_mut::<(&mut Transform2D, &Velocity)>() {
        t.position += v.0 * dt;
    }
}

struct Arena {
    world: World,
    schedule: Schedule,
}

impl Game for Arena {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        let mut world = World::new();
        world.insert_resource(Controls::default());
        world.spawn((
            Player,
            Ball(Color::ORANGE),
            Velocity(Vec2::ZERO),
            Transform2D::from_position(vec2(400.0, 300.0)),
        ));

        let mut schedule = Schedule::new();
        schedule.add_system(Stage::PreUpdate, "steer", steer);
        schedule.add_system(Stage::Update, "movement", movement);
        schedule.add_system(Stage::PostUpdate, "transforms", propagate_transforms);
        schedule.validate()?;
        Ok(Arena { world, schedule })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let key = |k| if ctx.input.key_down(k) { 1.0 } else { 0.0 };
        let thrust = vec2(key(Key::D) - key(Key::A), key(Key::S) - key(Key::W));
        if let Some(c) = self.world.resource_mut::<Controls>() {
            c.thrust = thrust;
        }
        set_delta(&mut self.world, ctx);
        self.schedule.run(&mut self.world);
        ctx.console.feed_world("arena", &self.world, Some(&mut self.schedule));
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        for (g, ball) in self.world.query::<(&GlobalTransform2D, &Ball)>().iter() {
            ctx.gfx.fill_circle(g.0.translation, 12.0, ball.0);
        }
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config { console: true, ..Config::new("Arena") };
    yowie::run::<Arena>(config)
}
```

The arrangement has these properties:

- The schedule runs once per update, so in the fixed-step loop every system
  sees the same `DeltaTime`, and the game's simulation does not depend on
  the frame rate.
- Pausing and the time scale (`ctx.time.set_paused`, `set_scale`) change
  how many updates run, so the systems need no code for them.
- Systems never touch the `Context`. They read resources and write
  components, resources and events, so they can be tested on a bare world.
- `draw` reads the world and does not change it.

`ctx.console.feed_world` shows the world in the debug console's Entities
panel: its entities, its systems with their run times, and a switch per
system that `feed_world` applies to the schedule. When you only want the
timings, call `ctx.record_schedule(&schedule)` after `run` instead. See
[The debug console](console.md#the-panels).

To run animation players as a system, add `yowie::ecs_glue::animate` (or
call `add_animation(&mut schedule)`). It advances every `Player` component
by `DeltaTime`, writes the pose into the entity's `Transform` or
`Transform2D`, and sends `AnimationEvent`s for markers and finished clips.
See [Animation](animation.md).

## Saving and loading a world

Saves, cloning, prefabs and scene documents work on registered component
types. To register one, call `world.register::<T>(name)`. The type needs
serde's `Serialize` and `Deserialize`, and `Clone`. The name is what
documents use, so keep it stable across versions of the game. Every new
world has `Transform`, `Transform2D` and `Name` registered.

To write a world as JSON, call `world.save(writer, &SaveOptions)` or
`world.save_string(&SaveOptions)`. To read one, call `world.load(reader,
&LoadOptions)` or `world.load_str(json, &LoadOptions)`.

```rust
use serde::{Deserialize, Serialize};
use yowie::ecs::{LoadOptions, SaveOptions, World};
use yowie::{Transform2D, vec2};

#[derive(Clone, Serialize, Deserialize)]
struct Health(i32);

#[derive(Serialize, Deserialize)]
struct Level(u32);

fn main() -> Result<(), yowie::ecs::Error> {
    let mut world = World::new();
    world.register::<Health>("health");
    world.register_resource::<Level>("level");
    world.insert_resource(Level(3));
    let hero = world.spawn((Transform2D::from_position(vec2(4.0, 2.0)), Health(7)));
    let json = world.save_string(&SaveOptions::default())?;

    let mut restored = World::new();
    restored.register::<Health>("health");
    restored.register_resource::<Level>("level");
    let map = restored.load_str(&json, &LoadOptions::default())?;
    let hero2 = map.get(hero);
    assert_eq!(restored.get::<&Health>(hero2).map(|h| h.0), Ok(7));
    assert_eq!(restored.resource::<Level>().map(|l| l.0), Some(3));
    Ok(())
}
```

A save holds every entity under its handle number, its registered
components keyed by name, each entity's children, and every registered
resource. `Parent`, `Children` and the global transforms are not written:
the child lists rebuild the hierarchy, and `propagate_transforms` rebuilds
the global transforms.

`load` adds to the world. Every loaded entity gets a new handle, and the
returned `EntityMap` translates the document's handles to the new ones
(`map.get(old)`). Existing entities are untouched, and loaded resources
replace existing ones. When loading fails, the entities it spawned are
removed again.

| Option | Default | Effect |
|---|---|---|
| `SaveOptions::skip_unregistered` | `false` | Leaves out unregistered components instead of failing with `Error::UnregisteredComponents`. |
| `SaveOptions::pretty` | `false` | Writes indented JSON. |
| `LoadOptions::skip_unknown` | `false` | Ignores unregistered component and resource names instead of failing with `Error::UnknownComponent` or `Error::UnknownResource`. |

A component that is derived data, such as a cached path or a GPU handle,
should not be saved. Mark its type with `world.ignore_component::<T>()`, and
saves and clones skip it without complaint. A document from a newer format
version fails with `Error::UnsupportedVersion`.

To store the save in the player's data directory, write it to a slot of
`yowie::save::Store`; see [Game services](services.md#saves-and-settings).

## Components that refer to entities

An `Entity` inside a component is only meaningful in the world it came
from. To keep such references correct across loading, cloning and
instantiating, implement `MapEntities` for the component and register it
with `register_mapped` instead of `register`. For the serde side, use the
`entity_serde` adapters on `Entity` fields:

```rust
use serde::{Deserialize, Serialize};
use yowie::ecs::{Entity, EntityMap, MapEntities, World};

#[derive(Clone, Serialize, Deserialize)]
struct Follow {
    #[serde(with = "yowie::ecs::entity_serde")]
    target: Entity,
    #[serde(with = "yowie::ecs::entity_serde::option", default)]
    leader: Option<Entity>,
}

impl MapEntities for Follow {
    fn map_entities(&mut self, map: &EntityMap) {
        self.target = map.get(self.target);
        self.leader = self.leader.map(|e| map.get(e));
    }
}

fn main() -> Result<(), yowie::ecs::Error> {
    let mut world = World::new();
    world.register_mapped::<Follow>("follow");

    let pack = world.spawn(());
    let wolf = world.spawn(());
    let cub = world.spawn((Follow { target: wolf, leader: None },));
    world.set_parent(wolf, pack)?;
    world.set_parent(cub, pack)?;

    // A copy of the pack: the cub's copy follows the wolf's copy.
    let copy = world.clone_tree(pack)?;
    let kids = world.children(copy);
    let target = world.get::<&Follow>(kids[1]).map(|f| f.target);
    assert_eq!(target, Ok(kids[0]));
    Ok(())
}
```

For an entity it does not contain, an `EntityMap` follows its `Unmapped`
setting. Cloning uses `Unmapped::Keep`, which leaves the reference as it
is. Loading uses `Unmapped::Dangling`, which turns it into
`Entity::DANGLING`.

## Cloning

| Call | What it does |
|---|---|
| `clone_entity(e)` | Copies the entity's registered components onto a new entity with no parent and no children. References inside them keep pointing at the originals. |
| `clone_tree(e)` | Copies the entity and all its descendants, with the same child order. References between entities of the subtree point at the copies; references outside it are kept. The copy of `e` has no parent. |

Components that are not registered are left out, and a warning is logged
for each one that is not marked with `ignore_component`.

## Prefabs

A `Prefab` is a JSON template for an entity and its descendants, spawned as
many times as needed. Its components are keyed by registered name. Its
entities are numbered from 1 in depth-first order, the root being 1, and an
entity reference inside a component value is one of those numbers.

```rust
use serde::{Deserialize, Serialize};
use yowie::ecs::{Overrides, Prefab, World};

#[derive(Clone, Serialize, Deserialize)]
struct Health(i32);

const GOBLIN: &str = r#"{
  "components": {
    "Transform2D": { "position": [0, 0], "rotation": 0, "scale": [1, 1] },
    "health": 3
  },
  "children": [
    { "components": { "Name": "spear",
        "Transform2D": { "position": [8, 0], "rotation": 0, "scale": [1, 1] } } }
  ]
}"#;

fn main() -> Result<(), yowie::ecs::Error> {
    let mut world = World::new();
    world.register::<Health>("health");
    let goblin = Prefab::from_json(GOBLIN)?;

    let grunt = world.spawn_prefab(&goblin, &Overrides::new())?;
    let chief = world.spawn_prefab(&goblin, &Overrides::new().set("health", &Health(12))?)?;

    assert_eq!(world.get::<&Health>(grunt).map(|h| h.0), Ok(3));
    assert_eq!(world.get::<&Health>(chief).map(|h| h.0), Ok(12));
    let spear = world.children(chief)[0];
    assert_eq!(world.name_of(spear).as_deref(), Some("spear"));
    Ok(())
}
```

`spawn_prefab` creates new entities on every call and returns the root.
Nothing is left in the world when it fails, for example on a component name
that is not registered (`Error::UnknownComponent`).

`Overrides` replace whole components of the prefab and may add components it
lacks. `set(name, &value)` encodes a value, `set_json(name, json)` takes a
`serde_json::Value`, and `child(index, overrides)` applies overrides to the
child at that index of `children`.

`Prefab::capture(&world, root)` makes a prefab from a live entity tree, and
`to_json` writes it. A `PrefabLibrary` holds prefabs by name
(`insert`, `insert_json`, `get`) for scenes to refer to.

## Scene documents

A `Scene` is a document of entities that `world.instantiate_scene` spawns as
a unit, such as a level or a menu. It has a `name`, free-form `properties`
for the game's own use, and a list of entities numbered from 1:

| `SceneEntity` field | Meaning |
|---|---|
| `name` | A label. It becomes a `Name` component and finds the entity in the spawned copy. Unique within the scene. |
| `parent` | The number of the parent entity, or none for a root. |
| `prefab` | The name of a prefab in the library to spawn first. Each entry of `components` then replaces the prefab's component of the same name. |
| `components` | Component values by registered name. An entity reference is another entity's number. |

```rust
use serde::{Deserialize, Serialize};
use yowie::ecs::{Prefab, PrefabLibrary, Scene, SceneOptions, World};

#[derive(Clone, Serialize, Deserialize)]
struct Door {
    locked: bool,
}

const ROOM: &str = r#"{
  "version": 1,
  "name": "cellar",
  "properties": { "music": "drips.ogg" },
  "entities": [
    { "name": "Room", "components": {
        "Transform2D": { "position": [0, 0], "rotation": 0, "scale": [1, 1] } } },
    { "name": "Door", "parent": 1, "prefab": "door",
      "components": { "door": { "locked": true } } }
  ]
}"#;

fn main() -> Result<(), yowie::ecs::Error> {
    let mut world = World::new();
    world.register::<Door>("door");
    let mut prefabs = PrefabLibrary::new();
    prefabs.insert_json("door", r#"{ "components": { "door": { "locked": false } } }"#)?;

    let scene = Scene::from_json(ROOM)?;
    let options = SceneOptions { prefabs: Some(&prefabs), ..SceneOptions::default() };
    let mut cellar = world.instantiate_scene(&scene, &options)?;

    let door = cellar.entity("Door").ok_or(yowie::ecs::Error::InvalidDocument("no door".into()))?;
    assert_eq!(world.get::<&Door>(door).map(|d| d.locked), Ok(true));
    assert_eq!(scene.properties["music"], "drips.ogg");

    // Remove this copy of the scene and everything it spawned.
    cellar.despawn(&mut world);
    assert_eq!(world.len(), 0);
    Ok(())
}
```

`Scene::from_json` parses the document and checks its structure: unique
names, parent numbers in range and not the entity itself. A document
without `version` reads as the current version; a newer version fails with
`Error::UnsupportedVersion`.

`instantiate_scene` creates new entities on every call, so several copies
of a scene live side by side. A reference to a number the scene does not use
becomes `Entity::DANGLING`. Nothing is left in the world when the call
fails. `SceneOptions` has three fields:

| Field | Default | Effect |
|---|---|---|
| `prefabs` | `None` | The library that resolves `prefab` names. Without one, a scene that names a prefab fails with `Error::UnknownPrefab`. |
| `parent` | `None` | Hangs every root of the scene under this entity. |
| `skip_unknown` | `false` | Ignores unregistered component names instead of failing. |

The returned `SceneInstance` lists the `roots` and every entity it
`spawned` (a prefab's children follow the entity that referenced it), finds
named entities with `entity(name)`, and removes the copy with
`despawn(&mut world)`, including children the entities gained since.
`world.name_of(e)` returns an entity's `Name`.

To export entities as a scene, call `Scene::capture(&world, &roots)` and
`to_json`. A `Name` component that is unique in the capture becomes the
entity's `name`. Unregistered components are left out, and references to
entities outside the capture are lost.

To load a scene from the assets, read the file through the asset file
system and parse it:

```rust,no_run
use yowie::ecs::{Scene, SceneInstance, SceneOptions, World};
use yowie::{Context, GameResult};

fn load_level(ctx: &Context, world: &mut World, path: &str) -> GameResult<SceneInstance> {
    let json = ctx.assets.vfs().read_to_string(path)?;
    let scene = Scene::from_json(&json)?;
    Ok(world.instantiate_scene(&scene, &SceneOptions::default())?)
}
```

## How the data is stored

Storage and queries come from hecs, which stores entities by archetype:
entities with the same set of component types share one table, with each
component type in its own contiguous column. That layout decides what
operations cost.

- Iterating a query visits only the tables whose component sets match, and
  reads each column in order. Iteration over many entities is cheap.
- Adding or removing a component moves the entity to the table for its new
  set of components. The cost is proportional to the number of components
  the entity has. `set_parent` and `detach` add and remove `Parent` and
  `Children`, so they move entities too.
- Spawning many entities with the same components is a run of appends to
  one table.
- For state that changes every update, change a field. Adding and removing
  a marker component every update moves the entity each time.

The layers Yowie adds have their own costs:

- Each `Commands` entry is a boxed closure, one allocation per command.
- Resources are a map from type to boxed value; events are one `Vec` per
  type.
- `propagate_transforms` looks at every entity in the world to find the
  roots, then walks the hierarchy. Its cost grows with the whole world, not
  only with the entities that moved.
- The schedule sorts a stage once after systems are added, not on every
  run. `stats` records the time of every system run.

## Structuring a game around the ECS

- Keep one `World` and its `Schedule` per running game, in the `Game`
  struct. To restart, call `world.clear()` and insert fresh resources:
  `clear` despawns entities only.
- Make components plain data and put behaviour in systems. A system reads
  resources and components and writes components, resources and events.
- Bring input and time in as resources written in `update` before
  `schedule.run`, and take results out as events or resources the game reads
  after it. The systems then never need the `Context`.
- Order systems by naming them and stating `before` and `after`, grouped
  with `in_set`. The order they are added in then does not matter.
- Gate whole groups of systems with run conditions, such as "the game is not
  over", instead of checking at the top of each system.
- Draw by querying the world in `draw`, and leave it unchanged there.
- Register every component and resource that belongs in a save, mark derived
  ones with `ignore_component`, and author levels as scene documents with
  prefabs for what repeats.

The Tetris example is built this way. The [Tetris guide](tetris.md)
explains its code.

## Examples

| Command | Shows |
|---|---|
| `cargo run --example tetris` | Systems, resources, events, ordering with sets, run conditions and a restart. `--autoplay` plays itself. |
| `cargo run --example solar` | A scene document and a moon prefab, the hierarchy carrying moons around planets, a belt spawned in code, and the console's Entities panel. `--select <name>` starts with a body selected, `--panels` opens the panels, `--timings` shows run times in headless runs. |
| `cargo run --example animation` | The `animate` system driving 2D and 3D transforms, and `AnimationEvent`s chaining clips. |

For the API reference, run `cargo doc --open` and look under `yowie_ecs`.
