//! Entity component system: queries, systems, resources, events,
//! hierarchy, saves, prefabs, cloning and scene documents.
//!
//! Storage and queries come from [`hecs`]: a [`World`] dereferences to a
//! `hecs::World`, so `spawn`, `get`, `query`, `query_mut`, `insert` and
//! `remove` work as hecs documents them. This crate adds the game-facing
//! layer:
//!
//! - [`Commands`]: spawns, despawns and component changes queued while the
//!   world is borrowed and applied later.
//! - Resources (`World::insert_resource`) and double-buffered events
//!   (`World::send_event`, `World::read_events`).
//! - [`Schedule`], [`System`] and [`Stage`]: ordered stages, run conditions
//!   and per-system timing ([`SystemStat`]).
//! - Hierarchy: [`Parent`], [`Children`], `World::set_parent`, recursive
//!   `World::despawn`, and [`propagate_transforms`] from `Transform` and
//!   `Transform2D` to [`GlobalTransform`] and [`GlobalTransform2D`].
//! - A component registry (`World::register`, `World::register_mapped`)
//!   that drives `World::save` and `World::load`, `World::clone_entity` and
//!   `World::clone_tree`, [`Prefab`]s and [`Scene`] documents. Components
//!   that hold entity handles implement [`MapEntities`] so the handles
//!   survive loading and cloning.
//!
//! # Example
//!
//! ```
//! use serde::{Deserialize, Serialize};
//! use yowie_ecs::{LoadOptions, SaveOptions, Schedule, Stage, World, propagate_transforms};
//! use yowie_core::{Transform2D, vec2};
//!
//! #[derive(Clone, Serialize, Deserialize)]
//! struct Velocity(f32, f32);
//!
//! struct Dt(f32);
//! struct Moved(usize);
//!
//! fn movement(world: &mut World) {
//!     let dt = world.resource::<Dt>().map_or(0.0, |d| d.0);
//!     let mut moved = 0;
//!     for (t, v) in world.query_mut::<(&mut Transform2D, &Velocity)>() {
//!         t.position += vec2(v.0, v.1) * dt;
//!         moved += 1;
//!     }
//!     world.send_event(Moved(moved));
//! }
//!
//! let mut world = World::new();
//! world.register::<Velocity>("Velocity");
//! world.insert_resource(Dt(0.5));
//! let ship = world.spawn((Transform2D::IDENTITY, Velocity(2.0, 0.0)));
//!
//! let mut schedule = Schedule::new();
//! schedule.add_system(Stage::Update, "movement", movement);
//! schedule.add_system(Stage::PostUpdate, "transforms", propagate_transforms);
//! schedule.run(&mut world);
//!
//! assert_eq!(world.get::<&Transform2D>(ship)?.position, vec2(1.0, 0.0));
//! assert_eq!(world.read_events::<Moved>().map(|m| m.0).sum::<usize>(), 1);
//!
//! // Save the world and load it into a fresh one.
//! let json = world.save_string(&SaveOptions::default())?;
//! let mut restored = World::new();
//! restored.register::<Velocity>("Velocity");
//! let map = restored.load_str(&json, &LoadOptions::default())?;
//! let ship2 = map.get(ship);
//! assert_eq!(restored.get::<&Transform2D>(ship2)?.position, vec2(1.0, 0.0));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]

mod clone;
mod commands;
mod entity_ref;
mod error;
mod events;
mod hierarchy;
mod instantiate;
mod order;
mod prefab;
mod registry;
mod resources;
mod save;
mod scene;
mod schedule;
mod transform;
mod world;

pub use commands::Commands;
pub use entity_ref::{EntityMap, MapEntities, Unmapped, entity_serde};
pub use error::{Error, Result};
pub use events::Events;
pub use hecs;
pub use hecs::{
    Bundle, Component, DynamicBundle, Entity, EntityBuilder, EntityRef, Or, Query, QueryBorrow,
    Ref, RefMut, With, Without,
};
pub use hierarchy::{Children, Parent};
pub use instantiate::{SceneInstance, SceneOptions};
pub use prefab::{Overrides, Prefab, PrefabLibrary};
pub use registry::Name;
pub use save::{LoadOptions, SAVE_VERSION, SaveOptions};
pub use scene::{SCENE_VERSION, Scene, SceneEntity};
pub use schedule::{Schedule, Stage, System, SystemStat, has_events, not, resource_exists};
pub use transform::{GlobalTransform, GlobalTransform2D, propagate_transforms};
pub use world::World;
