---
title: Why Rust?
group: Start
order: 2
summary: what Rust gives a game library, which crates Yowie builds on, and the tradeoffs to plan for
---

# Why Rust?

This guide explains which properties of Rust Yowie's design depends on,
which crates it builds on, and what the language costs a game project:
compile times, the code patterns the borrow checker leads to, and the CPU
cost of a portable GPU layer.

## One build on three platforms

To build a Yowie game on Linux, macOS or Windows, run `cargo build`.
Cargo downloads and compiles every dependency from source with the same
command on each platform. The build needs no separate build system and no
graphics SDK. wgpu finds the Vulkan, Metal or DX12 driver when the game
starts.

The platform differences live behind `cfg` attributes in the source, so
each platform compiles only its own code. On Linux, for example, the
window sets an application id for Wayland and X11 that the other
platforms never see. Each platform still needs a C compiler, and Linux
needs the development packages listed in
[getting started](getting-started.md#install-the-system-packages).

Yowie's continuous integration runs `cargo clippy -- -D warnings` and
`cargo test` on Ubuntu, macOS and Windows for every push to `main` and
every pull request, from one workflow file.

## Resources freed on drop

Rust frees a value when its owner goes out of scope, at a point the
compiler decides from the code. There is no garbage collector, so a frame
never pauses for a collection, and memory is released at points the
program's code determines.

Yowie ties GPU and audio resources to this rule. A `Texture`, `Mesh`,
`Shader` or `Sound` is a reference-counted handle, and the GPU memory or
the samples behind it are released when nothing uses them any more. For
a texture, that is when the last handle and the last queued draw that
uses it are gone. A game unloads a level by dropping the level:

```rust,no_run
use yowie::gfx::{Texture, TextureOptions};
use yowie::{Context, GameResult};

struct Level {
    tiles: Texture,
    props: Texture,
}

fn change_level(ctx: &mut Context, current: &mut Option<Level>, next: &str) -> GameResult {
    // Dropping the old level releases both textures.
    *current = None;
    let tiles = ctx.load_texture(&format!("{next}/tiles.png"), TextureOptions::default())?;
    let props = ctx.load_texture(&format!("{next}/props.png"), TextureOptions::default())?;
    *current = Some(Level { tiles, props });
    Ok(())
}
```

Because a drop is an ordinary function call, the engine also controls
which thread runs it. The audio thread does not allocate or free memory
in steady state, so it never waits on the allocator. The mixer sends
finished voices back over a channel, and the engine drops them on the
game thread when it calls `Mixer::update` each frame.

## `Send` and `Sync` checked across threads

Yowie runs work on several threads besides the game's: the audio device
callback, the asset loader pool, the music decoder, the save writer and
the background threads that service network sockets. Rust's `Send` and
`Sync` traits mark which types may move to or be shared with another
thread, and the compiler rejects code that breaks the rule.

The engine states these rules in its signatures. An asset loader and the
asset it produces must be `Send + Sync`, because loaders run on worker
threads:

```rust,no_run
use yowie::Context;
use yowie::asset::{BoxError, Handle, LoadContext, Loader};

struct Dialogue(Vec<String>);

struct DialogueLoader;

impl Loader for DialogueLoader {
    type Asset = Dialogue;

    fn load(&self, bytes: &[u8], _path: &str, _ctx: &LoadContext<'_>) -> Result<Dialogue, BoxError> {
        let text = std::str::from_utf8(bytes)?;
        Ok(Dialogue(text.lines().map(str::to_owned).collect()))
    }
}

fn start_loading(ctx: &mut Context) -> Handle<Dialogue> {
    ctx.assets.register(DialogueLoader);
    ctx.assets.load::<Dialogue>("intro.txt")
}
```

If `Dialogue` held an `Rc`, which is not `Send`, this would not compile.
In the other direction, `Context` is not `Send`, so the compiler stops a
game from moving it into a spawned thread, and the renderer, input and
window are only touched from the main thread. A thread that needs to wake the
loop takes a `Waker`, which is `Send` and `Sync`. The
[API design guide](api-design.md#threads-and-send) lists which types are
`Send`.

## `Result` and error enums

Every call that can fail returns a `Result`. Each Yowie crate defines one
error enum, so a caller can match on the cause: `GfxError::NoAdapter`
means the machine has no usable GPU driver, and `GfxError::Shader`
carries the compiler's message with line numbers in the game's own
source. Game callbacks return `GameResult`, and the `?` operator converts
any crate's error into it. An error ends the loop and comes back from
`run` as a value the game's `main` can inspect. The
[API design guide](api-design.md#errors) shows the patterns.

## The crates Yowie builds on

Yowie uses established crates for platform access, file formats and
solvers, and adds the game-facing layer on top.

| Crate | Used for |
|---|---|
| winit | windows, keyboard, pointer and window events on Wayland, X11, macOS and Windows |
| wgpu | the GPU on Vulkan, Metal and DX12; its naga compiler checks and translates WGSL |
| cosmic-text, swash | text shaping, layout, fallback and glyph rasterisation |
| lyon | tessellating vector paths into triangles |
| rapier2d, rapier3d | rigid bodies, colliders, joints and queries |
| hecs | entity storage and queries under `ecs::World` |
| cpal | audio output and capture devices |
| symphonia | decoding WAV, Ogg Vorbis, MP3 and FLAC |
| rustls | TLS for networking |
| gltf | glTF and GLB models, skins and animation |
| gilrs | gamepads |
| glam | vectors, matrices and quaternions |
| image, exr, ktx2 | PNG, JPEG, Radiance HDR, OpenEXR and KTX2 images |
| serde, serde_json, postcard | saves, scene documents, emitters, settings and network messages |
| notify | watching asset directories for hot reload |

`[workspace.dependencies]` in the root `Cargo.toml` pins every version in
one place. Yowie keeps most of these types out of its own API: rapier's
types do not appear in `yowie::phys`, and games draw with sprites, meshes
and cameras rather than wgpu calls. Two layers stay visible on purpose:
`ecs::World` dereferences to `hecs::World` for queries, and `yowie::gfx`
re-exports `wgpu` for games that record their own GPU work.

Yowie's own crates contain two `unsafe` blocks. Both build engine shader
modules with wgpu's runtime bounds checks turned off; game shaders keep
the checks.

## Tradeoffs

### Compile times

Rust compiles every dependency from source, and a Yowie game depends on
more than 300 crates on one platform. The first build of a game compiles
all of them. Later builds recompile only the crates that changed, which
for a game is usually its own crate.

Optimisation level matters more for a game than for most programs. An
unoptimised build (`opt-level = 0`) runs the renderer, physics and audio
decoding too slowly to play. Yowie's `Cargo.toml` sets:

```toml
[profile.dev]
# Games are unplayable at opt-level 0; optimise dependencies even in debug builds.
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

Dependencies are compiled once at full optimisation and then cached;
your own crate rebuilds at `opt-level = 1`, which keeps rebuilds short
and the game fast enough to play with debug assertions on. Cargo applies
profiles from the top-level `Cargo.toml` only, so copy these lines into
your game's `Cargo.toml` ([getting started](getting-started.md#create-a-project)).

To shorten builds further, turn off the cargo features you do not use.
Without `phys` the build skips rapier, and without `net` it skips rustls
and ring.

### Patterns the borrow checker leads to

Rust allows one mutable reference to a value or any number of shared
ones at a time, never both. Game objects that point at each other (a missile and
its target, a sprite and its parent) do not fit that rule as plain
references, so Yowie's API uses three patterns instead.

**Handles and IDs.** Objects refer to each other by copyable IDs:
`ecs::Entity`, the body IDs of the physics worlds, `WindowId`, the
`DrawId` a 3D draw returns, `asset::Handle<T>`. An ID is checked when it
is used, and a stale one gives an error or `None` rather than a dangling
pointer:

```rust
use yowie::ecs::{Entity, World};

struct Health(i32);
struct Target(Entity);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::new();
    let goblin = world.spawn((Health(5),));
    let archer = world.spawn((Target(goblin),));

    let target = world.get::<&Target>(archer)?.0;
    if let Ok(mut health) = world.get::<&mut Health>(target) {
        health.0 -= 2;
    }

    world.despawn(goblin)?;
    assert!(world.get::<&Health>(goblin).is_err());
    Ok(())
}
```

**The entity component system.** Game state that many systems read and
write fits in an `ecs::World`. Systems are functions that take
`&mut World` and run in a `Schedule`; spawns and despawns made while a
query holds the world go through `Commands` and apply afterwards. The
[entities and systems guide](ecs.md) covers it.

**The context passed into callbacks.** Yowie has no global engine object.
Each callback receives `&mut Context` for the length of the call, and
game code reaches the renderer, input and audio only through it. Its
subsystems are public fields, so one statement can borrow `ctx.gfx`
mutably and `ctx.input` immutably at the same time. Scoped
state (a camera, a render target, a shader) is a closure argument, so the
previous state comes back when the closure ends.

### The CPU cost of a portable GPU layer

wgpu validates and tracks every resource a draw uses, so each draw costs
more CPU time than the same draw recorded on the native API. These
figures compare the CPU time to record and submit 10,000 draws with wgpu
30.0.1 and with raw Metal, on an Apple M2:

| Case | wgpu CPU time against Metal |
|---|---|
| draws that share state | 1.3 to 2 times |
| state changes on every draw | 3 to 5 times |

At 10,000 draws that each bind their own object, wgpu's recording takes
about 1 ms of a 16.7 ms frame.

Yowie keeps the draw count well below that. The 2D batch merges every
run of sprites, shapes and glyphs that share a texture, shader, blend
mode and view into one draw call, and sets pipelines and bind groups once
per run. The 3D renderer sorts draws and merges repeated meshes into
instanced draws, and static geometry (`gfx::scene::StaticGroup`) replays
from render bundles. The F3 overlay shows the frame's draw calls, so you
can check a scene's count while you build it.

In a debug build wgpu also turns on its validation, which adds CPU time
to every call. Measure performance in a release build, or set
`WGPU_VALIDATION=0`.

### Other limits

- **Desktop only.** The renderer uses Vulkan, Metal and DX12. There is no
  OpenGL, browser or mobile backend, and a machine without one of those
  drivers stops at start with `GfxError::NoAdapter`.
- **WGSL only.** Game shaders are WGSL, compiled by naga at run time.
- **Code changes need a rebuild.** Hot reload covers asset files:
  textures and shaders change in place, and other assets reload through
  their handles. A change to Rust code needs a rebuild and a restart.
- **The main thread.** `run` must be called from `main`, and game
  callbacks run on that thread. `Context`, `ecs::World` and `ui::Ui` are
  not `Send`, so a game that wants more cores moves self-contained work
  (pathfinding, generation, saves) to threads and brings the result back,
  rather than running the world itself elsewhere.
- **Not on crates.io.** Yowie is a git dependency, and the API reference
  comes from `cargo doc --open`.

To see the approach in practice, read [getting started](getting-started.md)
and then [API design](api-design.md).
