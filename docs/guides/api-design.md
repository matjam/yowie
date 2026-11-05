---
title: API design
group: Start
order: 4
summary: ownership, cleanup on drop, scoped state, builders and defaults, typed errors, units, threads and the context
---

# API design

The same conventions hold across every Yowie crate. This guide lists
them with a short example each, so you can predict how a call you have
not used yet behaves. Read [getting started](getting-started.md) first.

## Ownership

The game owns its state. A `Game` is an ordinary struct, and Yowie holds
no references into it. The engine owns its subsystems and lends them to
each callback as `&mut Context`; nothing in the context outlives the
call.

`Context` exposes its subsystems as public fields, so the borrow checker
lets one callback use several at once. This line borrows `ctx.gfx`
mutably and `ctx.input` immutably in the same expression:

```rust,no_run
use yowie::Context;
use yowie::gfx::Texture;

fn draw_cursor(ctx: &mut Context, cursor: &Texture) {
    ctx.gfx.draw_texture(cursor, ctx.input.mouse());
}
```

Everything else is a value the game creates and keeps: an `ecs::World`
and its `Schedule`, a `phys::World2D`, particle systems, save stores,
network clients. The game advances them from its own `update`:

```rust,no_run
use yowie::ecs::{Schedule, World};
use yowie::phys::World2D;
use yowie::{Context, Game, GameResult, ecs_glue, vec2};

struct Arena {
    world: World,
    schedule: Schedule,
    physics: World2D,
}

impl Game for Arena {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        let mut schedule = Schedule::new();
        ecs_glue::add_animation(&mut schedule);
        Ok(Arena {
            world: World::new(),
            schedule,
            physics: World2D::new(vec2(0.0, 9.81)),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.physics.step(ctx.time.delta());
        ecs_glue::set_delta(&mut self.world, ctx);
        self.schedule.run(&mut self.world);
        ctx.record_schedule(&self.schedule);
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}
```

Game types need no lifetime parameters. Handles to engine objects
(textures, meshes, sounds, asset handles) are owned values, not borrows.

## GPU handles are freed on drop

`Texture`, `Shader`, `Font`, `RenderTexture`, `Mesh`, `Model` and
`Environment` are reference-counted handles. Cloning one shares the GPU
object. The object is freed when the last clone and every queued draw
that uses it are gone, so there is no `destroy` call and no ordering rule
to follow:

```rust,no_run
use yowie::gfx::{Texture, TextureOptions};
use yowie::{Context, GameResult};

struct Level {
    tiles: Texture,
}

fn load_level(ctx: &mut Context, path: &str) -> GameResult<Level> {
    let tiles = ctx.load_texture(path, TextureOptions::default())?;
    Ok(Level { tiles })
}

fn next_level(ctx: &mut Context, level: &mut Level) -> GameResult {
    // The old level's texture is freed once nothing holds or draws it,
    // even if a draw that uses it was queued earlier this frame.
    *level = load_level(ctx, "level-2.png")?;
    Ok(())
}
```

A handle keeps its identity when its contents change. Hot reload,
`Texture::replace_data` and `Shader::reload_source` swap the GPU object
inside the handle, so every clone, sprite and material that uses it shows
the new contents.

Audio follows the same rule. A `Sound` shares its samples between clones.
The audio thread does not free memory in steady state. Finished voices
are sent back and dropped on the game thread when the engine calls
`Mixer::update`. Two handles differ from the GPU ones. Dropping a `Voice` does not stop the
sound (call `Voice::stop`), and dropping an `audio::Output` stops the
device, which is why the engine keeps the one behind `ctx.audio`.

## Scoped state through closures

To change drawing state for a block of drawing, pass the block as a
closure. The state returns to what it was when the closure ends.

| Method | Scope |
|---|---|
| `Graphics::with_camera` | draws through a `Camera2D` |
| `Graphics::with_layer`, `with_blend`, `with_clip`, `with_transform` | a layer, blend mode, clip rectangle or transform |
| `Graphics::with_shader` | draws through a game shader |
| `Graphics::draw_to` | draws into a `RenderTexture` instead of the screen |
| `Graphics::draw_in` | draws into an extra window |
| `Graphics::scene` | records a 3D scene seen through a `Camera3D` |
| `Ui::draw`, `Frame::panel`, `Frame::window` | builds the interface and its containers |

```rust,no_run
use yowie::gfx::{Camera2D, RenderTexture, Shader, Sprite, Texture};
use yowie::{Color, Context, Vec2, vec2};

fn draw_world(
    ctx: &mut Context,
    map: &Texture,
    minimap: &RenderTexture,
    glow: &Shader,
    player: Vec2,
) {
    let camera = Camera2D::new(player).with_zoom(2.0);
    ctx.gfx.with_camera(&camera, |gfx| {
        gfx.draw_texture(map, Vec2::ZERO);
        gfx.with_shader(glow, |gfx| {
            gfx.draw_sprite(map, &Sprite::at(player).size(vec2(16.0, 16.0)).centered());
        });
    });
    ctx.gfx.draw_to(minimap, Some(Color::BLACK), |gfx| {
        gfx.draw_sprite(map, &Sprite::at(Vec2::ZERO).size(vec2(128.0, 128.0)));
    });
    ctx.gfx.draw_texture(minimap.texture(), vec2(8.0, 8.0));
}
```

Inside `draw_to`, the layer, sort key, blend mode, shader, camera and
clip start at their defaults. Each `with_*` method has a `set_*` partner
(`set_camera`, `set_layer`, `set_shader` and so on) for state that should
last until the game changes it again.

## Builders and defaults

Option structs have public fields and a `Default` that suits the common
case. Set the fields you care about with struct update syntax:

| Type | Default |
|---|---|
| `Config` | a 1280 by 720 resizable window, vsync, 60 updates per second |
| `TextureOptions` | nearest filtering, clamped edges, no mipmaps, sRGB colour (pixel art) |
| `Sprite` | the whole texture at its own size, unscaled, unrotated, untinted |
| `TextStyle` | white, 16 view units, left-aligned, unwrapped |
| `Camera2D` | centred on the origin at zoom 1 |
| `PostSettings` | exposure 1 and the ACES tone curve, every effect off |
| `audio::PlayOptions` | unity gain, centred, not looping, normal pitch |

Constructors take what has no useful default (`Config::new(title)`,
`Sprite::at(position)`, `TextStyle::new(size)`, `Camera2D::new(position)`,
`Material::color(base, metallic, roughness)`). Builder methods take the
value and return the changed copy. Most are named `with_*`
(`Config::with_size`, `Camera2D::with_zoom`,
`DirectionalLight::with_shadows`); `Sprite` and `TextStyle` use the field
name (`Sprite::tint`, `TextStyle::color`).

```rust
use yowie::gfx::{Camera2D, Sprite, TextStyle, TextureOptions};
use yowie::{Color, Config, vec2};

fn main() {
    let sprite = Sprite::at(vec2(100.0, 50.0))
        .centered()
        .rotation(0.5)
        .tint(Color::RED);
    assert_eq!(sprite.scale, vec2(1.0, 1.0));

    let camera = Camera2D::new(vec2(0.0, 0.0)).with_zoom(2.0);
    assert_eq!(camera.rotation, 0.0);

    let style = TextStyle::new(24.0).color(Color::YELLOW).width(300.0);
    assert_eq!(style.line_height, 1.25);

    let options = TextureOptions {
        mipmaps: true,
        ..TextureOptions::default()
    };
    assert!(!options.data);

    let config = Config {
        vsync: false,
        ..Config::new("Benchmark")
    };
    assert!(config.resizable);
}
```

## Errors

Each crate that can fail has one error enum, and most have a `Result`
alias that uses it:

| Crate | Error |
|---|---|
| `yowie` | `yowie::Error`, returned by `run` and the `Context` loaders |
| `gfx` | `GfxError` |
| `asset`, `audio`, `ecs`, `net`, `tiled` | `Error` in each module |
| `anim`, `input`, `locale`, `orbit`, `particle`, `phys`, `rng`, `save` | `AnimError`, `InputError`, `LocaleError`, `OrbitError`, `ParticleError`, `PhysError`, `RngError`, `SaveError` |

Game callbacks return `GameResult<T = ()>`, a `Result` whose error is
`BoxError` (`Box<dyn std::error::Error + Send + Sync>`). The `?` operator
converts every crate's error into it, and so does `.into()` on a string:

```rust
use yowie::GameResult;
use yowie::rng::{Dice, Rng};

fn roll(text: &str, rng: &mut Rng) -> GameResult<i64> {
    let dice: Dice = text.parse()?;
    let total = dice.roll(rng);
    if total < 0 {
        return Err(format!("{text} rolled below zero").into());
    }
    Ok(total)
}

fn main() {
    let mut rng = Rng::new(7);
    assert!(roll("2d6+1", &mut rng).is_ok());
    assert!(roll("two dice", &mut rng).is_err());
}
```

An error returned from `init`, `update`, `draw` or `event` ends the loop,
and `run` returns it as `Error::Game`. To act on one kind of error, match
the enum or downcast the boxed error:

```rust,no_run
use yowie::gfx::GfxError;
use yowie::{Config, Context, Error, Game, GameResult};

struct Empty;

impl Game for Empty {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Empty)
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() {
    match yowie::run::<Empty>(Config::new("Errors")) {
        Ok(()) => {}
        Err(Error::Gfx(GfxError::NoAdapter(why))) => {
            eprintln!("this machine has no Vulkan, Metal or DX12 driver: {why}");
        }
        Err(Error::Game(e)) if e.downcast_ref::<GfxError>().is_some() => {
            eprintln!("a renderer call in the game failed: {e}");
        }
        Err(e) => eprintln!("{e}"),
    }
}
```

Library code returns errors for bad input rather than panicking. Work
that runs later reports through the log instead: a hot-reloaded shader
that fails to compile logs the error and keeps the previous program
running.

## Units

| Quantity | Unit |
|---|---|
| 2D positions and sizes | view units: logical points of the window, or the units of a fixed view (`Config::with_view`); origin at the top left, +Y down |
| pointer | `InputState::mouse` in view units; `InputState::mouse_window` in window pixels |
| window size, `Config::width` and `height` | logical points (pixels in a headless run) |
| texture sizes, sprite source rectangles | texels |
| 2D angles | radians, clockwise on screen |
| 3D positions | world units, right-handed, +Y up |
| 3D angles, field of view | radians |
| time | seconds: `Time::delta` as `f32`, `Time::elapsed` as `f64`; timers and tweens take seconds too |
| colours given to the API | `Color`: sRGB, components `0..=1` (above 1 for HDR emission), straight alpha |
| colours inside the renderer | linear light, premultiplied alpha; textures are stored premultiplied |
| audio | gain (1 is unity), pitch as a rate (2 is an octave up), filter cutoffs in Hz, fades in seconds |
| physics | the game's own units, kept consistent; `World2D::default()` and `World3D::default()` pull towards -Y |

Lighting and blending happen in linear light, so a colour's linear value
differs from its sRGB components. `Color` converts in both directions:

```rust
use yowie::Color;

fn main() {
    let orange = Color::rgb8(255, 128, 0);
    let [r, g, _b, _a] = orange.to_linear();
    assert!((r - 1.0).abs() < 1e-6);
    // 128 of 255 in sRGB is about 0.216 in linear light.
    assert!((g - 0.216).abs() < 0.001);

    let half = orange.with_alpha(0.5).to_linear_premultiplied();
    assert_eq!(half[3], 0.5);
    assert!((half[0] - 0.5).abs() < 1e-6);
}
```

A game shader receives the tint and the sampled texture already in
linear light and premultiplied, and returns premultiplied linear colour.

`World2D::default()` pulls towards -Y, which is up the screen in 2D
drawing. A 2D game that draws bodies at their physics positions creates
its world with `World2D::new(vec2(0.0, 9.81))` instead.

## Immediate-mode submission

Drawing calls record work; they do not touch the GPU. During `draw`, every
sprite, shape, glyph, 3D draw and widget is appended to the frame. When
`draw` returns, the engine renders the frame: 2D draws sorted by layer,
then sort key, then call order, and batched into one draw call per run
that shares a texture, shader, blend mode and view; 3D draws culled,
sorted and merged into instanced runs.

Record in `draw` only. The engine starts each frame by dropping anything
queued since the last one, so drawing calls made in `update` show
nothing.

Sort keys order draws within a layer. A top-down game sets each sprite's
key to its foot position so that nearer sprites cover farther ones,
whatever order the game draws them in:

```rust,no_run
use yowie::gfx::{Sprite, Texture};
use yowie::{Context, Vec2, vec2};

fn draw_actors(ctx: &mut Context, sheet: &Texture, feet: &[Vec2]) {
    ctx.gfx.with_layer(10, |gfx| {
        for &at in feet {
            gfx.set_sort_key(at.y);
            gfx.draw_sprite(sheet, &Sprite::at(at).origin(vec2(0.5, 1.0)));
        }
        gfx.set_sort_key(0.0);
    });
}
```

Recording captures values at the call where it matters:

- A draw keeps the shader uniforms it was queued with.
  `Shader::set_uniforms` affects later draws only.
- `Texture::update_pixels` changes the texture for the whole frame,
  including draws queued before the call.
- A 3D scene's settings (`Scene::set_sun`, `set_sky`, `set_post` and the
  rest) apply to the whole scene, whatever the order of the calls.

## Threads and `Send`

`run` and every game callback run on the main thread. The engine runs
other work on its own threads: the audio callback, the asset loader pool,
the music decoder, the save writer and the network sockets. Types that
cross those threads are checked by the compiler:

| `Send` and `Sync` | Not `Send` |
|---|---|
| `gfx::Graphics`, `Texture`, `Shader`, `Font`, `RenderTexture`, `Mesh`, `Model`, `Material`, `Environment` | `Context` |
| `audio::Mixer`, `Sound`, `Voice`, `Bus`, `Music` | `ecs::World` |
| `asset::Assets`, `asset::Handle<T>` | `ui::Ui` |
| `phys::World2D`, `phys::World3D`, `save::Store`, `rng::Rng` | `console::Console` |
| `Waker`, `yowie::Error`, `input::InputState`, `Time` | |

The bounds that matter when you extend the engine:

- An asset `Loader` is `Send + Sync`, and its asset type is too, because
  loaders run on worker threads.
- A network `set_waker` callback is `Arc<dyn Fn() + Send + Sync>`; it runs
  on the socket's thread.
- An audio `Stream` is `Send`; it runs on the audio thread.
- `BoxError` is `Send + Sync`, so an error can come back from a worker.

To finish work on another thread and run a turn when it is done, take a
`Waker` from the context. It is the one handle that wakes a sleeping
turn-based loop from outside:

```rust,no_run
use std::sync::mpsc::{self, Receiver};
use std::thread;

use yowie::Context;

fn start_search(ctx: &Context) -> Receiver<u64> {
    let waker = ctx.waker();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let answer: u64 = (1..=1_000_000).sum();
        let _ = tx.send(answer);
        waker.wake();
    });
    rx
}
```

The game then checks the receiver with `try_recv` in `update`.

## The context and the subsystems

| Field | Type | Crate | Feature |
|---|---|---|---|
| `ctx.gfx` | `gfx::Graphics` | `yowie-gfx` | always |
| `ctx.input` | `input::InputState` | `yowie-input` | always |
| `ctx.time` | `Time` | `yowie` | always |
| `ctx.assets` | `asset::Assets` | `yowie-asset` | always |
| `ctx.audio` | `audio::Mixer` | `yowie-audio` | `audio` |
| `ctx.ui` | `ui::Ui` | `yowie-ui` | `ui` |
| `ctx.console` | `console::Console` | `yowie-console` | `console` |

`Context` methods cover what needs the window or several subsystems at
once:

| Methods | Purpose |
|---|---|
| `quit`, `request_redraw`, `wake_after`, `waker` | end the loop or run a turn |
| `set_title`, `set_window_size`, `set_fullscreen`, `set_cursor`, `set_cursor_grab` and the rest | control the window ([the window](window.md)) |
| `open_window`, `close_window`, `windows`, `window_info` | extra windows |
| `load_texture`, `load_texture_async`, `load_shader`, `load_shader_with_uniforms`, `load_font`, `load_sound`, `load_map` | load textures, shaders, fonts, sounds and maps from the assets |
| `asset_events` | the asset loads, changes and failures of this frame |
| `record_schedule`, `schedule_stats` | show an ECS schedule's timings in the console |
| `headless`, `focused`, `input_captured`, `set_debug_overlay` | engine state |

The `load_*` methods exist on the context, not on `ctx.assets`, because
they read a file through the asset store and create a GPU object through
the renderer. While hot reload is on, a texture or shader loaded this way
follows its file.

For the full API, run `cargo doc --open`.
