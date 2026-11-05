---
title: Introduction
group: Start
order: 1
summary: what Yowie is, how it is put together, how a frame runs, and what to read next
---

# Introduction

Yowie is a Rust game library for real-time and turn-based games that put
2D sprites and 3D models on one screen. One renderer draws sprites,
shaped text, vector paths and tilemaps alongside physically based 3D
scenes with shadows, sky and post-processing, all in the same frame.
Around it are an entity component system, 2D and 3D rigid-body physics
with cloth, soft bodies and fluids, skeletal animation, an immediate-mode
interface, a debug console, an audio mixer with positional sound and a
tracker player, celestial mechanics, and the services a finished game
needs: assets and pack files, saves, translation, random numbers, timers,
tweens, grid pathfinding and networking.

A game is a type that implements the `Game` trait. Yowie owns the window
and the loop, and calls the game's `update` and `draw` methods with a
`Context` that holds the renderer, input, time, audio and assets.

## Design

- **Plain `cargo build`.** Yowie builds with Cargo on Linux (native
  Wayland, with X11 as the fallback), macOS and Windows. It draws with
  wgpu over Vulkan, Metal and DX12, and shaders are WGSL.
- **Two loop modes.** A real-time game runs a fixed step. A turn-based
  game sets `LoopMode::TurnBased`, and the process then sleeps in the
  operating system until the player does something.
- **2D and 3D in one frame.** A 3D scene renders into its own HDR image,
  is tone mapped into its viewport, and 2D drawing goes over it. A scene
  can also render into a texture for a minimap or a portrait.
- **Immediate-mode submission.** Sprites, text, 3D draws and interface
  widgets are recorded during `draw` every frame. The renderer sorts,
  batches and instances them when the frame ends.
- **The game owns its state.** Yowie keeps no scene graph of its own. A
  game holds its state in its own types, or in an `ecs::World` that it
  creates and runs.
- **One crate per subsystem.** Each part is its own crate, re-exported as
  a module of `yowie`. Cargo features leave out audio, physics,
  networking, the interface, the console, particles and Tiled maps.
- **Headless verification.** Any game can run without a window, render
  under a fixed clock and write its last frame to a PNG. The examples are
  checked this way against stored images.

## The shape of a game

This program spins a lit cube and writes text over it:

```rust,no_run
use yowie::gfx::TextStyle;
use yowie::gfx::scene::{Camera3D, DirectionalLight, Material, Mesh, MeshData};
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Quat, Transform, Vec3, vec2};

struct Spinner {
    cube: Mesh,
    angle: f32,
}

impl Game for Spinner {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let cube = Mesh::new(&ctx.gfx, &MeshData::cube(1.0))?;
        Ok(Spinner { cube, angle: 0.0 })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.angle += 1.5 * ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let camera = Camera3D::perspective(Vec3::new(0.0, 1.5, 4.0), Vec3::ZERO, 0.9);
        let material = Material::color(Color::ORANGE, 0.0, 0.5);
        let placement = Transform::from_rotation(Quat::from_rotation_y(self.angle));
        ctx.gfx.scene(&camera, |s| {
            s.set_sun(DirectionalLight::default());
            s.draw(&self.cube, &material, placement);
        });
        let font = ctx.gfx.default_font();
        let style = TextStyle::new(24.0);
        ctx.gfx.draw_text(&font, "2D text over a 3D scene", vec2(16.0, 16.0), &style);
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Spinner>(Config::new("Spinner"))
}
```

`init` builds the game once the window and renderer exist. `update`
advances the simulation by `ctx.time.delta()` seconds. `draw` records the
frame. Each returns a `GameResult`, and an error from any of them ends
`run`, which returns it.

## How it is put together

The root crate `yowie` holds the loop, `Config`, `Game`, `Context` and
`Time`, and re-exports the other crates as modules. The maths, colour and
geometry types of `yowie-core` are re-exported at the root, so
`yowie::Vec2` and `yowie::Color` need no module path.

| Crate | Path | What it does |
|---|---|---|
| `yowie` | `yowie` | `run`, `Config`, `Game`, `Context`, `Time`, the loop, headless runs, window controls, extra windows, `FlyCamera` and `OrbitCamera` |
| `yowie-core` | `yowie` (root) | glam vectors, matrices and quaternions, `Color`, `Rect`, `Transform`, `Transform2D` |
| `yowie-input` | `yowie::input` | keys, mouse buttons, gamepads, the per-update `InputState`, rebindable `ActionMap`s |
| `yowie-gfx` | `yowie::gfx` | the wgpu renderer: sprites, shapes, nine-slices, shaped text, vector paths, tilemaps and autotiling, 2D lights, render textures, 2D shaders; `gfx::scene` for physically based 3D, glTF models, shadows, sky, terrain, water and post-processing |
| `yowie-ui` | `yowie::ui` | immediate-mode widgets, layout, themes, skins, keyboard and gamepad navigation |
| `yowie-console` | `yowie::console` | the in-game console: commands, variables, key bindings, log capture and debug panels |
| `yowie-ecs` | `yowie::ecs` | entity component system over hecs: systems, resources, events, hierarchy, saves, prefabs and scene documents |
| `yowie-phys` | `yowie::phys` | 2D and 3D rigid bodies, joints, queries and character controllers on rapier; cloth, soft bodies and 2D fluids in `phys::soft` |
| `yowie-anim` | `yowie::anim` | curves, clips and players, flipbooks, skeletons, blend spaces, root motion, IK and morph weights |
| `yowie-audio` | `yowie::audio` | the mixer: buses, streamed decoding, positional and binaural voices, reverb, capture, synthesis and a MOD, S3M, XM and IT player |
| `yowie-particle` | `yowie::particle` | particle emitters described in JSON, simulated on the CPU or on the GPU |
| `yowie-tiled` | `yowie::tiled` | maps from the Tiled editor in TMX and TMJ |
| `yowie-asset` | `yowie::asset` | directories and pack files behind one file system, typed loaders, background loading and hot reload |
| `yowie-save` | `yowie::save` | JSON save slots and settings in the platform's data directory, with atomic writes and migrations |
| `yowie-locale` | `yowie::locale` | translated string tables with placeholders, plural rules and fallback languages |
| `yowie-rng` | `yowie::rng` | seeded PCG32 random numbers, dice, weighted picks and shuffles |
| `yowie-timer` | `yowie::timer` | timers and cutscene sequences on game time |
| `yowie-tween` | `yowie::tween` | easing curves and tweens |
| `yowie-grid` | `yowie::grid` | cell grids, A*, Dijkstra maps, lines, field of view and flood fill |
| `yowie-orbit` | `yowie::orbit` | orbital elements, Kepler propagation, N-body integration and ships under thrust |
| `yowie-net` | `yowie::net` | typed messages over TCP (with TLS) and UDP (with reliable channels), prediction, interpolation and snapshot deltas |
| `yowie-ktx2` | (used by the renderer) | the KTX2 reader and writer shared by the renderer and the tools |
| `yowie-tools` | (binaries) | `yowie-tex`, `yowie-info`, `yowie-bundle` and `yowie-play` |

The `audio`, `phys`, `net`, `ui`, `console`, `particle` and `tiled` cargo
features control the modules of the same names. All of them are on by
default. `yowie-asset` also builds the `yowie-pack` tool.

## The context

The `Context` passed to every callback has one public field per engine
subsystem:

| Field | Type | Holds |
|---|---|---|
| `ctx.gfx` | `gfx::Graphics` | the renderer: drawing, textures, fonts, shaders, 3D scenes |
| `ctx.input` | `input::InputState` | keys, pointer, typed text and gamepads |
| `ctx.time` | `Time` | the update step, game time, frame counts, time scale and pause |
| `ctx.assets` | `asset::Assets` | the asset file system and loaders |
| `ctx.audio` | `audio::Mixer` | the mixer (`audio` feature) |
| `ctx.ui` | `ui::Ui` | the immediate-mode interface (`ui` feature) |
| `ctx.console` | `console::Console` | the debug console (`console` feature) |

Its methods control the window and the loop, and load textures, shaders,
fonts and sounds from the assets. The [API design guide](api-design.md#the-context-and-the-subsystems)
lists them.

## How a frame runs

In a fixed-step loop each displayed frame runs these steps in order:

1. Window and input events arrive. Each one is applied to `ctx.input`
   and passed to `Game::event` (or `Game::window_event` with its window)
   before the next update. While the debug console is open, input events
   go to the console instead.
2. The engine collects asset events and reloads, in place, the textures
   and shaders whose files changed.
3. The updates the frame owes run, from zero to
   `Config::max_updates_per_frame`. Each calls `Game::update` with
   `ctx.time.delta()` set to the step, then ends that update's input
   edges and adds the step to `ctx.time.elapsed()`.
4. Windows closed during the updates are removed. The mixer drops the
   voices that finished; with no audio device, the engine first renders
   the mixer for the game time that passed.
5. `Game::draw` records the frame. The engine then adds the frame-timing
   overlay and the console.
6. The renderer renders what was recorded: each extra window's frame,
   then the main window's. Within a window's frame, every render texture
   drawn to renders first, in the order its `draw_to` scope finished,
   then the window itself. Each of these outputs runs three passes:
   1. **3D scene passes.** Each scene recorded with `Graphics::scene`
      renders its shadow maps, then draws into its own HDR colour and
      depth images: opaque and cut-out surfaces, the sky, refracting
      surfaces, then blended surfaces.
   2. **Post-processing.** Each scene's chain (exposure, tone curve and
      any effects in its `PostSettings`) writes the HDR image into the
      scene's viewport of the output.
   3. **2D overlay.** The 2D batch draws over whatever the scenes left,
      or over the clear colour. It is sorted by layer, then sort key,
      then call order, and runs that share a texture, shader, blend mode
      and view become one draw call. The interface, the console and the
      overlay draw on the highest layers (`gfx::layers::UI`,
      `gfx::layers::CONSOLE`, `gfx::layers::DEBUG`).

A turn-based loop runs the same steps, with one update per turn instead
of the fixed-step count. A hidden or minimised window runs the updates
and skips the drawing.

## Verify a game without a window

Every Yowie program reads two flags and one environment variable, so it
can check itself on a build machine:

| Flag | Effect |
|---|---|
| `--shot <file.png>` | runs with no window, renders under the fixed clock, writes the last frame |
| `--seconds <n>` | runs `n` seconds of game time, then exits |
| `YOWIE_FIXED_CLOCK=1` | advances time by one fixed step per frame, so a frame is the same picture on every machine |

```bash
cargo run --example tetris -- --autoplay --seconds 12 --shot tetris.png
```

`cargo test --test examples` runs each example this way and compares its
frame with the stored image in `examples/testdata/`. The renderer's own
tests create a headless GPU device, read pixels back, and skip with a
message on a machine with no GPU adapter.

## What to read next

The guides come in five groups. Read Start first, then the group you
need.

| Group | Guides |
|---|---|
| Start | [why Rust](why-rust.md), [getting started](getting-started.md), [API design](api-design.md), [building Tetris](tetris.md) |
| Engine | [the window](window.md), [input](input.md), [entities and systems](ecs.md), [game services](services.md), [the debug console](console.md) |
| Graphics | [2D graphics](graphics-2d.md), [3D graphics](graphics-3d.md), [shaders](shaders.md), [animation](animation.md), [the interface](ui.md) |
| Audio | [audio](audio.md) |
| Simulation | [physics](physics.md), [orbits and space](space.md) |

To try the engine before reading further, clone the repository and run
`cargo run --example gallery`, `cargo run --example tiles` or
`cargo run --example lighting`. For the API reference, run
`cargo doc --open`.
