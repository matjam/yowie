---
title: Animation
group: Graphics
order: 4
summary: keyframe curves, clips and players for sprites and 3D transforms, crossfades and events, flipbooks, skeletons from glTF, layers and masks, blend spaces, root motion, IK and morph weights
---

# Animation

`yowie::anim` animates values over time: a sprite's position and tint, a
3D transform, a frame of a sprite sheet, the joints of a skeleton, the
morph weights of a face. It has no GPU dependency. You advance players
in `update` and copy the values they produce into whatever draws: a
sprite, a component, a pose for a skinned model.

These examples use it:

- `cargo run --example animation` plays keyframe clips on 2D sprites and
  3D cubes through the ECS, walks a flipbook character, crossfades a hero
  between idle, jump and spin, turns `Finished` events into a return to
  idle, fires an event from a robot arm, reaches a moving target by
  two-bone IK, mixes two clips by a slider and animates morph weights.
  `--autoplay` presses the Jump and Spin buttons on its own; `--yaw
  <radians>` sets the starting camera angle.
- `cargo run --example tiles` walks a character with a flipbook from a
  sprite sheet.

For drawing sprites and sheets see [2D graphics](graphics-2d.md); for
loading and drawing skinned glTF models see [3D graphics](graphics-3d.md).
For the full API, run `cargo doc --open`.

## Keyframe curves

A `Curve<T>` is a list of `Key<T>`s sorted by time in seconds. Sampling
between two keys interpolates as the earlier key says; sampling before
the first key or after the last holds the end value. `sample` returns
`None` only for a curve with no keys.

```rust
use yowie::anim::{Curve, Key};
use yowie::{Vec2, vec2};

let path = Curve::linear([(0.0, Vec2::ZERO), (2.0, vec2(100.0, 0.0))]);
assert_eq!(path.sample(1.0), Some(vec2(50.0, 0.0)));
assert_eq!(path.sample(5.0), Some(vec2(100.0, 0.0)));
assert_eq!(path.duration(), 2.0);

// Hold each value until the next key.
let ammo = Curve::new(vec![Key::step(0.0, 3.0_f32), Key::step(1.0, 2.0)]);
assert_eq!(ammo.sample(0.9), Some(3.0));
```

Each key's `Interp` chooses how the curve leaves it:

| `Interp` | Key constructor | Behaviour |
|---|---|---|
| `Linear` (default) | `Key::new(time, value)` | Straight-line interpolation; rotations take the shortest arc. |
| `Step` | `Key::step(time, value)` | Holds the value until the next key. |
| `Cubic` | `Key::cubic(time, incoming, value, outgoing)` | A Hermite spline through the key, with incoming and outgoing tangents in value units per second, as glTF cubic splines are. A key without tangents contributes a zero tangent. |

`Curve::linear` and `Curve::step` build a curve from `(time, value)`
pairs; `Curve::new` takes keys and sorts them.

The value types that curves interpolate implement `Animatable`: `f32`,
`Vec2`, `Vec3`, `Vec4`, `Quat`, `Color`, `Transform` and `Transform2D`.
Colours interpolate in sRGB, as `Color::lerp` does.

Keyframe curves have no easing functions of their own. Give a segment a
shape with cubic keys, or, for a single eased movement such as a menu
sliding in, use a tween from `yowie::tween`:

```rust
use yowie::tween::{Curve, Ease, Tween};

let mut slide = Tween::new(-200.0_f32, 0.0, 0.4).ease(Ease::Out(Curve::Back));
let x = slide.update(0.2);
assert!(x > -200.0);
slide.update(0.2);
assert!(slide.done());
```

`Ease` has the In, Out and InOut forms of quadratic to quintic, sine,
exponential, circular, back, elastic and bounce curves, plus
`Smoothstep`, `Steps`, CSS-style cubic Béziers and custom functions.
[Game services](services.md) covers tweens and chains of them.

## Clips and tracks

A `Clip` is a named set of tracks that play together, plus timed events.
Each track pairs a `Channel` (a curve of one value kind) with a target
name.
What the name means is up to whoever applies the clip: a joint for a
skeleton, a node for a glTF model, `"position"` for a 2D transform, or a
property of your own such as `"tint"`.

```rust
use std::f32::consts::PI;
use std::sync::Arc;

use yowie::anim::{Channel, Clip, Curve, LoopMode};
use yowie::{Color, Vec2, vec2};

fn bounce() -> Arc<Clip> {
    let up = vec2(0.0, -40.0);
    Arc::new(
        Clip::new("bounce")
            .with_mode(LoopMode::Loop)
            .with_track("position", Channel::Vec2(Curve::linear([
                (0.0, Vec2::ZERO),
                (0.3, up),
                (0.6, Vec2::ZERO),
            ])))
            .with_track("rotation", Channel::Float(Curve::linear([(0.0, 0.0), (0.6, PI)])))
            .with_track("tint", Channel::Color(Curve::linear([
                (0.0, Color::WHITE),
                (0.3, Color::rgb8(255, 200, 80)),
                (0.6, Color::WHITE),
            ])))
            .with_event(0.6, "land"),
    )
}

assert_eq!(bounce().duration(), 0.6);
```

A clip's duration is the last key or event time of any track, unless
`with_length` fixes it. Clips, curves and blend trees implement serde's
`Serialize` and `Deserialize`, so they can live in data files in any
format serde reads.

| `Channel` | Curve of | Sampled as |
|---|---|---|
| `Frame` | `f32` | `Value::Frame(usize)`, the floor of the curve value: a sprite frame index |
| `Float` | `f32` | `Value::Float` |
| `Vec2`, `Vec3`, `Vec4` | vectors | `Value::Vec2` and so on |
| `Quat` | `Quat` | `Value::Quat` |
| `Color` | `Color` | `Value::Color` |
| `Transform`, `Transform2D` | whole transforms | `Value::Transform`, `Value::Transform2D` |
| `Translation`, `Rotation`, `Scale` | `Vec3`, `Quat`, `Vec3` | one part of a joint's or node's transform |
| `Weights` | one `f32` curve per morph target | `Value::Weights(Vec<f32>)` |

To sample a clip without a player, call `Clip::apply(time, target)` or
`Clip::for_each_value(time, f)`.

## Playing clips

A `Player` plays one clip at a time. Start a clip with `play`, call
`advance(dt)` once per update, and read the values with `apply` (into
anything that implements `Target`) or `evaluate` (into a closure). Any
`FnMut(&str, &Value)` closure is a `Target`.

```rust,no_run
use std::sync::Arc;

use yowie::anim::{Clip, Player, Value};
use yowie::gfx::{Graphics, Sprite, Texture};
use yowie::{Color, Vec2};

struct Coin {
    anchor: Vec2,
    player: Player,
}

impl Coin {
    fn new(anchor: Vec2, clip: Arc<Clip>) -> Coin {
        let mut player = Player::new();
        player.play(clip);
        Coin { anchor, player }
    }

    fn update(&mut self, dt: f32) {
        self.player.advance(dt);
    }

    fn draw(&self, gfx: &mut Graphics, texture: &Texture) {
        let mut offset = Vec2::ZERO;
        let mut turn = 0.0;
        let mut tint = Color::WHITE;
        self.player.evaluate(|name, value| match (name, value) {
            ("position", Value::Vec2(v)) => offset = v,
            ("rotation", Value::Float(r)) => turn = r,
            ("tint", Value::Color(c)) => tint = c,
            _ => {}
        });
        let sprite = Sprite::at(self.anchor + offset).centered().rotation(turn).tint(tint);
        gfx.draw_sprite(texture, &sprite);
    }
}
```

What happens at the end of a clip depends on its `LoopMode`, which
`Clip::with_mode` sets and `Player::set_loop_mode` overrides for the
clip playing:

| `LoopMode` | At the end |
|---|---|
| `Loop` (default) | Starts over. |
| `PingPong` | Plays backwards, then forwards again. |
| `Once` | Reports `Finished` and stops driving targets: `evaluate` yields nothing. |
| `Clamp` | Reports `Finished` and keeps applying the last pose. |

`set_speed` scales playback (2 is double speed, a negative speed plays
backwards), `pause` and `resume` freeze and continue, `seek(time)` jumps
without firing events for the skipped range, and `stop` clears the
player. `time`, `progress` (0 to 1), `is_playing` and `is_finished`
report where playback is. A backwards `Once` or `Clamp` clip finishes at
once unless you first `seek` to its end.

## Crossfades

To change clips without a sudden jump in the pose, call `crossfade(clip, seconds)` instead of
`play`. The old clip keeps playing while its weight falls to zero over
`seconds` of update time; the fade time does not change with playback
speed. During the fade, `evaluate` blends each track that both clips
have, and a track only the old clip has keeps the old clip's value.
`weights()` lists the contributing clips, the incoming one first, with
weights that sum to one.

Values blend by kind: numbers, vectors and colours linearly, rotations
along the shortest arc. Frame indices, and two values of different
kinds, cannot be mixed, so the result switches from the old value to the
new one halfway through the fade.

## Events

To mark moments in a clip, such as a footstep or the frame a sword
connects, add events with `with_event(time, name)`. After each `advance`,
`take_events` returns what playback crossed, oldest first:

- `PlayerEvent::Marker { clip, name, time }` for each event crossed,
  including across loop points and steps longer than the clip. An event
  at time 0 fires on the first advance after `play`. A clip fading out
  still reports its markers.
- `PlayerEvent::Finished { clip }` when a `Once` or `Clamp` clip reaches
  its end.

```rust,no_run
use std::sync::Arc;

use yowie::anim::{Clip, Player, PlayerEvent};

fn update_hero(player: &mut Player, idle: &Arc<Clip>, dt: f32, footsteps: &mut u32) {
    player.advance(dt);
    for event in player.take_events() {
        match event {
            PlayerEvent::Marker { name, .. } if name == "step" => *footsteps += 1,
            PlayerEvent::Finished { .. } => player.crossfade(Arc::clone(idle), 0.3),
            _ => {}
        }
    }
}
```

## Flipbooks

A `Flipbook` is a list of sprite-sheet frame indices, each with a
duration, and named tags over ranges of them. A `FlipbookPlayer` plays
the whole book or one tag and reports the sheet index to draw. The usual
source is a sprite sheet laid out in rows of animations, which
`yowie::gfx::sheet_flipbook` describes in one call, or an Aseprite file,
whose tags and frame durations `AsepriteSheet::flipbook` reads.

```rust,no_run
use std::sync::Arc;

use yowie::anim::{FlipbookPlayer, LoopMode};
use yowie::gfx::{Graphics, Sprite, SpriteSheet, sheet_flipbook};
use yowie::{Vec2, vec2};

fn make_walker() -> FlipbookPlayer {
    // Frames 0 to 15 at 8 frames a second; each tag is one row of four.
    let book = sheet_flipbook(
        0..16,
        8.0,
        &[("down", 0, 3), ("up", 4, 7), ("left", 8, 11), ("right", 12, 15)],
    );
    let mut player = FlipbookPlayer::new(Arc::new(book));
    player.play_tag("down", LoopMode::Loop);
    player
}

fn draw_walker(gfx: &mut Graphics, sheet: &SpriteSheet, player: &FlipbookPlayer, feet: Vec2) {
    if let Some(frame) = player.frame() {
        let sprite = Sprite::at(feet).origin(vec2(0.5, 1.0));
        sheet.draw_frame(gfx, frame as u32, &sprite);
    }
}
```

Call `advance(dt)` each update while the animation should move.
`play_tag` restarts from the tag's first frame and returns `false` for an
unknown tag, so call it only when the tag changes. A tag's
`TagDirection` is `Forward`, `Reverse` or `PingPong`; a ping-pong tag
played with `LoopMode::Loop` bounces. With `Once` or `Clamp`,
`is_finished` turns true at the end and `frame` keeps returning the last
frame. To build a book by hand, call `Flipbook::from_fps(indices, fps)`
and add tags with `with_tag(name, from, to, direction)`, where `from`
and `to` are positions in the book's frame list.

A clip can also carry a `Channel::Frame` track, which samples to a frame
index. That keeps a sprite's frames in step with its other tracks in one
clip.

## Animation in the ECS

`yowie::ecs_glue` has a system that runs players stored as components.
Give an entity a `Player` and a `Transform` or `Transform2D`, insert the
update step as the `DeltaTime` resource, and add `animate` to the
schedule (`add_animation` adds it under the name `animation`). Each run
advances every player, writes its values into the entity's transform and
sends the player's events as `AnimationEvent`s.

```rust,no_run
use std::sync::Arc;

use yowie::anim::{Clip, Player};
use yowie::ecs::{Schedule, World};
use yowie::ecs_glue::{add_animation, set_delta};
use yowie::{Context, Transform2D};

fn spawn_animated(world: &mut World, clip: Arc<Clip>) {
    let mut player = Player::new();
    player.play(clip);
    world.spawn((player, Transform2D::IDENTITY));
}

fn build_schedule() -> Schedule {
    let mut schedule = Schedule::new();
    add_animation(&mut schedule);
    schedule
}

fn update_world(ctx: &mut Context, world: &mut World, schedule: &mut Schedule) {
    set_delta(world, ctx);
    schedule.run(world);
}
```

Tracks reach the components by value kind and name:

| Component | Tracks applied |
|---|---|
| `Transform` | `Value::Transform` replaces it; `Translation`, `Rotation` and `Scale` set that part, whatever the track's name. |
| `Transform2D` | `Value::Transform2D` replaces it; the tracks named `position` (`Vec2`), `rotation` (`Float`, radians) and `scale` (`Vec2`) set that part. |

Tracks that match nothing are ignored, so one clip can drive other
values too: a later system can read the same `Player` with `evaluate`,
as the `animation` example does for a sprite's tint and size. To react
to events, read `AnimationEvent`s in a later system; each names the
entity and carries the `PlayerEvent`. The [ECS guide](ecs.md) covers
worlds, schedules and events.

## Skeletons

A `Skeleton` is a joint hierarchy with a rest pose. Each `Joint` has a
unique name, a parent index that comes before its own, a local rest
transform and an inverse bind matrix. `Skeleton::from_rest` derives the
inverse bind matrices from the rest pose; `Skeleton::new` takes joints
whose matrices you already have, such as those from a file.

A `Pose` holds one local `Transform` per joint plus morph weights by node
name. `pose_from_player(&player)` gives the pose a player shows over the
rest pose (track names are joint names), and `pose_from_clip(&clip,
time)` samples a clip directly. `world_matrices(&pose)` returns each
joint's model-space matrix, which places things on a hand or a muzzle,
and `joint_matrices(&pose)` returns the skinning matrices a skinned mesh
uploads.

```rust
use yowie::Transform;
use yowie::anim::{AnimError, Joint, Skeleton};

fn build_arm() -> Result<Skeleton, AnimError> {
    Skeleton::from_rest(vec![
        Joint::new("shoulder", None, Transform::IDENTITY),
        Joint::new("elbow", Some(0), Transform::from_xyz(0.0, 1.0, 0.0)),
        Joint::new("hand", Some(1), Transform::from_xyz(0.0, 1.0, 0.0)),
    ])
}

fn main() -> Result<(), AnimError> {
    let arm = build_arm()?;
    let hand = arm.index_of("hand").unwrap_or(0);
    let world = arm.world_matrices(&arm.rest_pose());
    assert_eq!(world[hand].w_axis.y, 2.0);
    Ok(())
}
```

`Skeleton::new` and `from_rest` return `AnimError::InvalidSkeleton` when
a parent does not come before its child or a name repeats.

### Skeletons and clips from glTF

To read skins and animations from a glTF or GLB file, call
`yowie::anim::gltf_import::import_path` (or `import_slice` for bytes with
embedded buffers). The result has one `GltfSkeleton` per skin, one `Clip`
per animation (looping by default), and `node_names`, the unique name of
every node that the clips' tracks use. A node with no name is called
`node<index>`, and a name already taken by an earlier node gets
`#<index>` appended. Each `GltfSkeleton` maps glTF nodes to joints and back,
and its `skin_order` maps a mesh's joint attribute to joint indices.

To draw an animated glTF model, the renderer does this work for you.
`yowie::gfx::scene::Model::load` reads the meshes, materials, skins,
morph targets and clips, and a `ModelInstance` holds one pose and a
`Player`:

```rust,no_run
use yowie::gfx::scene::{Model, ModelInstance};
use yowie::{Context, GameResult};

fn load_knight(ctx: &mut Context) -> GameResult<ModelInstance> {
    let model = Model::load(&ctx.gfx, "assets/models/knight.glb")?;
    let mut knight = model.instance();
    knight.play("Idle");
    Ok(knight)
}

fn update_knight(knight: &mut ModelInstance, running: bool, dt: f32) {
    let want = if running { "Run" } else { "Idle" };
    let current = knight.player.clip().map(|c| c.name.clone());
    if current.as_deref() != Some(want) {
        knight.crossfade(want, 0.25);
    }
    knight.advance(dt);
}
```

`play` and `crossfade` take a clip name and return `false` when the model
has none by that name; `Model::clips` lists them. `advance` runs the
player and recomputes the pose; after changing `player` directly, call
`update_pose`. `node_matrix(node)` returns a node's model-space matrix,
and `Model::node_index(name)` finds a node. Draw the instance in a scene
with `draw_model`, as [3D graphics](graphics-3d.md) describes.

## Layers and masks

To play a second clip over part of a skeleton, such as waving while
walking, use a `Layer`. A layer has its own `Player`, a `JointMask`
saying which joints it affects, a `weight` from 0 to 1 and a
`LayerMode`. `Skeleton::evaluate(&base, &layers)` returns the base
player's pose with the layers applied in order.

| `LayerMode` | Effect |
|---|---|
| `Override` (default) | Replaces the masked joints' transforms with the layer's, scaled by weight. |
| `Additive` | Adds the layer's difference from the rest pose, scaled by weight: breathing or recoil over any base clip. |

```rust,no_run
use std::sync::Arc;

use yowie::anim::{Clip, JointMask, Layer, Player, Pose, Skeleton};
use yowie::GameResult;

fn wave_layer(skeleton: &Skeleton, wave: Arc<Clip>) -> GameResult<Layer> {
    let mut player = Player::new();
    player.play(wave);
    let mask = JointMask::subtree(skeleton, "upper_arm.R")?;
    Ok(Layer::new(player, mask))
}

fn pose_hero(skeleton: &Skeleton, walk: &mut Player, layers: &mut [Layer], dt: f32) -> Pose {
    walk.advance(dt);
    for layer in layers.iter_mut() {
        layer.player.advance(dt);
    }
    skeleton.evaluate(walk, layers)
}
```

`JointMask::all` and `none` cover every joint or none, `from_names`
selects named joints, `subtree` selects a joint and everything below it,
and `with_weight(joint, weight)` sets one joint's weight for a partial
blend. To fade a layer in or out, change its `weight` over time. A layer
with weight 0, or whose player has no clip, is skipped. A finished
`Once` clip contributes the rest pose on the masked joints. To hold a
layer's last pose, give its clip `LoopMode::Clamp`; to hand the joints
back to the base clip, call `layer.player.stop()` when it finishes.

## Blend spaces

To mix clips by game parameters, such as idle, walk and run by speed,
build a blend tree. Its nodes are `BlendNode`s:

| `BlendNode` | Mixes |
|---|---|
| `Clip(name)` | one clip on its own |
| `Space1D(BlendSpace1D)` | clips placed along one parameter; between two clips the pose mixes linearly, and past the ends the nearest clip plays alone |
| `Space2D(BlendSpace2D)` | clips placed on a plane of two parameters, such as a strafe set read from a velocity; weights are normalised to sum to one |
| `Mix { parameter, children }` | whole subtrees placed along a parameter, such as standing locomotion fading into crouched locomotion |

A `Blend` runs a tree. Set parameters by name with `set` (an unset
parameter reads as 0), call `advance(dt, &clips)` each update and
`pose(&skeleton, &clips)` for the mixed pose. `clips` is a `ClipSet`, a
map from clip name to `Arc<Clip>`. All the mixed clips play at one shared
phase of their own lengths, advancing at the weighted rate of the blend,
so a walk's and a run's feet land together.

```rust,no_run
use std::sync::Arc;

use yowie::anim::blend::BlendPoint1D;
use yowie::anim::{Blend, BlendNode, BlendSpace1D, Clip, ClipSet, Pose, Skeleton};

fn locomotion() -> Blend {
    let point = |clip: &str, at: f32| BlendPoint1D { clip: clip.into(), at };
    Blend::new(BlendNode::Space1D(BlendSpace1D {
        parameter: "speed".into(),
        clips: vec![point("idle", 0.0), point("walk", 1.5), point("run", 5.0)],
    }))
}

fn clip_set(idle: Arc<Clip>, walk: Arc<Clip>, run: Arc<Clip>) -> ClipSet {
    [("idle", idle), ("walk", walk), ("run", run)]
        .into_iter()
        .map(|(name, clip)| (name.to_string(), clip))
        .collect()
}

fn pose_runner(blend: &mut Blend, skeleton: &Skeleton, clips: &ClipSet, speed: f32, dt: f32) -> Pose {
    blend.set("speed", speed);
    blend.advance(dt, clips);
    blend.pose(skeleton, clips).unwrap_or_else(|| skeleton.rest_pose())
}
```

`weights()` lists the clips and weights for the current parameters,
which is useful for a debug readout. `BlendPoint2D` and `BlendChild`,
the points of the 2D space and the children of a `Mix`, are in
`yowie::anim::blend` with `BlendPoint1D`.

## Root motion

Some clips move the root joint as part of the animation, such as a walk
that travels forwards with the feet planted. To move the character by
that amount instead, read `player.root_motion(root)` after each `advance`. It
returns a `RootMotion` with the root joint's `translation` and `yaw`
(radians about +Y) over that advance, in model space, summed across loop
points and blended through crossfades. Then hold the root in place in
the pose with `root_motion::lock_root`, so the movement is not applied
twice.

```rust,no_run
use yowie::anim::root_motion::lock_root;
use yowie::anim::{Player, Pose, Skeleton};
use yowie::{Quat, Transform};

fn step_character(
    skeleton: &Skeleton,
    player: &mut Player,
    body: &mut Transform,
    dt: f32,
) -> Option<Pose> {
    player.advance(dt);
    let motion = player.root_motion("hips");
    body.translation += body.rotation * motion.translation;
    body.rotation *= Quat::from_rotation_y(motion.yaw);

    let hips = skeleton.index_of("hips")?;
    let mut pose = skeleton.pose_from_player(player);
    lock_root(&mut pose, hips, &skeleton.joints()[hips].rest);
    Some(pose)
}
```

`lock_root` holds the root's translation (vertical included) and yaw at
the rest pose; pitch and roll animate as authored. The reported
translation includes the vertical part too, so the up and down movement
of a walk cycle moves the body. To keep a character on the ground, set
`motion.translation.y` to 0 before applying it; the pose then shows no
vertical movement of the root, because `lock_root` holds it.
`RootMotion::scaled` and `plus` combine movements, and
`root_motion_between` measures a clip directly.

## Inverse kinematics

The solvers in `yowie::anim::ik_pose` read joint positions from a pose,
solve in model space, and write the resulting rotations back into the
pose. Run them after sampling the clip and before using the pose.

| Function | Solves |
|---|---|
| `solve_two_bone(skeleton, pose, [root, mid, end], target, pole)` | A two-bone chain such as a leg or an arm, so `end` reaches `target` with `mid` bending towards `pole`. Returns whether the target was in reach; out of reach, the chain straightens towards it. |
| `solve_look_at(skeleton, pose, joint, forward, target, limit)` | Turns `joint` so its local `forward` axis faces `target`, by at most `limit` radians: heads, eyes, turrets. |
| `solve_chain(skeleton, pose, joints, target, solver, options)` | A chain of any length with `ChainSolver::Fabrik` or `ChainSolver::Ccd`. `ChainOptions` sets the tolerance, the most iterations and a bend limit per joint. Returns a `ChainSolve` with whether it converged and the remaining error. |

Each joint in a chain must be the parent of the next; the solvers return
an error otherwise.

```rust,no_run
use yowie::anim::ik_pose::solve_two_bone;
use yowie::anim::{Player, Pose, Skeleton};
use yowie::{GameResult, Vec3, vec3};

/// The player's pose with the arm bent to touch `target`, and whether
/// the hand reached it.
fn reach(skeleton: &Skeleton, player: &Player, target: Vec3) -> GameResult<(Pose, bool)> {
    let mut pose = skeleton.pose_from_player(player);
    let pole = vec3(-2.0, 1.0, 1.0);
    let reached = solve_two_bone(skeleton, &mut pose, [0, 1, 2], target, pole)?;
    Ok((pose, reached))
}
```

For a glTF model, set the solved joints on the instance with
`ModelInstance::set_node_local`, which holds until the next `advance` or
`update_pose`. The lower-level functions `ik::two_bone`, `ik::look_at`,
`chain::fabrik` and `chain::ccd` work on plain points and directions
when you keep your own joint positions.

## Morph targets

Morph targets (blend shapes) deform a mesh by weighted offsets: a smile,
a blink, a squash. A clip animates them with a `Channel::Weights` track,
one curve per target, which samples to `Value::Weights`.
`morph::weights_channel(times, values, interp)` builds that channel from
rows of weights. In a skeleton pose the weights land in `Pose::morph`
under the track's name, which is the name of the node that owns the
mesh.

glTF models carry their morph targets and the clips that animate them,
and `ModelInstance::advance` applies those. To set weights from game
code, call `set_morph_weights(node, &weights)`, which holds until the
next pose update, and read them back with `morph_weights(node)`.

To keep weights by name in game code, use `morph::MorphWeights`. `set`
and `get` address a target by name, `apply` copies the weights from a
sampled `Value::Weights`, and `blend` moves every weight towards another
set, for expression changes the game chooses.

```rust
use yowie::anim::morph::MorphWeights;

let mut face = MorphWeights::new(vec!["smile".into(), "blink".into()]);
assert!(face.set("blink", 1.0));
let mut happy = face.clone();
happy.set("smile", 1.0);
face.blend(&happy, 0.5);
assert_eq!(face.get("smile"), Some(0.5));
```
