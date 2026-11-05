---
title: Input
group: Engine
order: 2
summary: keys, typed text, the mouse, gamepads, action maps, rebinding and saved bindings, and how input edges relate to updates
---

# Input

To read input, use `ctx.input`, an `InputState` that holds every key,
mouse button, stick and the pointer. The engine feeds it from the window
system and the gamepad library; a game only reads it. Its types live in
`yowie::input`.

Each query is one of two kinds:

- Level queries report what is true now: `key_down`, `mouse_down`, a
  gamepad's `down` and `axis`, the pointer position.
- Edge queries report what changed: `key_pressed`, `key_released`,
  `mouse_pressed`, `text`, `wheel` and the rest.

## Edges and updates

Edges belong to updates. An edge query in `update` reports the changes since
the previous update:

- A press that arrives between two updates is kept until the next update
  sees it, so no press is lost however short it is.
- When one frame runs several updates to catch up, only the first sees the
  press.
- When a frame runs no update (a fast display with a 60 Hz fixed step), the
  edge waits for the next frame that does.

During `draw` the edge queries cover the whole drawn frame instead, so an
interface built in `draw` sees every change, whether the frame ran several
updates or none.

The engine calls `InputState::apply` for each event, `end_update` after each
update, and `begin_draw` and `end_draw` around `draw`. A test can make the
same calls to drive an `InputState` without a window:

```rust
use yowie::input::{InputEvent, InputState, Key};

let mut input = InputState::new();

// A tap that starts and ends between two updates.
input.apply(&InputEvent::KeyDown { key: Key::Space, repeat: false });
input.apply(&InputEvent::KeyUp { key: Key::Space });

// The next update sees both edges, although the key is up again.
assert!(!input.key_down(Key::Space));
assert!(input.key_pressed(Key::Space));
assert!(input.key_released(Key::Space));
input.end_update();

// The update after that does not.
assert!(!input.key_pressed(Key::Space));

// Drawing reads the frame's edges, which last until the frame ends.
input.begin_draw();
assert!(input.key_pressed(Key::Space));
input.end_draw();
```

Read gameplay input in `update`, where each press counts once. While game
time is paused (`ctx.time.set_paused(true)`) the fixed-step loop runs no
updates, so read the key that unpauses in `event` or `draw`:

```rust,no_run
use yowie::input::{InputEvent, Key};
use yowie::{Context, Event, GameResult};

fn pause_key(ctx: &mut Context, event: &Event) -> GameResult {
    if let Event::Input(InputEvent::KeyDown { key: Key::P, repeat: false }) = event {
        let paused = ctx.time.paused();
        ctx.time.set_paused(!paused);
    }
    Ok(())
}
```

Call it from `Game::event`. Every input event reaches `event` as it
arrives, already applied to `ctx.input`.

When the window loses focus, every held key and mouse button is released,
because their release events will not arrive.

## Keys

| Call | Returns |
|---|---|
| `key_down(key)` | Whether the key is held. |
| `key_pressed(key)` | Whether it went down since the previous update. Auto-repeats do not count. |
| `key_released(key)` | Whether it went up since the previous update. |
| `key_repeated(key)` | Whether the operating system sent an auto-repeat since the previous update. |
| `key_held(key)` | How long the key has been held, in seconds of updates; zero when up. |
| `keys_down()` | Every held key, in `Key::ALL` order. |
| `modifiers()` | The held `Modifiers`. |

A menu that steps while a key is held tests
`key_pressed(k) || key_repeated(k)`.

`Key` names a physical position, after the key in that place on a US
layout. `Key::W` is the key in W's place whatever the active layout prints
on it, which is what movement bindings need. For text entry, read typed
text instead (next section).

`Modifiers` is a set: test it with `contains`, which accepts several at
once (`Modifiers::CONTROL | Modifiers::SHIFT`). The members are `SHIFT`,
`CONTROL`, `ALT` (Option on macOS) and `SUPER` (Command on macOS, the
Windows key elsewhere). `Modifiers::shortcut()` is `SUPER` on macOS and
`CONTROL` elsewhere, for copy and paste shortcuts.

Every key has a stable name for saved settings: `Key::name` returns it,
`Display` writes it, and `str::parse` and `Key::from_name` read it back
without regard to case.

```rust
use yowie::input::Key;

assert_eq!(Key::Space.name(), "Space");
assert_eq!("keypadenter".parse::<Key>().ok(), Some(Key::KeypadEnter));
assert_eq!(Key::from_name("Digit7"), None);
assert_eq!(Key::from_name("7"), Some(Key::Digit7));
```

## Typed text and input methods

To read typed text, call `ctx.input.text()`. It holds the text committed
since the previous update, with the keyboard layout, modifiers and input
method applied and control characters removed. Backspace, Enter and the
arrows arrive as keys, not text.

```rust,no_run
use yowie::input::Key;
use yowie::{Context, GameResult};

fn edit_name(ctx: &mut Context, name: &mut String) -> GameResult {
    name.push_str(ctx.input.text());
    if ctx.input.key_pressed(Key::Backspace) || ctx.input.key_repeated(Key::Backspace) {
        name.pop();
    }
    Ok(())
}
```

Languages typed through an input method compose text before committing it.
To let the input method compose, call `ctx.set_text_input(true)`. While it
is on, `ctx.input.composition()` holds the uncommitted text and
`composition_cursor()` the caret or selection inside it as a byte range;
show them after the committed text. While it is off, every key press
arrives as a key event, which is what games want during play. Typed text
arrives either way.

To place the input method's candidate window beside the text field, call
`ctx.set_text_input_area` with the field's rectangle in view units.

The interface toolkit's text fields and the console turn text input on
while a field has focus, place the candidate window, and turn it off again
afterwards.

## The mouse

| Call | Returns |
|---|---|
| `mouse()` | The pointer in view units, the game's 2D coordinates. |
| `mouse_window()` | The pointer in window pixels from the top-left corner. |
| `mouse_inside()` | Whether the pointer is over the window. |
| `mouse_delta()` | Pointer movement in view units since the previous update. |
| `mouse_motion()` | Raw device motion since the previous update. It keeps arriving while the cursor is grabbed. |
| `mouse_down(button)` | Whether the button is held. |
| `mouse_pressed(button)`, `mouse_released(button)` | Whether it went down or up since the previous update. |
| `mouse_double_clicked(button)` | Whether this press was the second of two within `DOUBLE_CLICK_TIME` (0.4 s of updates) and `DOUBLE_CLICK_DISTANCE` (6 view units). A third click starts a new pair. |
| `wheel()` | Wheel movement in lines since the previous update; +Y is away from the user. |

`MouseButton` is `Left`, `Right`, `Middle`, `Back` or `Forward`; the last
two are the side buttons many mice have. Trackpads that scroll by pixels
are converted to lines at 20 logical points a line.

With a fixed view, a pointer over the letterbox bars reads outside the view
rectangle; see [The window](window.md#a-fixed-virtual-resolution). To turn
the pointer into a world position under a 2D camera, pass it to
`Camera2D::screen_to_world` with the view size:

```rust,no_run
use yowie::gfx::Camera2D;
use yowie::{Context, Vec2};

fn pointer_in_world(ctx: &Context, camera: &Camera2D) -> Vec2 {
    camera.screen_to_world(ctx.input.mouse(), ctx.gfx.view_size())
}
```

For a 3D scene, build a ray from the pointer with `Scene::screen_ray`; see
[3D graphics](graphics-3d.md).

For mouse look, lock the cursor with `ctx.set_cursor_grab` and read
`mouse_motion`; see [The window](window.md#the-cursor).

## Gamepads

The input state tracks up to `MAX_GAMEPADS` (4) controllers. Each connected
controller takes the lowest free slot, a `GamepadId`, and keeps it until it
disconnects. To read one, call `ctx.input.gamepad(GamepadId(0))`; a
disconnected slot reads as idle. `ctx.input.gamepads()` iterates the
connected ones.

| `Gamepad` call | Returns |
|---|---|
| `id()`, `connected()`, `name()` | The slot, whether a controller is in it, and the device's name. |
| `down(button)` | Whether the button is held. |
| `pressed(button)`, `released(button)` | Whether it went down or up since the previous update. |
| `axis(axis)` | The axis value; values strictly inside `AXIS_DEAD_ZONE` (0.08) read as zero, and the rest are not rescaled. |
| `raw_axis(axis)` | The axis value as the device reported it. |
| `previous_axis(axis)` | The raw value at the end of the previous update. |
| `just_connected()`, `just_disconnected()` | Whether the controller connected or disconnected since the previous update. |

Buttons are named by position on a standard pad:

| `GamepadButton` | Button |
|---|---|
| `South`, `East`, `West`, `North` | The face buttons; `South` is A on Xbox controllers and Cross on PlayStation ones. |
| `LeftShoulder`, `RightShoulder` | The shoulder buttons. |
| `LeftTrigger`, `RightTrigger` | The triggers read as buttons. |
| `LeftStick`, `RightStick` | Pressing a stick in. |
| `Start`, `Select`, `Mode` | Start or Menu; Select, Back or Share; the vendor button. |
| `DPadUp`, `DPadDown`, `DPadLeft`, `DPadRight` | The directional pad. |

| `GamepadAxis` | Range |
|---|---|
| `LeftX`, `LeftY`, `RightX`, `RightY` | -1 to 1, +X right and +Y up as the hardware reports them. |
| `LeftTrigger`, `RightTrigger` | 0 (released) to 1 (pressed). |

Stick Y is positive up, and screen Y is positive down, so negate it to move
something on screen:

```rust,no_run
use yowie::input::{GamepadAxis, GamepadButton, GamepadId};
use yowie::{Context, Vec2, vec2};

fn pad_move(ctx: &Context, player: &mut Vec2, jumping: &mut bool) {
    let pad = ctx.input.gamepad(GamepadId(0));
    let stick = vec2(pad.axis(GamepadAxis::LeftX), -pad.axis(GamepadAxis::LeftY));
    *player += stick * 240.0 * ctx.time.delta();
    if pad.pressed(GamepadButton::South) {
        *jumping = true;
    }
}
```

In a turn-based loop gamepads are polled every 16 ms while one is connected,
because their events do not wake the window system. There is no rumble
output. Action maps (below) read gamepads with a larger dead zone of their
own.

## Action maps

To let the player rebind controls, name what the player does and bind the
names to keys, buttons and axes in an `ActionMap`. Game code then asks for
actions, and a settings screen rebinds them.

An action is one of two kinds:

- A one-axis action, made with `bind`: a button such as "jump", or an axis
  such as "steer". It is held when any of its sources is held, and its
  value is the sum of its sources clamped to -1 to 1.
- A two-axis action, made with `bind_vec2`: "move" from WASD, the arrows or
  a stick. Its value is the sum of each source's push, limited to unit
  length, so WASD and a stick give one `Vec2`.

```rust
use yowie::input::{
    ActionMap, Binding, Binding2, GamepadAxis, GamepadButton, InputEvent, InputState, Key,
    Source,
};
use yowie::vec2;

let mut actions = ActionMap::new();
let movement = actions.bind_vec2("move", Binding2::wasd());
actions.bind_vec2("move", Binding2::arrows());
actions.bind_vec2("move", Binding2::left_stick());
let jump = actions.bind(
    "jump",
    [Binding::from(Key::Space), Binding::from(GamepadButton::South)],
);
let steer = actions.bind(
    "steer",
    [
        Source::Key(Key::A).negative(),
        Binding::from(Key::D),
        Binding::from(GamepadAxis::LeftX),
        Source::PadAxis(GamepadAxis::LeftX).negative(),
    ],
);

let mut input = InputState::new();
input.apply(&InputEvent::KeyDown { key: Key::D, repeat: false });
input.apply(&InputEvent::KeyDown { key: Key::Space, repeat: false });

assert_eq!(actions.vec2(&input, movement), vec2(1.0, 0.0));
assert_eq!(actions.value(&input, steer), 1.0);
assert!(actions.pressed(&input, jump));

input.end_update();
assert!(actions.down(&input, jump));
assert!(!actions.pressed(&input, jump));
```

The calls that build bindings are:

| Call | Binding |
|---|---|
| `Binding::from(Key::Space)` | A key, mouse button, gamepad button or axis at full strength. |
| `source.negative()` | The source contributing -1: the A of an A and D pair, or the left half of a stick axis. |
| `source.scaled(s)` | The source contributing `s` at full travel. |
| `source.toward(direction)` | A two-axis binding that pushes towards `direction` while held, or by the axis value times `direction`. |
| `Binding2::wasd()`, `arrows()`, `dpad()` | Four directions in screen coordinates (+Y down). |
| `Binding2::left_stick()`, `right_stick()` | A stick in screen coordinates: pushing it up gives negative Y. |

An axis source in a one-axis action reads only the side its scale's sign
selects, so bind a stick axis twice, once with `negative`, to read both
directions, as "steer" does above.

To read an action, pass the input state and the action to the map:

| Call | Returns |
|---|---|
| `value(input, action)` | A one-axis action's value; a two-axis action's length. |
| `vec2(input, action)` | A two-axis action's vector; `(value, 0)` for a one-axis action. |
| `down(input, action)` | Whether any source is held. A key or button counts when down, an axis when past `AXIS_PRESS_THRESHOLD` (0.5) on its side. |
| `pressed(input, action)` | Whether the action became held since the previous update. A second source going down while another is held does not count. |
| `released(input, action)` | Whether it stopped being held since the previous update. |

`bind` and `bind_vec2` return an `ActionId`. Reading by `ActionId` skips the
name lookup; reading by `&str` works too. A handle names the action, not its
sources, so it stays valid across rebinding and loading. `bind` and
`bind_vec2` add to an action's sources; binding a one-axis action as
two-axis, or the reverse, replaces its sources.

Stick sources in an action map ignore travel inside the map's own dead
zone, `DEFAULT_ACTION_DEAD_ZONE` (0.2); change it with `set_dead_zone`. By
default pad sources read every connected controller. For local
multiplayer, give each player a map and call `set_gamepad(Some(id))` on it.
Keys and mouse buttons are read by every map.

## Rebinding

To capture the player's choice on a settings screen, call `listen` each
update while waiting. It returns the first key, mouse button, gamepad button
or stick flick since the previous update as a `Binding`; a flick to the
negative side comes back negated. Then replace the action's sources:

| Call | Effect |
|---|---|
| `rebind(action, binding)` | Replaces every source of a one-axis action with `binding`. |
| `rebind_vec2(action, index, source)` | Replaces the source of a two-axis action's `index`th binding, keeping its direction. Returns `false` when there is no such binding. |
| `unbind(action)` | Removes every source; the handle stays valid. |
| `bindings(action)` | What the action is bound to, as `Bindings::Axis` or `Bindings::Vec2`, for showing on screen. |
| `names()` | Every action name, in the order the actions were created. |

```rust
use yowie::input::{ActionMap, Binding, InputEvent, InputState, Key};

let mut actions = ActionMap::new();
let jump = actions.bind("jump", [Key::Space]);

// The player presses J on the "press a key for jump" screen.
let mut input = InputState::new();
input.apply(&InputEvent::KeyDown { key: Key::J, repeat: false });
if let Some(choice) = actions.listen(&input) {
    actions.rebind(jump, choice);
}
assert_eq!(actions.listen(&input), Some(Binding::from(Key::J)));
assert!(actions.down(&input, jump));
```

Show a binding to the player with `Display`, which writes `key:J`,
`pad:South` or `-axis:LeftX`, or build a label from `Key::name` and the
other names.

## Saving bindings

`ActionMap` implements serde's `Serialize` and `Deserialize`. A map is
written as an object keyed by action name, in creation order. A one-axis
action is a list of binding strings; a two-axis action is an object with a
`vec2` list of sources and directions:

```text
{
  "jump": ["key:Space", "pad:South"],
  "move": {"vec2": [{"source": "key:W", "direction": [0.0, -1.0]}, ...]}
}
```

A binding string is `kind:Name` with an optional sign and scale:

| String | Source |
|---|---|
| `key:Space` | A key, by `Key::name`. |
| `mouse:Left` | A mouse button. |
| `pad:South` | A gamepad button. |
| `axis:LeftX` | The positive side of a gamepad axis. |
| `-axis:LeftX` | The negative side. |
| `axis:RightTrigger*0.5` | An axis contributing 0.5 at full travel. |

To restore saved bindings without invalidating the handles the game holds,
read the file into a new map and pass it to `load_from`. It replaces the
bindings of every action the saved map names and adds actions the game's
map lacks.

```rust
use yowie::input::{ActionMap, InputEvent, InputState, Key};

fn main() -> Result<(), serde_json::Error> {
    let mut actions = ActionMap::new();
    let jump = actions.bind("jump", [Key::Space]);
    let saved = r#"{"jump": ["key:J", "pad:South"]}"#;

    let loaded: ActionMap = serde_json::from_str(saved)?;
    actions.load_from(&loaded);

    let mut input = InputState::new();
    input.apply(&InputEvent::KeyDown { key: Key::J, repeat: false });
    assert!(actions.pressed(&input, jump));
    assert_eq!(serde_json::to_string(&actions)?, r#"{"jump":["key:J","pad:South"]}"#);
    Ok(())
}
```

To keep bindings between runs, write the map to a settings slot with
`yowie::save::Store`; see [Game services](services.md#saves-and-settings).

## Input while the console or interface has it

While the debug console's drop-down is open it has the keyboard. The engine
then gives the game an idle input state and no input events, and
`ctx.input_captured()` returns `true`. See
[The debug console](console.md).

The interface toolkit draws over the game in the same frame, so check
`ctx.ui.wants_mouse()` and `ctx.ui.wants_keyboard()` before acting on a
click or key that the interface may have used. See
[The interface](ui.md).

```rust,no_run
use yowie::input::MouseButton;
use yowie::{Context, Vec2};

fn click_in_world(ctx: &Context) -> Option<Vec2> {
    let free = !ctx.ui.wants_mouse() && !ctx.input_captured();
    (free && ctx.input.mouse_pressed(MouseButton::Left)).then(|| ctx.input.mouse())
}
```

## Example

`cargo run --example inputs` shows everything the input state reports: a
keyboard that lights the held keys, the mouse buttons, wheel and position in
view units and window pixels, raw motion while the cursor is locked (C
toggles, Escape releases), full screen (F), typed text with input-method
composition, four gamepad slots with their sticks and triggers, and an
action map moving a dot with WASD, the arrows or a stick (Space or the
south button flashes it).

For the API reference, run `cargo doc --open` and look under
`yowie_input`.
