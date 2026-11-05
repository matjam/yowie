//! Systems that connect the engine's services to the ECS: advancing
//! animation players and applying them to transforms, and the schedule
//! statistics the console reads.

use yowie_anim::{Player, PlayerEvent, Value};
use yowie_core::{Transform, Transform2D};
use yowie_ecs::{Entity, Schedule, Stage, SystemStat, World};

use crate::Context;

/// A resource holding the seconds one update advances. Insert it with
/// [`set_delta`] before running the schedule; [`animate`] reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeltaTime(pub f32);

/// An event [`animate`] sends when a [`Player`] reports a marker or the end
/// of a clip.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvent {
    /// The entity whose player reported it.
    pub entity: Entity,
    /// What the player reported.
    pub event: PlayerEvent,
}

/// Stores the context's update step in the world as [`DeltaTime`]. Call it
/// at the start of `update`, before `Schedule::run`.
pub fn set_delta(world: &mut World, ctx: &Context) {
    world.insert_resource(DeltaTime(ctx.time.delta()));
}

/// A system that advances every [`Player`] by [`DeltaTime`], applies the
/// pose to the entity's [`Transform`] or [`Transform2D`], and sends the
/// player's events as [`AnimationEvent`]s.
///
/// The tracks map to components by their value kind. For a [`Transform`]:
/// `Value::Transform` replaces it, `Value::Translation`, `Value::Rotation`
/// and `Value::Scale` set that part. For a [`Transform2D`]:
/// `Value::Transform2D` replaces it, and the tracks named `position`
/// (`Value::Vec2`), `rotation` (`Value::Float`, radians) and `scale`
/// (`Value::Vec2`) set that part. Tracks that match nothing are ignored, so
/// one clip can also drive other values through the player directly.
pub fn animate(world: &mut World) {
    let dt = world.resource::<DeltaTime>().map_or(0.0, |d| d.0);
    let mut events = Vec::new();
    for (entity, player, mut t3, mut t2) in world.query_mut::<(
        Entity,
        &mut Player,
        Option<&mut Transform>,
        Option<&mut Transform2D>,
    )>() {
        player.advance(dt);
        player.evaluate(|name, value| {
            if let Some(t) = t3.as_deref_mut() {
                apply_3d(t, &value);
            }
            if let Some(t) = t2.as_deref_mut() {
                apply_2d(t, name, &value);
            }
        });
        events.extend(
            player
                .take_events()
                .into_iter()
                .map(|event| AnimationEvent { entity, event }),
        );
    }
    for e in events {
        world.send_event(e);
    }
}

fn apply_3d(t: &mut Transform, value: &Value) {
    match value {
        Value::Transform(v) => *t = *v,
        Value::Translation(v) => t.translation = *v,
        Value::Rotation(v) => t.rotation = *v,
        Value::Scale(v) => t.scale = *v,
        _ => {}
    }
}

fn apply_2d(t: &mut Transform2D, name: &str, value: &Value) {
    match (name, value) {
        (_, Value::Transform2D(v)) => *t = *v,
        ("position", Value::Vec2(v)) => t.position = *v,
        ("rotation", Value::Float(v)) => t.rotation = *v,
        ("scale", Value::Vec2(v)) => t.scale = *v,
        _ => {}
    }
}

/// Adds [`animate`] to the schedule's update stage under the name
/// `animation`.
pub fn add_animation(schedule: &mut Schedule) {
    schedule.add_system(Stage::Update, "animation", animate);
}

impl Context {
    /// Records a schedule's per-system timings so the debug console's
    /// systems panel can show them. Call it after `Schedule::run` each
    /// update.
    pub fn record_schedule(&mut self, schedule: &Schedule) {
        self.services.schedule_stats = schedule.stats();
        self.services.schedule_updates = schedule.updates();
    }

    /// The per-system timings last recorded with
    /// [`record_schedule`](Context::record_schedule), in run order. Empty
    /// until a game records a schedule.
    pub fn schedule_stats(&self) -> &[SystemStat] {
        &self.services.schedule_stats
    }

    /// How many times the recorded schedule had run when it was recorded.
    pub fn schedule_updates(&self) -> u64 {
        self.services.schedule_updates
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use yowie_anim::{Channel, Clip, Curve, LoopMode};
    use yowie_core::{Vec2, Vec3, vec2};

    use super::*;

    fn clip_2d() -> Arc<Clip> {
        Arc::new(
            Clip::new("slide")
                .with_mode(LoopMode::Once)
                .with_track(
                    "position",
                    Channel::Vec2(Curve::linear([(0.0, Vec2::ZERO), (2.0, vec2(10.0, 0.0))])),
                )
                .with_track(
                    "rotation",
                    Channel::Float(Curve::linear([(0.0, 0.0), (2.0, 1.0)])),
                ),
        )
    }

    #[test]
    fn players_drive_transforms_and_send_events() {
        let mut world = World::new();
        let mut p = Player::new();
        p.play(clip_2d());
        let e2 = world.spawn((p, Transform2D::IDENTITY));
        let mut p3 = Player::new();
        p3.play(Arc::new(
            Clip::new("lift").with_mode(LoopMode::Once).with_track(
                "t",
                Channel::Translation(Curve::linear([
                    (0.0, Vec3::ZERO),
                    (2.0, Vec3::new(0.0, 8.0, 0.0)),
                ])),
            ),
        ));
        let e3 = world.spawn((p3, Transform::IDENTITY));
        // A one second clip with no transform tracks, finished by the step.
        let mut p4 = Player::new();
        p4.play(Arc::new(
            Clip::new("blink")
                .with_mode(LoopMode::Once)
                .with_track("x", Channel::Float(Curve::linear([(0.0, 0.0), (1.0, 1.0)]))),
        ));
        let e4 = world.spawn((p4, Transform::IDENTITY));
        world.insert_resource(DeltaTime(1.0));

        let mut schedule = Schedule::new();
        add_animation(&mut schedule);
        schedule.run(&mut world);

        let t2 = world
            .get::<&Transform2D>(e2)
            .map(|t| (t.position, t.rotation));
        assert_eq!(t2, Ok((vec2(5.0, 0.0), 0.5)));
        let t3 = world.get::<&Transform>(e3).map(|t| t.translation);
        assert_eq!(t3, Ok(Vec3::new(0.0, 4.0, 0.0)));
        // Only the one second clip finished on that step.
        let finished = world
            .read_events::<AnimationEvent>()
            .filter(|e| matches!(e.event, PlayerEvent::Finished { .. }))
            .map(|e| e.entity)
            .collect::<Vec<_>>();
        assert_eq!(finished, [e4]);
        let stats = schedule.stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].runs, 1);
    }
}
