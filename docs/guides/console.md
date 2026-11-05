---
title: The debug console
group: Engine
order: 5
summary: the in-game command line, its built-in commands and variables, registering your own, key bindings and scripts, the debug panels, the frame-timing overlay, physics debug drawing and screenshots
---

# The debug console

The debug console is a drop-down command line and a window of panels that
show what the engine is doing. A game adds its own commands, variables,
information lines and panels to it, so you can tune and inspect a running
game without rebuilding it.

## Turning it on

To build the console, set `Config::console`:

```rust,no_run
use yowie::input::Key;
use yowie::{Config, Context, Game, GameResult};

struct Sandbox;

impl Game for Sandbox {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Sandbox)
    }

    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        console: true,
        console_key: Key::Grave,
        ..Config::new("Sandbox")
    };
    yowie::run::<Sandbox>(config)
}
```

| Setting | Default | Effect |
|---|---|---|
| `Config::console` | `false` | Builds the console and copies the log into it. |
| `Config::console_key` | `Key::Grave` (`` ` ``) | Opens and closes the drop-down. |

The console needs the `console` cargo feature, which is on by default. When
`Config::console` is off, `ctx.console` is a disabled console: every call
works and nothing is drawn, so registration code needs no conditions.

The engine draws the console after everything else, on the layer
`yowie::gfx::layers::CONSOLE`.

## Using the drop-down

| Key | Action |
|---|---|
| `` ` `` (the console key) | Opens and closes the drop-down. |
| F4 | Opens and closes the panels window. |
| Enter | Runs the line. |
| Up, Down | Steps through earlier commands. |
| Tab | Completes a command, variable, key name, panel name or command argument. |
| PageUp, PageDown, mouse wheel | Scrolls the output. |
| Escape | Closes the drop-down. |

While the drop-down is open it has the keyboard. The game's `update` and
`draw` see an idle `ctx.input`, `event` receives no input events, and
`ctx.input_captured()` returns `true`. Only the drop-down takes input away
from the game. While the panels window is open the game still gets input, so
check `ctx.console.wants_mouse()` before treating a click as a click in the
game world. See
[Input](input.md#input-while-the-console-or-interface-has-it).

A line is split into words at spaces. Double quotes keep spaces inside one
word, and `\"` and `\\` inside quotes stand for a quote and a backslash.
Lines starting with `#` or `//` are comments.

## Built-in commands

| Command | What it does |
|---|---|
| `help [command]` | Lists the commands, or describes one command or variable. |
| `echo <text...>` | Prints the text. |
| `clear` | Empties the output. |
| `quit` | Ends the game. |
| `screenshot [file]` | Saves the next frame as a PNG, `screenshot.png` by default. |
| `exec <file>` | Runs a file of commands, one per line. |
| `bind <key> <command...>` | Runs a command when a key is pressed. |
| `unbind <key>` | Removes a key's binding. |
| `binds` | Lists the bindings. |
| `fps` | The frame rate and frame time. |
| `stats` | Frame, update and draw times, draw calls, vertices, indices and outputs. |
| `log [level]` | Shows or sets the lowest log level the console shows: `trace`, `debug`, `info`, `warn` or `error`. |
| `panels [tab]` | Opens or closes the panels window, or opens it on a named tab. |
| `set <name> <value>` | Changes a variable. |
| `get <name>` | Prints a variable. |
| `vars [prefix]` | Lists the variables, with their types and help. |
| `timescale [factor]` | Shows or sets the game speed: 1 is normal, 0.5 half, 0 stops game time. |

The engine registers `timescale` and one variable, `time.scale`. Both follow
`ctx.time.scale()` in both directions: a change in the console changes the
game speed, and a call to `ctx.time.set_scale` shows in the variable.

A variable's name works as a command: `time.scale` prints it, and
`time.scale 0.5` sets it.

## Your own variables

To expose a value for tuning, register a variable. The call returns a
handle; read it in `update` with `get()`. Changes typed in the console show
up in the handle at once. The handle is cheap to clone, and every clone
reads and writes the same value.

| Call | Handle | Accepts |
|---|---|---|
| `ctx.console.float(name, default, help)` | `FloatVar` | A finite number. |
| `ctx.console.int(name, default, help)` | `IntVar` | A whole number. |
| `ctx.console.boolean(name, default, help)` | `BoolVar` | `true`, `false`, `on`, `off`, `yes`, `no`, `1`, `0`. |
| `ctx.console.string(name, default, help)` | `StringVar` | Any text. |

Use dotted names, such as `player.speed`, so `vars player` lists a group.
Variables also appear in the Services panel as editable fields.

## Your own commands

To add a command, call `register(name, help, handler)`. The handler gets
the words after the command's name and returns a `CommandResult`:
`Ok(text)` prints the text, `Err(text)` prints it in red. The `help`
command lists the help line beside the name.

```rust
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use yowie::console::{Console, ConsoleOptions};

/// What the console asks the game to do on its next update.
enum Order {
    Spawn(u32),
    Reset,
}

// The engine builds ctx.console; this builds one directly.
let mut console = Console::new(ConsoleOptions::default());

let speed = console.float("player.speed", 5.0, "running speed in units per second");
let god = console.boolean("cheat.god", false, "the player takes no damage");

let orders: Rc<RefCell<VecDeque<Order>>> = Rc::default();
let queue = orders.clone();
console.register("spawn", "spawn [count]: add monsters", move |args| {
    let count = match args.first() {
        Some(word) => word.parse().map_err(|_| format!("{word:?} is not a count"))?,
        None => 1,
    };
    queue.borrow_mut().push_back(Order::Spawn(count));
    Ok(format!("spawning {count}"))
});
let queue = orders.clone();
console.register("reset", "reset: start the level again", move |_| {
    queue.borrow_mut().push_back(Order::Reset);
    Ok(String::new())
});

console.execute("set player.speed 7.5");
console.execute("cheat.god on");
console.execute("spawn 4");
assert_eq!(speed.get(), 7.5);
assert!(god.get());
assert_eq!(console.lines(1)[0].text, "spawning 4");

// In update: carry out what the console asked for.
let pending: Vec<Order> = orders.borrow_mut().drain(..).collect();
for order in pending {
    match order {
        Order::Spawn(count) => assert_eq!(count, 4),
        Order::Reset => {}
    }
}
```

A handler must be `'static`, so it cannot borrow the game. Share state with
it through `Rc<RefCell<...>>` or a variable handle, as above: the handler
records what to do, and the game does it in its next `update`.

Registering a name again replaces the command, built-ins included. To give
a command Tab completion for its arguments, use `register_completing`. Its
completer gets the words typed so far, the last one partial, and returns the
candidates:

```rust
use yowie::console::{Console, ConsoleOptions};

const LEVELS: [&str; 3] = ["cellar", "crypt", "tower"];

let mut console = Console::new(ConsoleOptions::default());
console.register_completing(
    "level",
    "level <name>: jump to a level",
    |args| match args {
        [name] if LEVELS.contains(&name.as_str()) => Ok(format!("going to {name}")),
        _ => Err(format!("levels: {}", LEVELS.join(", "))),
    },
    |args| {
        let partial = args.last().map_or("", String::as_str);
        LEVELS.iter().filter(|l| l.starts_with(partial)).map(|l| l.to_string()).collect()
    },
);
console.execute("level crypt");
assert_eq!(console.lines(1)[0].text, "going to crypt");
```

To run a command from code, call `ctx.console.execute(line)`. The line is
echoed and added to the history, the same as a typed line. `print(text)` and
`print_error(text)` write to the output, and `lines(count)` returns the
latest output and log lines as `LogLine { level, target, text }`.

## Key bindings

To run a command with one key, bind it, from the console or from code:

```text
bind F5 screenshot quick.png
bind G cheat.god on
unbind G
```

`ctx.console.bind(Key::F5, "screenshot quick.png")` does the same from
code, and `ctx.console.unbind(key)` removes it. Key names are those of
`Key::from_name`, such as `F5`, `G`, `Space` or `Digit7`. Bindings fire
only while the drop-down is shut, and the game sees the key press as well. A
binding on the console key or F4 never fires.

## Scripts

`exec <file>` runs a text file of commands, one per line, with `#` and `//`
comment lines. Scripts may run other scripts, up to eight deep. The file is
read from disk, relative to the working directory:

```text
# tuning.cfg
set player.speed 6
set cheat.god on
bind F6 spawn 10
```

To run commands at start-up, call `ctx.console.execute` in `init`. The
physics lab example runs the commands of its `--exec` argument this way.

## The panels

F4, or `panels`, opens a window of tabs:

| Tab | Shows |
|---|---|
| Engine | Frame rate and frame time, update and draw times, a graph of the last 240 frames (update in blue, draw in orange) against a 16.7 ms line, draw calls, vertices, indices and outputs. |
| GPU | The adapter, backend and device type, the output size and format, the view size and pixels per view unit, and GPU time per render pass where the device supports timestamps. Textures you watch, with an estimate of their memory. |
| Input | Keys held, the pointer, buttons, wheel and modifiers, and each gamepad with its axes. |
| Services | Every variable as an editable field, the lines you add with `add_info`, and the key bindings. |
| Entities | An ECS world's systems with their run times and an on/off switch each, and its entities with a filter. Appears once fed. |
| Physics | Body and collider counts and gravity of each physics world you feed. |
| Audio | The mixer's master, music and effects buses, with volume, mute, solo and pause. Appears once attached. |
| Your own | Tabs added with `add_panel`. |

GPU timing is turned on while the panels window is open.

To fill the panels from your game:

| Call | When | Effect |
|---|---|---|
| `ctx.console.add_info(name, f)` | Once, in `init` | Adds a line to the Services panel, redrawn from `f()` every frame. |
| `ctx.console.add_panel(name, body)` | Once, in `init` | Adds a tab. `body` runs every frame the tab is shown and adds widgets to the `yowie::ui::Frame` it is given. |
| `ctx.console.watch_texture(name, &texture)` | Once | Lists a texture in the GPU panel. |
| `ctx.console.feed_world(name, &world, Some(&mut schedule))` | Every update | Shows an ECS world in the Entities panel, its first 500 entities with name and position, and applies the system switches to the schedule. |
| `ctx.record_schedule(&schedule)` | Every update | Shows only the system timings in the Entities panel. |
| `ctx.console.feed_physics_2d(name, &world)`, `feed_physics_3d` | Every update | Shows a physics world in the Physics panel. |
| `ctx.console.attach_mixer(&ctx.audio)` | Once | Shows the mixer in the Audio panel. |

`feed_world` is cheap while the panels are hidden.

```rust,no_run
use std::cell::Cell;
use std::rc::Rc;

use yowie::{Context, GameResult};

/// Registers the game's console additions. Call it from `init`.
fn add_console_tools(ctx: &mut Context, seed: u64) -> GameResult<Rc<Cell<bool>>> {
    let reset = Rc::new(Cell::new(false));

    ctx.console.add_info("seed", move || seed.to_string());

    let pressed = reset.clone();
    ctx.console.add_panel("Level", move |f| {
        f.heading("Level");
        f.label(&format!("seed {seed}"));
        if f.button("Reset").clicked {
            pressed.set(true);
        }
    });

    ctx.console.attach_mixer(&ctx.audio);
    Ok(reset)
}
```

The ECS side is in
[Entities and systems](ecs.md#plugging-into-the-game-loop).

## The frame-timing overlay

F3 shows a small overlay in the top left corner: frames per second, frame
time, smoothed update and draw times, draw calls and vertices, and a graph
of the last 120 frame times. Bars are green up to 17 ms, yellow up to
33.3 ms and red above, with a line at 16.7 ms.

The overlay works without the console. `Config::debug_overlay` shows it
from the start, and `ctx.set_debug_overlay(on)` and `ctx.debug_overlay()`
control it from code. It draws on the layer `yowie::gfx::layers::DEBUG`,
above the console.

## Physics debug drawing

To draw colliders, joints and contacts as lines, call the physics world's
debug drawing inside the camera the world is drawn with:

```rust,no_run
use yowie::gfx::Camera2D;
use yowie::gfx::scene::Camera3D;
use yowie::phys::{World2D, World3D};
use yowie::{Context, SceneDebugDraw};

fn draw_physics_2d(ctx: &mut Context, camera: &Camera2D, world: &mut World2D) {
    ctx.gfx.with_camera(camera, |g| world.debug_draw_to(g));
}

fn draw_physics_3d(ctx: &mut Context, camera: &Camera3D, world: &mut World3D) {
    world.set_debug_contacts(true);
    ctx.gfx.scene(camera, |s| world.debug_draw(&mut SceneDebugDraw::new(s)));
}
```

Sleeping bodies are drawn darker. `SceneDebugDraw` hides lines behind
geometry; set its `on_top` field to draw them over everything. For your own
renderer, implement `yowie::phys::DebugDraw` and pass it to
`debug_draw`. The physics guide covers the worlds themselves: see
[Physics](physics.md).

A common setup puts the switch in a console variable, such as
`ctx.console.boolean("phys.debug", false, "draw colliders")`, and checks it
in `draw`.

## Screenshots

`screenshot [file]` saves the next frame as a PNG and logs
`yowie: wrote <file>`. Typed into the drop-down, the picture includes the
open drop-down; bind the command to a key (`bind F12 screenshot`) to capture
the game alone. To capture a frame from code, call
`ctx.gfx.request_capture()` during a frame, then `ctx.gfx.take_capture()`
in a later update or draw. It returns the frame as an sRGB RGBA image once
the frame has rendered. For captures of headless runs, see
[`--shot`](window.md#running-without-a-window).

## The log

The engine installs a logger that writes to the terminal, filtered by
`RUST_LOG` (default `warn,yowie=info,yowie_gfx=info`). With the console on,
it also copies records at `info` and above into the console, which keeps the
last 1000 lines. `log debug` makes the console show debug records too,
without changing what the terminal prints.

If the game installs its own logger before calling `yowie::run`, the
engine keeps it, and the console shows only its own output.

## Examples

| Command | Shows |
|---|---|
| `cargo run --example gallery` | The variables `gallery.theme` and `gallery.volume`, a `theme` command with completion, and the Audio panel. `--console` and `--panels` start with them open, `--debug` shows the frame-timing overlay. Try `theme nord`, `set gallery.volume 0.2` and `help`. |
| `cargo run --example physics_lab` | Commands `spawn`, `reset`, `ragdoll`, `kick`, `pause` and `debug`, and variables `lab.paused`, `lab.debug`, `lab.contacts`, `lab.gravity`, `wheel.speed` and `wheel.torque`. `--exec "spawn 20; set lab.gravity 3"` runs commands at start. |
| `cargo run --example solar` | The Entities panel on a scene, with system switches and timings. `--panels` opens the panels. |
| `cargo run --example animation` | System timings recorded with `record_schedule`. |

For the API reference, run `cargo doc --open` and look under
`yowie_console`.
