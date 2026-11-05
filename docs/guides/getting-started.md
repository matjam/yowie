---
title: Getting started
group: Start
order: 3
summary: install the toolchain and system packages, open a window, learn the loop and the window controls
---

# Getting started

This guide installs Rust and the system packages Yowie needs, builds a
first game, and then covers the loop, the configuration, the keys the
engine handles, and running a game without a window.

## Install Rust

To install Rust, use [rustup](https://rustup.rs). On macOS and Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows, download and run `rustup-init.exe` from the same site. It
offers to install the Visual Studio C++ build tools, which Rust needs on
Windows.

Yowie needs Rust 1.95 or later. The 2024 edition alone needs 1.85, but
some dependencies need 1.95 (`parking_lot` 0.13 declares
`rust-version = "1.95"`). With an older toolchain, Cargo stops and names
the dependency that needs a newer one. To check the toolchain and update
it:

```bash
rustc --version
rustup update stable
```

## Install the system packages

Every platform needs a C compiler, because `zstd` (pack files and
compressed maps) and `ring` (TLS in networking) compile C code. Linux
also needs development files for audio, gamepads and the window system.

| Platform | Install |
|---|---|
| macOS | The Xcode Command Line Tools: `xcode-select --install`. |
| Windows | The Visual Studio C++ build tools, which rustup offers to install. |
| Debian, Ubuntu | `sudo apt-get install build-essential pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev` |
| Fedora | `sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel systemd-devel wayland-devel libxkbcommon-devel` |

What each Linux package is for:

| Package (Debian name) | Needed by |
|---|---|
| `pkg-config` | the build scripts that find the libraries below |
| `libasound2-dev` | audio output and capture (ALSA, through cpal) |
| `libudev-dev` | gamepads (through gilrs) |
| `libwayland-dev` | the native Wayland window (through winit) |
| `libxkbcommon-dev` | keyboard layouts on Wayland and X11 |

The renderer draws with Vulkan on Linux and Windows, Metal on macOS and
DX12 on Windows. On Linux, install the Vulkan driver for your GPU:
`mesa-vulkan-drivers` covers AMD and Intel, and NVIDIA's driver includes
Vulkan. A machine with no usable driver fails at start with
`GfxError::NoAdapter`.

On Linux the window is a native Wayland window when the session runs
Wayland, and an X11 window otherwise.

## Create a project

To start a game, create a binary crate:

```bash
cargo new my-game
cd my-game
```

Add Yowie to `Cargo.toml`. The crate is not published on crates.io, so
the dependency names the repository:

```toml
[dependencies]
yowie = { git = "https://github.com/matjam/yowie" }
```

Add these profiles to the same `Cargo.toml`:

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

Cargo reads profiles from the top-level `Cargo.toml` only, so Yowie's own
profile settings do not reach your game. Without these lines a debug
build compiles the renderer, physics and audio decoding at
`opt-level = 0`, and games run too slowly to play. With them,
dependencies are fully optimised and your own code is lightly optimised,
which keeps your rebuilds short.

The default cargo features build every part of Yowie. To leave parts out,
turn the defaults off and list the features you want. The
[README](../../README.md#cargo-features) lists what each feature turns on.

```toml
[dependencies]
yowie = { git = "https://github.com/matjam/yowie", default-features = false, features = ["audio", "ui"] }
```

## A first game

Replace `src/main.rs` with this program. It opens a window, moves a square
with the arrow keys, writes a line of text and quits on Escape.

```rust,no_run
use yowie::gfx::TextStyle;
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, vec2};

struct FirstGame {
    position: Vec2,
}

impl Game for FirstGame {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(FirstGame {
            position: vec2(200.0, 200.0),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let step = 240.0 * ctx.time.delta();
        if ctx.input.key_down(Key::Left) {
            self.position.x -= step;
        }
        if ctx.input.key_down(Key::Right) {
            self.position.x += step;
        }
        if ctx.input.key_down(Key::Up) {
            self.position.y -= step;
        }
        if ctx.input.key_down(Key::Down) {
            self.position.y += step;
        }
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let square = Rect::new(self.position.x, self.position.y, 48.0, 48.0);
        ctx.gfx.fill_rect(square, Color::ORANGE);
        let font = ctx.gfx.default_font();
        let style = TextStyle::new(20.0);
        ctx.gfx.draw_text(&font, "Arrow keys move. Escape quits.", vec2(16.0, 16.0), &style);
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<FirstGame>(Config::new("First game").with_size(960, 600))
}
```

Run it:

```bash
cargo run
```

`yowie::run` opens the window, creates the renderer, the audio mixer and
the asset store, and calls `Game::init` to build the game. It then runs
the loop until the game calls `Context::quit`, the player closes the
window, or a callback returns an error, which `run` returns. Call `run`
from `main`: window systems deliver events to the thread that opened the
connection, and on macOS that must be the main thread.

A game implements `init` and `draw`. `update`, `event` and
`window_event` have default bodies that do nothing (the default
`window_event` passes the main window's events to `event`).

2D drawing coordinates are view units with the origin at the top left
and +Y down. Without a fixed view, one view unit is one logical point of
the window, so a 48 unit square is the same size on a standard and a
high-density display. Angles are radians, clockwise on screen. Colours
are sRGB. The [API design guide](api-design.md#units) lists every unit.

The [2D graphics guide](graphics-2d.md) covers sprites, text and shapes,
and the [input guide](input.md) covers keys, the mouse, gamepads and
action maps.

## Load an image

To load an image, put it in an `assets` directory beside `Cargo.toml`
and call `Context::load_texture` with its path inside that directory.
Load in `init` and keep the texture in the game.

```rust,no_run
use yowie::gfx::{Sprite, Texture, TextureOptions};
use yowie::{Context, GameResult, vec2};

fn load_player(ctx: &mut Context) -> GameResult<Texture> {
    let texture = ctx.load_texture("player.png", TextureOptions::default())?;
    Ok(texture)
}

fn draw_player(ctx: &mut Context, player: &Texture) {
    let sprite = Sprite::at(vec2(320.0, 240.0)).centered();
    ctx.gfx.draw_sprite(player, &sprite);
}
```

The asset store reads the `assets` directory in the working directory and
the one beside the executable, so `cargo run` finds it. Textures load
from PNG, JPEG, Radiance HDR and KTX2 files. `TextureOptions::default()`
samples the nearest texel, which keeps pixel art sharp;
`TextureOptions::smooth()` filters linearly and builds mipmaps.

In debug builds `Config::hot_reload` is on, so when you save a new
version of `player.png`, the texture the game holds shows the new image
without a restart. The [game services guide](services.md) covers asset
directories, pack files and background loading.

## The loop

`Config::loop_mode` picks one of two loops.

### Fixed step

`LoopMode::FixedStep { hz }` is the default, at 60 updates per second.
`update` runs `hz` times per second of wall time and `draw` runs once per
displayed frame. `ctx.time.delta()` is the fixed step in seconds, so the
simulation gives the same result at any frame rate.

When a frame takes longer than a step, the next frame runs the updates
it owes, up to `Config::max_updates_per_frame` (8 by default), and drops
the rest of the backlog. When frames come faster than updates, a frame
can run no update at all. `ctx.time.alpha()` says how far the drawn frame
falls between the last update and the next, from 0 to 1. To keep motion
smooth at any refresh rate, keep the previous position and draw between
the two:

```rust,no_run
use yowie::{Color, Config, Context, Game, GameResult, LoopMode, Vec2, vec2};

struct Ball {
    previous: Vec2,
    current: Vec2,
    velocity: Vec2,
}

impl Game for Ball {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        let start = vec2(100.0, 100.0);
        Ok(Ball {
            previous: start,
            current: start,
            velocity: vec2(180.0, 120.0),
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        self.previous = self.current;
        self.current += self.velocity * ctx.time.delta();
        let size = ctx.gfx.view_size();
        if self.current.x < 0.0 || self.current.x > size.x {
            self.velocity.x = -self.velocity.x;
        }
        if self.current.y < 0.0 || self.current.y > size.y {
            self.velocity.y = -self.velocity.y;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let at = self.previous.lerp(self.current, ctx.time.alpha());
        ctx.gfx.fill_circle(at, 12.0, Color::WHITE);
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        loop_mode: LoopMode::FixedStep { hz: 120 },
        ..Config::new("Ball")
    };
    yowie::run::<Ball>(config)
}
```

Edge queries such as `key_pressed` and `mouse_pressed` report changes
since the previous update. In `update`, a press is reported once, in the
first update after it, however many frames the key stays down. During
`draw`, the edge queries cover the whole frame instead, so an interface
built in `draw` sees every click.

When the window is hidden or minimised, the fixed-step loop keeps
updating at its rate and stops drawing. `Config::pause_when_unfocused`
stops updates while the window lacks keyboard focus; frames still draw.

### Turn-based

`LoopMode::TurnBased` sleeps in the operating system until something
happens. Input, a gamepad change, a `Context::wake_after` deadline, a
`Waker` or `Context::request_redraw` runs one turn: one `update`, then
one `draw`. The first frame draws without an update. A turn-based game
uses no CPU while it waits for the player.

```rust,no_run
use std::time::Duration;

use yowie::gfx::TextStyle;
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, IVec2, LoopMode, Rect, ivec2, vec2};

const CELL: f32 = 32.0;

struct Walker {
    hero: IVec2,
    moves: u32,
}

impl Game for Walker {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Walker {
            hero: ivec2(5, 5),
            moves: 0,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let steps = [
            (Key::Left, ivec2(-1, 0)),
            (Key::Right, ivec2(1, 0)),
            (Key::Up, ivec2(0, -1)),
            (Key::Down, ivec2(0, 1)),
        ];
        for (key, step) in steps {
            if ctx.input.key_pressed(key) {
                self.hero += step;
                self.moves += 1;
                if self.moves % 10 == 0 {
                    // Every tenth move, run one more turn two seconds later.
                    ctx.wake_after(Duration::from_secs(2));
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let at = self.hero.as_vec2() * CELL;
        ctx.gfx.fill_rect(Rect::new(at.x, at.y, CELL, CELL), Color::YELLOW);
        let font = ctx.gfx.default_font();
        let text = format!("moves {}, turns {}", self.moves, ctx.time.updates());
        ctx.gfx.draw_text(&font, &text, vec2(8.0, 8.0), &TextStyle::new(18.0));
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        loop_mode: LoopMode::TurnBased,
        ..Config::new("Turns")
    };
    yowie::run::<Walker>(config)
}
```

Each wake runs a turn:

| To run a turn | Call |
|---|---|
| after a delay, for a timer | `ctx.wake_after(Duration)`; the earliest pending wake wins |
| on the next frame, while an animation plays | `ctx.request_redraw()`, once per frame until it ends |
| from another thread, for a network reply or a finished load | `ctx.waker()`, then `Waker::wake` on that thread |

Any input runs a turn, including pointer movement, so `ctx.time.updates()`
in this program counts more turns than moves.

In a turn-based loop, `ctx.time.delta()` is the wall time since the
previous turn, at most one second, times the time scale. A timer driven
by `delta()` therefore loses time when turns are more than a second
apart; for a wait of a known length, use `wake_after`. `ctx.time.alpha()`
is zero. While a gamepad is connected the loop checks it every 16 ms,
because gamepad events do not wake the operating system's wait.

### Time

`ctx.time` is a `Time`. Every value is game time unless it says wall time.

| Method | Returns |
|---|---|
| `delta()` | seconds the current update covers; during `draw`, the last update's value |
| `elapsed()` | seconds of game time, the sum of every update's delta (`f64`) |
| `alpha()` | where the drawn frame falls between updates, `0..1`; zero in turn-based loops and under a fixed clock |
| `frame()` | frames drawn before this one |
| `updates()` | updates run so far |
| `frame_delta()` | wall seconds between the previous frame and this one |
| `fps()` | frames per second, smoothed |
| `scale()`, `set_scale(f32)` | how fast game time runs against the wall clock, clamped to `0..=64` |
| `paused()`, `set_paused(bool)` | stops game time; in a fixed-step loop no updates run, and `draw` still does |

In a fixed-step loop a time scale keeps the step size and changes how many
updates run per wall second. Timers, tweens and anything else driven by
`delta()` follow the scale.

## Configuration

`Config` sets the window and the loop as they start. `Config::new(title)`
gives the defaults with a title; change other fields with struct update
syntax:

```rust
use yowie::{Color, Config, LoopMode, ScaleMode};

fn config() -> Config {
    Config {
        clear_color: Color::hex(0x1d2330),
        loop_mode: LoopMode::FixedStep { hz: 60 },
        console: true,
        ..Config::new("My Game").with_view(320, 180, ScaleMode::Integer)
    }
}

fn main() {
    let c = config();
    assert_eq!(c.width, 1280);
    assert_eq!(c.view_size, Some(yowie::uvec2(320, 180)));
}
```

Fields change nothing once `run` has started. To change the window while
the game runs, call `Context` methods (see the next section).

| Field | Default | Meaning |
|---|---|---|
| `title` | `"Yowie"` | the window title |
| `width`, `height` | 1280, 720 | the initial content size in logical points; the headless image size in pixels |
| `resizable` | true | whether the player can resize the window |
| `vsync` | true | whether presentation waits for the display's refresh |
| `fullscreen` | false | whether the window starts borderless full screen |
| `view_size` | `None` | a fixed view size in view units for a virtual resolution; `None` uses the window's logical size |
| `scale_mode` | `Letterbox` | how a fixed view fills the window: `Letterbox`, `Integer` or `Stretch` |
| `loop_mode` | `FixedStep { hz: 60 }` | the loop |
| `max_updates_per_frame` | 8 | the most updates one frame runs to catch up after a stall |
| `clear_color` | black | the colour each frame starts with |
| `pause_when_unfocused` | false | whether updates stop while the window lacks keyboard focus |
| `debug_overlay` | false | whether the frame-timing overlay starts shown |
| `handle_close` | false | whether the game decides what the close button does |
| `console` | false | whether the debug console is built (needs the `console` feature) |
| `console_key` | `Key::Grave` | the key that opens the console |
| `icon` | `None` | the window icon (not shown on macOS or Wayland) |
| `min_size`, `max_size` | `None` | resize limits in logical points |
| `position` | `None` | the initial top-left corner on the desktop (ignored on Wayland) |
| `always_on_top` | false | whether the window stays above others (no effect on Wayland) |
| `audio` | true | whether windowed runs open the default audio device (`audio` feature) |
| `asset_dir` | `None` | a directory whose files take priority over the `assets` directories |
| `packs` | empty | pack files to mount behind the asset directories |
| `hot_reload` | on in debug builds | whether changed asset files reload while the game runs |

`Config::with_size(width, height)` and
`Config::with_view(width, height, mode)` return a changed copy. With a fixed view, the game draws in a
`width` by `height` coordinate space and the renderer scales it into the
window at the window's full pixel resolution.

## Keys and window controls

The engine handles these keys itself:

| Key | Action | When |
|---|---|---|
| F3 | shows or hides the frame-timing overlay | always |
| `` ` `` (backtick) | opens and closes the debug console | `Config::console` is true; `Config::console_key` changes the key |
| F4 | opens and closes the console's debug panels | `Config::console` is true |

The overlay shows the frame rate, the frame, update and draw times in
milliseconds, the draw calls and vertices, and a graph of recent frame
times. `Context::set_debug_overlay` shows or hides it from code.

While the console is open it has the keyboard: the game gets an idle
input state and no input events, so typing does not move the player.
`Context::input_captured` reports when that is so. The
[console guide](console.md) covers its keys, commands and panels.

Escape, full screen and every other key belong to the game. The window's
close button ends the loop unless `Config::handle_close` is set; with it
set, the game receives `Event::CloseRequested` and calls `Context::quit`
when it is ready, for example after asking to save.

To control the window while the game runs, call these `Context` methods:

| Method | Does |
|---|---|
| `set_title`, `title` | sets or reads the title |
| `set_window_size`, `window_size` | asks for a content size in logical points, or reads it |
| `set_fullscreen`, `fullscreen` | switches borderless full screen, or reads whether it is on |
| `set_cursor_visible`, `set_cursor`, `set_cursor_grab` | hides the cursor, changes its shape or image, confines or locks it |
| `set_min_window_size`, `set_max_window_size` | changes the resize limits |
| `set_window_position`, `window_position` | moves the window or reads its position (not on Wayland) |
| `set_always_on_top`, `set_window_icon` | changes stacking and the icon |
| `focused`, `minimized`, `scale_factor` | reads the window's state |
| `clipboard_text`, `set_clipboard_text` | reads and writes the clipboard |
| `open_window`, `close_window` | opens and closes extra windows |

For example, to toggle full screen on F11:

```rust,no_run
use yowie::Context;
use yowie::input::Key;

fn toggle_fullscreen(ctx: &mut Context) {
    if ctx.input.key_pressed(Key::F11) {
        let on = ctx.fullscreen();
        ctx.set_fullscreen(!on);
    }
}
```

The [window guide](window.md) covers sizing, density, cursors, extra
windows and the events a window sends.

## Run without a window

The engine reads two command-line flags and one environment variable, so
any game can check itself on a machine with no desktop. It ignores every
other argument, so a game can parse its own.

| Flag | Effect |
|---|---|
| `--shot <file.png>` | runs with no window (a GPU device and no surface), renders under the fixed clock, and writes the last frame to the file |
| `--seconds <n>` | runs `n` seconds of game time, then exits; with `--shot`, renders that many seconds headless first |
| `YOWIE_FIXED_CLOCK=1` | advances time by exactly one fixed step per frame, so a given frame is the same picture on every machine |

Both flags also take the `--flag=value` form. With `--shot` and no
`--seconds`, the run draws one frame. The headless image is `Config::width`
by `Config::height` pixels. To pass the flags through `cargo run`, put
them after `--`:

```bash
cargo run -- --shot frame.png --seconds 2
YOWIE_FIXED_CLOCK=1 cargo run -- --seconds 10
```

In a headless run `Context::headless` returns true, the audio mixer plays
to no device (voices still start, finish and call back), and the
clipboard is private to the process. To get a useful picture from a game
that waits for the player, script its input: the `particles` and
`pathfinding` examples check `ctx.headless()`, and `tetris`, `roguelike`,
`animation` and `space` take an `--autoplay` flag of their own.

## Debugging

- F3 shows the frame-timing overlay.
- `Config { console: true, .. }` builds the debug console, which shows
  the log, runs commands, and opens panels for frame timings, the GPU,
  input devices, ECS worlds, physics and the mixer. See the
  [console guide](console.md).
- The engine installs `env_logger` when `run` starts, unless the game
  installed a logger first. Its default filter is
  `warn,yowie=info,yowie_gfx=info`; set `RUST_LOG` to change it, for
  example `RUST_LOG=debug cargo run`.
- In a debug build wgpu turns its validation on, which checks every GPU
  call and costs CPU time. A release build turns it off. To override
  either, set `WGPU_VALIDATION=1` or `WGPU_VALIDATION=0`.
- `WGPU_BACKEND` narrows the GPU backends the renderer tries, for example
  `WGPU_BACKEND=vulkan` on Windows.
- `yowie::FlyCamera` and `yowie::OrbitCamera` are input-driven 3D cameras
  for looking around a scene while it is being built. See the
  [3D graphics guide](graphics-3d.md).

## Run the examples

The repository has 30 example programs. To run one, clone the repository
and use `cargo run --example`:

```bash
git clone https://github.com/matjam/yowie
cd yowie
cargo run --example tiles
```

Flags for the example go after `--`:

```bash
cargo run --example gallery -- --theme nord --skin
cargo run --example lighting -- --effects taa,ssr,dof
cargo run --example tetris
cargo run --example roguelike
```

Every example accepts `--shot`, `--seconds` and `YOWIE_FIXED_CLOCK=1`.
The [README](../../README.md#examples) lists all 30 with their flags.
`cargo test --test examples` builds the examples, runs each one headless
and compares its frame with the image in `examples/testdata/`. The
comparison runs on the GPU the images were recorded on (named in
`examples/testdata/gpu.txt`); on another GPU the test checks that each
example drew something. The `assets` example changes files as it runs,
so it is only checked for a frame that is not blank. `network` and
`window` have no headless test.

## Ship a game

To ship a game, build it in release mode, pack its assets and bundle the
result:

```bash
cargo build --release
cargo install --git https://github.com/matjam/yowie yowie-asset yowie-tools
yowie-pack -o assets.pak assets
yowie-bundle --name "My Game" --exe target/release/my-game --assets assets.pak --icon icon.png
```

`yowie-pack` writes a directory into one compressed pack file. The engine
mounts `assets.pak` beside the executable and in the working directory
without being told. `yowie-bundle` writes a `.app` bundle on macOS and a
folder holding the executable and assets elsewhere, into `dist` by
default. It does not sign, notarize or cross-compile. The
[README](../../README.md#tools) lists every tool and its options.

## Next steps

- [API design](api-design.md): how ownership, scoped state, defaults and
  errors work across the API.
- [Building Tetris](tetris.md): a complete game on the entity component
  system.
- [The window](window.md) and [input](input.md): the platform layer in
  detail.
- For the API reference, run `cargo doc --open` in your game's directory.
