---
title: The window
group: Engine
order: 1
summary: how the window is opened, sized and controlled, extra windows, the loop that drives it, and what is fixed at start and what changes at run time
---

# The window

To open a window, call `yowie::run` from `main` with a `Config` and your
`Game` type. `run` creates the window, the renderer, the audio mixer and the
asset manager, calls `Game::init`, and then drives `update` and `draw` until
the game quits, the window closes, or `--seconds` of game time pass. It
returns the first error a callback, the renderer or the window system
produced.

This guide covers the main window and its settings, extra windows, the
events a window sends, the clipboard, and the two loop modes. Input from the
keyboard, mouse and gamepads is in [Input](input.md).

## Opening the window

Build a `Config` with `Config::new`, which takes the title and sets every
other field to its default, then adjust it with the `with_*` methods or with
struct update syntax.

```rust,no_run
use yowie::input::Key;
use yowie::{Color, Config, Context, Game, GameResult, vec2};

struct Hello;

impl Game for Hello {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Hello)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let font = ctx.gfx.default_font();
        let style = yowie::gfx::TextStyle::new(24.0);
        ctx.gfx.draw_text(&font, "Hello", vec2(20.0, 20.0), &style);
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(24, 28, 36),
        ..Config::new("Hello").with_size(800, 450).with_min_size(400, 225)
    };
    yowie::run::<Hello>(config)
}
```

Call `run` from `main`. Window systems deliver events only to the thread
that created the connection, and on macOS that must be the main thread.

`Config` holds the window and loop settings:

| Field | Default | Meaning |
|---|---|---|
| `title` | `"Yowie"` | The window title. |
| `width`, `height` | 1280, 720 | Content size in logical points. In a headless run, the image size in pixels. |
| `resizable` | `true` | Whether the player can resize the window. |
| `vsync` | `true` | Whether presentation waits for the display's refresh. |
| `fullscreen` | `false` | Whether the window starts borderless full screen on the current display. |
| `view_size` | `None` | A fixed view size for a virtual resolution, or `None` for the window's own size in points. |
| `scale_mode` | `ScaleMode::Letterbox` | How a fixed view fills the window. |
| `loop_mode` | `LoopMode::FixedStep { hz: 60 }` | How game time advances. |
| `max_updates_per_frame` | 8 | The most updates one frame runs to catch up after a stall. Zero means 1. |
| `clear_color` | `Color::BLACK` | The colour each frame starts with. |
| `pause_when_unfocused` | `false` | Whether updates stop while no window has keyboard focus. |
| `debug_overlay` | `false` | Whether the frame-timing overlay starts shown. |
| `handle_close` | `false` | Whether the game decides what a close request does. |
| `console` | `false` | Whether the debug console is built. See [The debug console](console.md). |
| `console_key` | `Key::Grave` | The key that opens the console. |
| `icon` | `None` | The window icon. |
| `min_size`, `max_size` | `None` | Limits on the content size the player can resize to, in logical points. |
| `position` | `None` | The window's initial top-left corner in logical desktop points. |
| `always_on_top` | `false` | Whether the window stays above other windows. |
| `audio` | `true` | Whether windowed runs open the default audio device. See [Audio](audio.md). |
| `asset_dir`, `packs`, `hot_reload` | see [Game services](services.md) | Where assets load from and whether they reload. |

The builder methods are `with_size`, `with_view`, `with_icon`,
`with_min_size`, `with_max_size`, `with_position`, `with_always_on_top`,
`with_audio`, `with_asset_dir` and `with_pack`. Each returns the changed
`Config`.

## What is fixed at start and what changes later

Changing a `Config` field after `run` begins does nothing. To change the
window while the game runs, call a method of `Context` or of `ctx.gfx`:

| Setting | At start | While running |
|---|---|---|
| Title | `title` | `ctx.set_title`, read with `ctx.title` |
| Size | `width`, `height` | `ctx.set_window_size`, read with `ctx.window_size` |
| Size limits | `min_size`, `max_size` | `ctx.set_min_window_size`, `ctx.set_max_window_size` |
| Resizable | `resizable` | fixed |
| Full screen | `fullscreen` | `ctx.set_fullscreen`, read with `ctx.fullscreen` |
| Position | `position` | `ctx.set_window_position`, read with `ctx.window_position` |
| Always on top | `always_on_top` | `ctx.set_always_on_top`, read with `ctx.always_on_top` |
| Icon | `icon` | `ctx.set_window_icon` |
| Vsync | `vsync` | `ctx.gfx.set_vsync` |
| Fixed view and scale mode | `view_size`, `scale_mode` | `ctx.gfx.set_view_settings` |
| Clear colour | `clear_color` | `ctx.gfx.set_clear_color` |
| Cursor shape, visibility, grab | none | `ctx.set_cursor`, `ctx.set_cursor_visible`, `ctx.set_cursor_grab` |
| Frame-timing overlay | `debug_overlay` | `ctx.set_debug_overlay`, or F3 |
| Loop mode, update rate, catch-up cap | `loop_mode`, `max_updates_per_frame` | fixed |
| Pause when unfocused, close handling | `pause_when_unfocused`, `handle_close` | fixed |
| Console, audio device, asset mounts, hot reload | `console`, `audio`, `asset_dir`, `packs`, `hot_reload` | fixed |

The display's scale factor is not a setting. Read it with
`ctx.scale_factor`; it changes when the window moves to another display.

## Points, pixels and view units

Three units meet at the window:

| Unit | What it measures | Where it appears |
|---|---|---|
| Logical points | Window size on the desktop, independent of the display's density | `Config::width` and `height`, `ctx.window_size`, `ctx.set_window_size`, size limits, positions |
| Pixels | The drawable surface | `Event::Resized`, `ctx.gfx.output_size`, `ctx.input.mouse_window` |
| View units | The game's 2D coordinates | everything drawn in 2D, `ctx.gfx.view_size`, `ctx.input.mouse` |

`ctx.scale_factor` is the number of pixels per logical point. Without a
fixed view, one view unit is one logical point, so a game that draws in view
units looks the same size on a standard and a high-density display. With a
fixed view, the view is `view_size` units across whatever the window size,
and `ctx.gfx.pixels_per_unit` says how many pixels one unit covers.

The origin is the top-left corner and +Y points down.

## A fixed virtual resolution

To draw at a fixed resolution and scale it to the window, call
`Config::with_view` with the view size and a `ScaleMode`:

```rust,no_run
use yowie::{Config, Context, Game, GameResult, ScaleMode};

struct Pixels;

impl Game for Pixels {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Pixels)
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // Always 320 by 180 view units, whatever the window size.
        let _view = ctx.gfx.view_size();
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config::new("Pixels")
        .with_size(1280, 720)
        .with_view(320, 180, ScaleMode::Integer);
    yowie::run::<Pixels>(config)
}
```

| `ScaleMode` | Behaviour |
|---|---|
| `Letterbox` | The largest scale that keeps the aspect ratio, centred, with bars on two sides. The default. |
| `Integer` | The largest whole-number scale that fits, centred, with bars around it. Pixel art stays sharp. When the window is smaller than the view, the scale stays 1 and the view is cropped. |
| `Stretch` | Fills the whole window and distorts the aspect ratio. |

The bars are black. The clear colour fills the view area only. The pointer
position from `ctx.input.mouse` is in view units through the same mapping,
so a pointer over a bar reads outside the view rectangle (a negative
coordinate, or one past the view size).

To change the view while the game runs, call `ctx.gfx.set_view_settings`.
Call it from `update`, so the next frame is drawn with the new view:

```rust,no_run
use yowie::gfx::ViewSettings;
use yowie::{Context, ScaleMode, uvec2};

fn use_window_size(ctx: &mut Context) {
    ctx.gfx.set_view_settings(ViewSettings::default());
}

fn use_pixel_view(ctx: &mut Context) {
    ctx.gfx.set_view_settings(ViewSettings {
        fixed: Some(uvec2(320, 180)),
        scale: ScaleMode::Integer,
    });
}
```

## Size, full screen and resize events

To ask for a new size, call `ctx.set_window_size` with logical points. The
desktop may adjust the size, and the size it settles on arrives as
`Event::Resized`. The event carries the new drawable size in pixels and the
scale factor. A move to a display with another density also arrives as
`Event::Resized`.

To switch to full screen, call `ctx.set_fullscreen(true)`. The window
becomes borderless full screen on its current display. `ctx.fullscreen`
reads the state back from the window system, so it also follows the
player's own full-screen button.

```rust,no_run
use yowie::input::Key;
use yowie::{Context, Event, Game, GameResult};

struct Viewer {
    pixels: (u32, u32),
}

impl Game for Viewer {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let size = ctx.gfx.output_size();
        Ok(Viewer { pixels: (size.x, size.y) })
    }

    fn event(&mut self, _ctx: &mut Context, event: &Event) -> GameResult {
        if let Event::Resized { width, height, scale_factor } = event {
            self.pixels = (*width, *height);
            log::info!("now {width} by {height} pixels at {scale_factor}x");
        }
        Ok(())
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::F11) {
            let on = !ctx.fullscreen();
            ctx.set_fullscreen(on);
        }
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}
```

The renderer resizes itself before `event` sees the resize, so a game that
draws in view units needs no code for it. Handle `Event::Resized` only to
rebuild things sized in pixels, such as render textures that match the
window.

## Title, icon, position and stacking

To change the title, call `ctx.set_title`. To set the icon, build an `Icon`
from a PNG or JPEG with `Icon::from_image_bytes`, or from RGBA pixels with
`Icon::from_rgba`, and pass it to `Config::with_icon` or
`ctx.set_window_icon`. `ctx.set_window_icon(None)` restores the default.

```rust,no_run
use yowie::{Config, Context, Game, GameResult, Icon};

struct Launcher;

impl Game for Launcher {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Launcher)
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icon = Icon::from_image_bytes(&std::fs::read("assets/icon.png")?)?;
    let config = Config::new("Launcher").with_icon(icon).with_position(100, 80);
    yowie::run::<Launcher>(config)?;
    Ok(())
}
```

Windows and X11 show the icon in the title bar and task bar. macOS takes
the application's icon from its bundle, so build the bundle with an icon:
`cargo run -p yowie-tools --bin yowie-bundle -- --name Game --exe
target/release/game --icon icon.png`. Wayland compositors look the icon up
from the desktop entry whose name matches the window's app id.

To move the window, call `ctx.set_window_position` with logical desktop
coordinates; `ctx.window_position` reads the top-left corner back. To keep
the window above others, call `ctx.set_always_on_top(true)`.
`ctx.minimized` reports whether the window is minimised, where the window
system says.

## The cursor

To change the pointer's shape over the window, call `ctx.set_cursor` with a
`Cursor`: `Arrow` (the default), `Pointer`, `Text`, `Crosshair`, `Move`,
`Grab`, `Grabbing`, `NotAllowed`, `Wait`, `Progress`, `Help`,
`ResizeHorizontal`, `ResizeVertical`, `ZoomIn`, `ZoomOut`, or
`Cursor::Custom` with an image.

A `CursorImage` is RGBA pixels plus the hotspot, the pixel that sits on the
pointer's position. Every platform accepts images; 64 by 64 pixels works
everywhere. A custom cursor shows from the next event-loop turn.

```rust,no_run
use yowie::{Context, Cursor, CursorImage, GameResult};

fn use_crosshair_image(ctx: &mut Context) -> GameResult {
    let mut pixels = vec![0u8; 16 * 16 * 4];
    for i in 0..16 {
        for (x, y) in [(i, 7), (7, i)] {
            let p = (y * 16 + x) * 4;
            pixels[p..p + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    let image = CursorImage::from_rgba(16, 16, (7, 7), pixels)?;
    ctx.set_cursor(Cursor::Custom(image));
    Ok(())
}
```

To hide the cursor, call `ctx.set_cursor_visible(false)`. To keep it in the
window, call `ctx.set_cursor_grab` with a `CursorGrab`:

| `CursorGrab` | Behaviour |
|---|---|
| `None` | The cursor moves freely. The default. |
| `Confined` | The cursor cannot leave the window. |
| `Locked` | The cursor stays where it is. Read movement from `ctx.input.mouse_motion`. |

A platform that lacks the requested mode falls back to the other one, and
the call returns an error only when neither is available. A first-person
camera locks and hides the cursor and reads the raw motion:

```rust,no_run
use yowie::input::Key;
use yowie::{Context, CursorGrab, GameResult, Vec2};

fn mouse_look(ctx: &mut Context, look: &mut Vec2, captured: &mut bool) -> GameResult {
    if ctx.input.key_pressed(Key::C) {
        *captured = !*captured;
        let grab = if *captured { CursorGrab::Locked } else { CursorGrab::None };
        ctx.set_cursor_grab(grab)?;
        ctx.set_cursor_visible(!*captured);
    }
    if *captured {
        *look += ctx.input.mouse_motion() * 0.004;
    }
    Ok(())
}
```

## Focus, visibility and closing

Every window event reaches `Game::event` before the next update, after the
engine has applied it. While the debug console's drop-down is open, input
events go to the console and not to the game. The window events are:

| `Event` | When |
|---|---|
| `Input(InputEvent)` | Keyboard, pointer, text or gamepad input, already applied to `ctx.input`. |
| `Resized { width, height, scale_factor }` | The drawable size (pixels) or the density changed. |
| `Focused(bool)` | The window gained or lost keyboard focus. |
| `Occluded(bool)` | The window became hidden or visible again. |
| `FileHovered(PathBuf)` | A file is dragged over the window, one event per file. |
| `FileHoverCancelled` | The drag left without dropping. |
| `FileDropped(PathBuf)` | A file was dropped, one event per file. |
| `CloseRequested` | The player asked to close the window. |
| `Closed` | An extra window no longer exists. Only extra windows send it. |

When the window loses focus, every held key and mouse button is released,
because their release events will not arrive. `ctx.focused` reports whether
any of the game's windows has keyboard focus. With
`Config::pause_when_unfocused`, updates stop while none has; frames still
draw.

A hidden or minimised window does not draw. In the fixed-step loop it keeps
updating at its rate, asleep between steps, and draws the skipped frame when
it becomes visible again.

By default a close request ends the loop after `event` sees
`Event::CloseRequested`. To ask the player first, set
`Config::handle_close` and call `ctx.quit` when the game is ready to end:

```rust,no_run
use yowie::{Config, Context, Event, Game, GameResult};

struct Editor {
    unsaved: bool,
    asking: bool,
}

impl Game for Editor {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Editor { unsaved: true, asking: false })
    }

    fn event(&mut self, ctx: &mut Context, event: &Event) -> GameResult {
        if *event == Event::CloseRequested {
            if self.unsaved {
                self.asking = true;
            } else {
                ctx.quit();
            }
        }
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config { handle_close: true, ..Config::new("Editor") };
    yowie::run::<Editor>(config)
}
```

`ctx.quit` ends the loop after the current callback.

## Dropped files

To accept files dragged onto the window, match `Event::FileDropped` in
`event`. Each dropped file arrives as its own event with its path.
`Event::FileHovered` arrives while the drag is over the window, for
highlighting a drop target, and `Event::FileHoverCancelled` when it leaves.

```rust,no_run
use std::path::PathBuf;

use yowie::{Context, Event, Game, GameResult};

struct Importer {
    dropped: Vec<PathBuf>,
    hovering: bool,
}

impl Game for Importer {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Importer { dropped: Vec::new(), hovering: false })
    }

    fn event(&mut self, _ctx: &mut Context, event: &Event) -> GameResult {
        match event {
            Event::FileHovered(_) => self.hovering = true,
            Event::FileHoverCancelled => self.hovering = false,
            Event::FileDropped(path) => {
                self.hovering = false;
                self.dropped.push(path.clone());
            }
            _ => {}
        }
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}
```

## The clipboard

To read the clipboard, call `ctx.clipboard_text`, which returns `None` when
it holds no text. To write it, call `ctx.set_clipboard_text`. The system
clipboard opens on first use, so a game that never touches it never
connects to it. A headless run keeps a private clipboard in memory.

`Modifiers::shortcut` is Command on macOS and Control elsewhere, for copy
and paste keys:

```rust,no_run
use yowie::input::{Key, Modifiers};
use yowie::{Context, GameResult};

fn copy_paste(ctx: &mut Context, field: &mut String) -> GameResult {
    if !ctx.input.modifiers().contains(Modifiers::shortcut()) {
        return Ok(());
    }
    if ctx.input.key_pressed(Key::C) {
        ctx.set_clipboard_text(field)?;
    }
    if ctx.input.key_pressed(Key::V)
        && let Some(text) = ctx.clipboard_text()
    {
        field.push_str(&text);
    }
    Ok(())
}
```

Text fields in the interface toolkit and the console use the same clipboard
for cut, copy and paste.

## Extra windows

To open a second window, call `ctx.open_window` with a `WindowConfig`. It
returns a `WindowId` at once. The window system creates the window after the
current callback returns, so the first frame it can show is the next one.

To draw into it, call `ctx.gfx.draw_in(id, |g| ...)`. Inside the closure
every drawing call goes to that window and `g.view_size` is its view.
`draw_in` returns `None` without calling the closure while the window does
not exist yet, and after it closes. Do not nest `draw_in` scopes; draw to
the main window with plain calls outside any scope.

```rust,no_run
use yowie::input::Key;
use yowie::{Color, Context, Event, Game, GameResult, WindowConfig, WindowId, vec2};

struct Tools {
    palette: Option<WindowId>,
}

impl Game for Tools {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Tools { palette: None })
    }

    fn window_event(&mut self, ctx: &mut Context, window: WindowId, event: &Event) -> GameResult {
        if Some(window) == self.palette && *event == Event::Closed {
            self.palette = None;
        }
        if window == WindowId::MAIN {
            self.event(ctx, event)?;
        }
        Ok(())
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::P) {
            match self.palette.take() {
                Some(id) => ctx.close_window(id),
                None => {
                    let config = WindowConfig {
                        clear_color: Color::rgb8(40, 24, 44),
                        ..WindowConfig::new("Palette").with_size(240, 320)
                    };
                    self.palette = Some(ctx.open_window(config));
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        if let Some(id) = self.palette {
            let pointer = ctx.window_pointer(id);
            ctx.gfx.draw_in(id, |g| {
                g.fill_circle(vec2(120.0, 160.0), 40.0, Color::ORANGE);
                if let Some(p) = pointer {
                    g.fill_circle(p, 6.0, Color::WHITE);
                }
            });
        }
        Ok(())
    }
}
```

`WindowConfig` sets what the window is when it opens:

| Field | Default | Meaning |
|---|---|---|
| `title` | `"Yowie"` | The title bar text. |
| `width`, `height` | 480, 320 | Content size in logical points. |
| `resizable` | `true` | Whether the player can resize it. |
| `vsync` | `true` | Whether presentation waits for the display's refresh. |
| `view_size` | `None` | A fixed 2D view size, or `None` for the window's size in points. |
| `scale_mode` | `ScaleMode::Letterbox` | How a fixed view fills the window. |
| `clear_color` | `Color::BLACK` | The colour each frame starts with. |
| `position` | `None` | The top-left corner in logical desktop points. |
| `always_on_top` | `false` | Whether the window stays above others. |
| `parent` | `None` | The window this one belongs to. Closing the parent closes this window too. |
| `handle_close` | `false` | Whether the game decides what a close request does. |

`WindowConfig::new`, `with_size` and `with_parent` build one.

The events of every window, the main one included, reach
`Game::window_event` with the window's id. The default implementation
forwards the main window's events to `Game::event` and ignores the rest, so
a game with extra windows overrides `window_event`, as above. Keyboard input
from any window also reaches `ctx.input`. Pointer events of an extra window
reach only `window_event`, in that window's own view units; `ctx.input`
tracks the main window's pointer. To read an extra window's pointer, call
`ctx.window_pointer(id)`, which returns `None` while the pointer is outside.

A close request on an extra window closes it after `window_event` sees
`Event::CloseRequested`, unless its `handle_close` is set. To close one
from code, call `ctx.close_window(id)`. Its children close first. The window
disappears after the current callback, and the game then receives
`Event::Closed` for it. Closing `WindowId::MAIN` ends the run.

The other calls for windows are:

| Call | What it does |
|---|---|
| `ctx.windows()` | Every open window, the main one first. |
| `ctx.window_info(id)` | A `WindowInfo` with the title, size in points, scale factor, focus and parent, or `None`. |
| `ctx.set_window_title_of(id, title)` | Sets a window's title. |
| `ctx.set_window_size_of(id, w, h)` | Asks for a new size in logical points. The result arrives as `Event::Resized` for that window. |
| `ctx.gfx.read_window_image(id)` | Reads back the last frame of an offscreen window as sRGB RGBA pixels. A real window's frames cannot be read. |

In a headless run, `open_window` creates an offscreen image of the same size
at once instead of a window, and `read_window_image` reads it.

## The loop

`Config::loop_mode` picks one of two loops.

In `LoopMode::FixedStep { hz }`, `update` runs `hz` times per second of wall
time and `draw` runs once per displayed frame. A zero `hz` means 60. After a
slow frame the loop runs extra updates to catch up, at most
`max_updates_per_frame` in one frame; the rest of the backlog is dropped.
`ctx.time.delta()` is the fixed step, and `ctx.time.alpha()` is how far the
drawn frame falls between the last update and the next, from 0 to 1. Draw
moving things at `previous.lerp(current, alpha)` for smooth motion at any
refresh rate.

In `LoopMode::TurnBased`, the process sleeps in the operating system until
something happens, and each wake runs one `update` and one `draw`. The first
frame draws without an update. These wake the loop:

| Wake | How |
|---|---|
| Input | Any key, pointer, text or window event. |
| A gamepad change | Polled every 16 ms while a controller is connected. |
| A timer | `ctx.wake_after(duration)`. The earliest pending wake wins. |
| Another frame | `ctx.request_redraw()`, for an animation that spans turns. |
| Another thread | `Waker::wake` on a handle from `ctx.waker()`. |

`ctx.time.delta()` in a turn-based loop is the wall time since the previous
turn times the time scale, at most one second, and zero for the first turn
after a pause.

```rust,no_run
use std::time::Duration;

use yowie::{Config, Context, Game, GameResult, LoopMode};

struct Board {
    sliding: f32,
}

impl Game for Board {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let waker = ctx.waker();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            // A finished background job runs one turn.
            waker.wake();
        });
        Ok(Board { sliding: 0.0 })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if self.sliding > 0.0 {
            self.sliding = (self.sliding - ctx.time.delta()).max(0.0);
            // Keep turns coming until the slide has finished.
            ctx.request_redraw();
        }
        // Blink a cursor twice a second while idle.
        ctx.wake_after(Duration::from_millis(500));
        Ok(())
    }

    fn draw(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config { loop_mode: LoopMode::TurnBased, ..Config::new("Board") };
    yowie::run::<Board>(config)
}
```

`Waker` is `Clone` and works from any thread. It does nothing once the loop
has ended and in headless runs. The networking types take a callback that
calls it, so a turn-based game wakes when a message arrives; see
[Game services](services.md#networking).

`ctx.time` also counts `elapsed` game seconds, `frame` (frames drawn
before this one), `updates`, `frame_delta` (wall seconds between frames) and
`fps`. To slow or speed game time, call `ctx.time.set_scale`, clamped to 0
to 64. To stop it, call `ctx.time.set_paused(true)`: a paused fixed-step
loop runs no updates but still draws, so menus and the console stay live.

## Running without a window

The engine reads three settings from the command line and the environment
and ignores every other argument, so a game can have its own:

| Setting | Effect |
|---|---|
| `--shot <file.png>` | Runs with no window, renders under the fixed clock and writes the last frame to the file. |
| `--seconds <n>` | Runs n seconds of game time, then exits. With `--shot`, renders that long headless first. |
| `YOWIE_FIXED_CLOCK=1` | Advances exactly one fixed step per frame, so a given frame is the same picture on every machine. |

Both flags accept `--flag value` and `--flag=value`.

```bash
cargo run --example tetris -- --autoplay --seconds 2 --shot /tmp/tetris.png
YOWIE_FIXED_CLOCK=1 cargo run --example window -- --seconds 3
```

A headless run renders `Config::width` by `Config::height` pixels with a
scale factor of 1. `ctx.headless()` returns `true`, the context reports
focus, no input arrives, and the clipboard is private. Window controls that
have nothing to act on do nothing; `set_fullscreen` and `set_title` keep
their values so `fullscreen` and `title` read them back. Extra windows
become offscreen images. Without `--seconds`, `--shot` renders one frame. A
run whose game time never advances (paused, or a time scale of 0) still
ends, after twice the frames `--seconds` needs at normal speed plus 600.

## Platform differences

| Feature | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Window icon | title bar and task bar | ignored; set in the app bundle | shown | ignored; from the desktop entry |
| Initial position, `set_window_position` | applied | applied | applied | ignored; the compositor places windows |
| `window_position` | reported | reported | reported | `None` |
| Always on top | applied | applied | applied | no effect |
| Custom cursor images | yes | yes | yes | yes |

On Linux the window's app id (Wayland) and `WM_CLASS` (X11) come from the
title: ASCII letters and digits in lower case, every other character
replaced by `-`. A window titled "My Game" has the id `my-game`, which is
the name its desktop entry needs.

## Example

`cargo run --example window` opens a window that prints every event as it
arrives, tagged with the window it came from. F11 toggles full screen, C
toggles a locked cursor, T toggles input-method composition, N opens or
closes a second window that draws its own content, and Escape quits. In a
headless run the second window opens at once as an offscreen image, and
`--second-shot <file.png>` writes it:

```bash
cargo run --example window -- --shot /tmp/main.png --second-shot /tmp/second.png
```

For the API reference, run `cargo doc --open` and look under `yowie`.
