---
title: The interface
group: Graphics
order: 5
summary: immediate-mode widgets, layout, windows and dialogs, keyboard and gamepad navigation, text fields, drag and drop, themes and skins
---

# The interface

`yowie::ui` draws menus, settings screens, inventories, editors and
heads-up displays. It is an immediate-mode toolkit: your `draw` code
calls a method for each widget every frame, and the call returns what
the player did with it. There are no widget objects to create, keep or
remove. The engine owns one `Ui` in `ctx.ui`, which keeps the state
that has to last between frames: focus, scroll offsets, text editors,
open menus and the order of windows.

To see every widget, run `cargo run --example gallery`. It accepts these
arguments:

| Argument | Effect |
|---|---|
| `--theme <name>` | Starts with a built-in theme, such as `nord` or `high-contrast`. |
| `--skin` | Draws the widgets from nine-slice textures. |
| `--debug` or `--overlay` | Shows the frame-timing overlay. |
| `--console`, `--panels` | Starts with the debug console or its panels open. |
| `--beep` | Plays a tone through the audio mixer. |

In the gallery, the backtick key opens the console, where `theme nord`
switches theme. For the full API, run `cargo doc --open`.

## A first panel

To show widgets, call `ctx.ui.draw` in `draw` and add widgets to the
`Frame` it passes to your closure:

```rust,no_run
use yowie::{Config, Context, Game, GameResult, Rect};

struct Settings {
    volume: f32,
    name: String,
    fullscreen: bool,
}

impl Game for Settings {
    fn init(_ctx: &mut Context) -> GameResult<Self> {
        Ok(Settings {
            volume: 0.8,
            name: String::from("Player"),
            fullscreen: false,
        })
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        ctx.ui.draw(&mut ctx.gfx, &ctx.input, |f| {
            f.panel(Some("Settings"), Rect::new(20.0, 20.0, 300.0, 220.0), |f| {
                f.slider("Volume", &mut self.volume, 0.0..=1.0);
                f.text_field("Name", &mut self.name);
                f.checkbox("Full screen", &mut self.fullscreen);
                if f.button("Reset").clicked {
                    self.volume = 0.8;
                }
            });
        });
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    yowie::run::<Settings>(Config::new("Settings"))
}
```

Widgets edit your own data through `&mut` references, so the game's
state is the only copy. Call `ctx.ui.draw` once per frame. It draws on
`yowie::gfx::layers::UI`, which sorts above the layers games draw on,
in screen space: cameras do not move it, and it appears over the game
whatever order you draw in.
Positions and sizes are view units, the same units as the rest of 2D
drawing (see [2D graphics](graphics-2d.md)). With a small fixed view
such as 320 by 180, the default theme's 15-unit text is large; lower
the theme's sizes as described under [Themes](#themes).

Each widget method returns a `Response`:

| Field | Meaning |
|---|---|
| `clicked` | Activated: released after pressing on it, or confirmed with Enter, Space or a gamepad's south button while focused. |
| `double_clicked`, `right_clicked` | Clicked twice quickly; right button released over it. |
| `hovered`, `held` | The pointer is over it; the button went down on it and is still down. |
| `changed` | The value it edits changed this frame. |
| `focused` | It has keyboard focus. |
| `submitted` | Enter was pressed in a single-line text field. |
| `id`, `rect` | Its `WidgetId` and where it was placed, in view units. |

## Widget identity

The toolkit finds a widget's state from one frame to the next by its
`WidgetId`. The id comes from the label and the containers around the
widget, so the same call in the same place gets the same id every frame.
Two widgets with the same label in one container are told apart by call
order.

When the order can change, give the widgets something stable to tell
them apart:

- Add `##` and a suffix to the label. The suffix is part of the id but
  is not drawn: `f.button("Save##slot2")` shows "Save".
- Wrap repeated widgets in `Frame::with_id(key, body)`, where `key` is
  any hashable value, such as the item's own id.

```rust,no_run
use yowie::ui::Frame;

struct Item {
    id: u32,
    name: String,
    count: u32,
}

fn inventory(f: &mut Frame<'_>, items: &mut Vec<Item>) {
    let mut dropped = None;
    for (index, item) in items.iter().enumerate() {
        f.with_id(item.id, |f| {
            f.horizontal(|f| {
                f.label(&format!("{} x{}", item.name, item.count));
                if f.button("Drop").clicked {
                    dropped = Some(index);
                }
            });
        });
    }
    if let Some(index) = dropped {
        items.remove(index);
    }
}
```

Every row has a button labelled "Drop", and `with_id` keeps each
button's id tied to its item when rows above it are removed. State for
a widget that stops appearing, such as a scroll offset or a text
field's undo history, is kept for at least 600 frames and then dropped.

## Layout

Widgets outside any container stack in a column from the top-left
corner of the view, as wide as the view. Inside a panel or window they
stack in a column as wide as its content. Each widget takes the
container's width or its natural width, with `Theme::spacing` between
widgets.

These methods lay out the widgets their closure adds:

| Method | Layout |
|---|---|
| `row(&[Size], body)` | Slots of the given sizes, side by side, wrapping to a new line after the last slot. `Size::Fixed(w)` is `w` units wide; `Size::Fill(weight)` shares what is left in proportion to its weight. |
| `columns(n, body)` | A row of `n` equal slots. |
| `grid(columns, body)` | Equal columns, filled row by row. |
| `horizontal(body)` | Side by side at natural widths. |
| `vertical(body)` | A stack, for use inside one slot of a row or a horizontal line. |
| `indent(amount, body)` | The column moved right by `amount`. |
| `padded(amount, body)` | The column inset by `amount` on both sides. |
| `aligned(Align, body)` | Natural-width widgets (labels, images, swatches) at the `Start`, `Center` or `End` of the container. |
| `area(rect, body)` | A column inside `rect`, clipped to it, with no background: heads-up displays and custom layouts. |

`space(amount)` adds a gap and `separator()` draws a line across the
container.

To compute rectangles for panels, windows and areas, use the functions
in `yowie::ui`. `f.view()` returns the whole view.

| Function | Returns |
|---|---|
| `anchored(area, Anchor, size, margin)` | A `size` rectangle placed at one of nine anchors of `area` (`TopLeft`, `Top`, `TopRight`, `Left`, `Center`, `Right`, `BottomLeft`, `Bottom`, `BottomRight`), `margin` in from the edges. |
| `stretched(area, left, top, right, bottom)` | `area` with each edge moved in. |
| `padded(area, amount)` | `area` with every edge moved in by `amount`. |
| `split_horizontal(area, t, gap)` | Two rectangles side by side; the first takes fraction `t` of the width. |
| `split_vertical(area, t, gap)` | Two rectangles one above the other. |

```rust,no_run
use yowie::ui::{Anchor, Frame, Size, anchored};
use yowie::vec2;

fn hud(f: &mut Frame<'_>, health: f32, gold: u32) {
    let corner = anchored(f.view(), Anchor::TopRight, vec2(220.0, 64.0), 12.0);
    f.area(corner, |f| {
        f.progress("Health", health);
        f.row(&[Size::Fill(1.0), Size::Fixed(60.0)], |f| {
            f.label("Gold");
            f.label(&gold.to_string());
        });
    });
}
```

## Widgets

| Method | Adds |
|---|---|
| `label(text)`, `label_dim(text)`, `label_colored(text, color)` | One line of text, in the text colour, the dim colour or your own. |
| `heading(text)` | A section heading. |
| `paragraph(text)` | Text wrapped to the container's width. |
| `button(label)` | A push button; check `clicked`. |
| `checkbox(label, &mut bool)` | A check box. |
| `radio(label, &mut value, option)` | One option of a group; selected while `value == option`. |
| `slider(label, &mut f32, range)`, `slider_int(label, &mut i32, range)` | A slider. Left and Right step it while it has focus; Home and End jump to the ends. |
| `progress(label, fraction)` | A bar filled from 0 to 1 with the percentage on it. |
| `image(&texture, size)`, `image_region(&texture, source, size)` | A picture, or a texel rectangle of one, `size` units large. Images do not respond to the pointer. |
| `color_swatch(label, color)` | A colour sample with a checkerboard behind translucent colours; `clicked` when pressed. |
| `tooltip(&response, text)` | Text beside the pointer after the widget has been hovered for 0.4 seconds. |
| `combo_box(label, &mut usize, &items)` | A drop-down list. Up and Down change the choice while it is closed. |
| `tabs(label, &tabs, &mut usize)` | A row of tabs. You draw the selected tab's content after it. |
| `list_box(label, height, &items, &mut usize)` | A list with one selected item, `height` units tall. |
| `table(...)` | A table with headers; see [Tables](#tables). |
| `tree_node(label, body)`, `tree_node_with(label, default_open, selected, body)`, `tree_leaf(label, selected)` | A tree. `body` adds the children of an open branch. |
| `collapsing_header(label, default_open, body)` | A header that shows or hides `body`. |
| `scroll_area(label, height, body)`, `scroll_area_with(label, height, horizontal, body)` | A bordered area whose contents scroll. |
| `text_field(label, &mut String)`, `text_field_with(...)`, `text_area(label, &mut String, height)` | Text editors; see [Text fields](#text-fields). |
| `menu_bar`, `menu`, `menu_item`, `context_menu` | Menus; see [Menus](#menus). |

Labels, buttons and other widgets with a caption show the label without
any `##` suffix. `is_open(id)` reports whether a tree node or header is
open.

```rust,no_run
use yowie::ui::Frame;

#[derive(PartialEq)]
enum Difficulty {
    Easy,
    Normal,
    Hard,
}

struct Options {
    tab: usize,
    lives: i32,
    difficulty: Difficulty,
    resolution: usize,
    music: f32,
}

const RESOLUTIONS: [&str; 3] = ["1280 x 720", "1920 x 1080", "2560 x 1440"];

fn options_page(f: &mut Frame<'_>, options: &mut Options) {
    f.tabs("pages", &["Game", "Video"], &mut options.tab);
    if options.tab == 0 {
        f.slider_int("Lives", &mut options.lives, 1..=9);
        f.radio("Easy", &mut options.difficulty, Difficulty::Easy);
        f.radio("Normal", &mut options.difficulty, Difficulty::Normal);
        f.radio("Hard", &mut options.difficulty, Difficulty::Hard);
    } else {
        f.combo_box("Resolution", &mut options.resolution, &RESOLUTIONS);
        let music = f.slider("Music", &mut options.music, 0.0..=1.0);
        f.tooltip(&music, "Background music volume");
    }
}
```

To grey out widgets and ignore input to them, wrap them in
`f.disabled(true, |f| ...)`. Disabled widgets cannot take focus.

### Tables

`table(label, &columns, rows, height, &mut selected, cell)` adds a table
`height` units tall. `Column::fill(title)` shares the leftover width and
`Column::fixed(title, width)` has a fixed width. The table calls `cell`
with the row and column of each visible cell, and only visible rows are
built, so a table can have many thousands of rows. Up, Down, Home, End,
PageUp and PageDown move `selected` while the table has focus. The
returned `TableResponse` says which row was clicked or chosen with the
keyboard and which header was clicked, for sorting.

```rust,no_run
use yowie::ui::{Column, Frame};

struct Score {
    name: String,
    points: u32,
}

fn high_scores(f: &mut Frame<'_>, scores: &mut [Score], selected: &mut Option<usize>) {
    let columns = [Column::fill("Name"), Column::fixed("Points", 80.0)];
    let table = f.table("scores", &columns, scores.len(), 240.0, selected, |f, row, column| {
        let score = &scores[row];
        if column == 0 {
            f.label(&score.name);
        } else {
            f.label(&score.points.to_string());
        }
    });
    if table.header_clicked == Some(1) {
        scores.sort_by(|a, b| b.points.cmp(&a.points));
    }
}
```

### Menus

`menu_bar(body)` adds a bar across the container, and `menu(label,
body)` adds a title to it. While a menu is open, its body adds items
with `menu_item(label)` and `menu_item_with(label, shortcut, checked)`,
which shows a shortcut hint and, when `checked` is `Some`, a check mark.
A `menu` inside a menu is a submenu. Choosing an item closes the menus.
`menu_separator()` adds a line between items. Shortcut hints are text
only; read the keys yourself.

To open a menu on right-click, call `context_menu(&response, body)`
after the widget.

```rust,no_run
use yowie::ui::Frame;

fn editor_menus(f: &mut Frame<'_>, show_grid: &mut bool, quit: &mut bool) {
    f.menu_bar(|f| {
        f.menu("File", |f| {
            if f.menu_item_with("Quit", "Ctrl+Q", None).clicked {
                *quit = true;
            }
        });
        f.menu("View", |f| {
            if f.menu_item_with("Grid", "", Some(*show_grid)).clicked {
                *show_grid = !*show_grid;
            }
        });
    });
    let tile = f.button("Tile 12");
    f.context_menu(&tile, |f| {
        if f.menu_item("Clear tile").clicked {
            *show_grid = true;
        }
    });
}
```

### Scroll areas

A scroll area scrolls with the wheel, with its scroll bar, and to
whichever widget inside it receives keyboard focus. To read or set its
offset in view units, pass its id to `scroll_offset(id)` or
`set_scroll_offset(id, offset)`. Get the id with `f.peek_id(label)`
before adding the area, in the same container. Offsets are clamped to
the size of the contents.

```rust,no_run
use yowie::Vec2;
use yowie::ui::Frame;

fn rules_text(f: &mut Frame<'_>, rules: &[String]) {
    let id = f.peek_id("rules");
    if f.scroll_offset(id).y > 0.0 && f.button("Back to top").clicked {
        f.set_scroll_offset(id, Vec2::ZERO);
    }
    f.scroll_area("rules", 200.0, |f| {
        for line in rules {
            f.paragraph(line);
        }
    });
}
```

### Drawing your own widgets

To draw something the toolkit lacks, reserve space with
`allocate(width, height)`, which returns the rectangle in view units,
and draw into it with `fill_rect`, `stroke_rect`, `image_rect`,
`text_at` and `text_at_mono`. `measure` and `measure_mono` return the
size of a string, `pointer` the pointer position, and `theme` the
current theme's colours.

```rust,no_run
use yowie::ui::Frame;
use yowie::{Color, Rect};

fn stamina_bar(f: &mut Frame<'_>, stamina: f32) {
    let rect = f.allocate(200.0, 10.0);
    let filled = Rect::new(rect.x, rect.y, rect.w * stamina.clamp(0.0, 1.0), rect.h);
    let border = f.theme().border;
    f.fill_rect(rect, Color::rgba(0.0, 0.0, 0.0, 0.5));
    f.fill_rect(filled, Color::rgb8(90, 200, 120));
    f.stroke_rect(rect, 1.0, border);
}
```

## Panels, windows and dialogs

| Method | Adds |
|---|---|
| `panel(title, rect, body)` | A fixed panel at `rect`, with a title bar when `title` is `Some`. Content that does not fit scrolls. |
| `window(title, &mut rect, body)` | A window the player can move by its title bar and resize by its bottom-right corner. |
| `window_with(title, options, &mut rect, body)` | A window with the behaviour `WindowOptions` sets. Returns the body's result and whether the close button was pressed. |
| `modal(title, &mut open, size, body)` | A dialog centred in the view while `open` is true, with the rest of the view dimmed behind it. |

A window's rectangle lives in your code, so you can save it with the
game's settings; the window writes the player's moves and resizes back
into it and keeps part of its title bar inside the view. Clicking a window
brings it to the front.

| `WindowOptions` field | Default | Meaning |
|---|---|---|
| `movable` | `true` | Dragging the title bar moves the window. |
| `resizable` | `true` | Dragging the bottom-right corner resizes it. |
| `closable` | `false` | The title bar has a close button. |
| `min_size` | 140 by 80 | The smallest size resizing allows, in view units. |

A modal dialog blocks input to everything behind it. Escape or a
gamepad's east button sets `open` to false when no popup is open.
`modal` returns the body's result while the dialog shows, and `None`
otherwise.

```rust,no_run
use yowie::ui::{Frame, WindowOptions};
use yowie::{Rect, vec2};

struct Windows {
    map: Rect,
    map_open: bool,
    confirm_quit: bool,
}

fn windows(f: &mut Frame<'_>, w: &mut Windows, quit: &mut bool) {
    if w.map_open {
        let options = WindowOptions {
            closable: true,
            ..WindowOptions::default()
        };
        let (_, closed) = f.window_with("Map", options, &mut w.map, |f| {
            f.label("The map goes here.");
        });
        w.map_open = !closed;
    }

    let mut open = w.confirm_quit;
    f.modal("Quit?", &mut open, vec2(300.0, 130.0), |f| {
        f.label("Leave the game?");
        f.columns(2, |f| {
            if f.button("Quit").clicked {
                *quit = true;
            }
            if f.button("Stay").clicked {
                w.confirm_quit = false;
            }
        });
    });
    w.confirm_quit &= open;
}
```

## Keeping input from reaching the game

The toolkit reads input but does not consume it: `ctx.input` reports
every click and key to your game as well. To ignore input the interface
took, check `ctx.ui` in `update`:

- `ctx.ui.wants_mouse()` is true while the pointer is over a panel,
  window, open combo box list or open menu, while a modal dialog is
  open, or while a drag is under way.
- `ctx.ui.wants_keyboard()` is true while a text field or text area has
  focus.

Both describe the last frame the interface drew, which is the frame the
player saw.

```rust,no_run
use yowie::input::{Key, MouseButton};
use yowie::{Context, GameResult, Vec2};

struct Player {
    target: Vec2,
    jumping: bool,
}

fn steer(ctx: &mut Context, player: &mut Player) -> GameResult {
    if ctx.input.mouse_pressed(MouseButton::Left) && !ctx.ui.wants_mouse() {
        player.target = ctx.input.mouse();
    }
    if ctx.input.key_pressed(Key::Space) && !ctx.ui.wants_keyboard() {
        player.jumping = true;
    }
    Ok(())
}
```

`wants_mouse` does not cover widgets added outside any panel or window,
widgets inside `area`, or a `menu_bar` outside a panel, because those
draw no layer of their own. For a heads-up display with buttons, put
the buttons in a panel, or test the button's `Response::rect` against
the pointer yourself.

A widget you click keeps keyboard focus until another widget takes it.
While a button has focus, Enter, Space and a gamepad's south button
click it again, and the arrow keys and d-pad move focus between
widgets, even when the game reads the same keys. When play resumes
after the player uses the interface, call `ctx.ui.clear_focus()`.

While the debug console is open, the engine gives the game an idle
input state instead; `ctx.input_captured()` reports that. The
[console guide](console.md) covers it.

## Keyboard and gamepad navigation

Every interactive widget can be reached without a mouse:

| Input | Effect |
|---|---|
| Tab, Shift+Tab | Move focus forwards or back in the order widgets were added. |
| Arrow keys, d-pad | Move focus to the nearest widget in that direction, unless the focused widget uses that key itself. |
| Enter, Space, gamepad south button | Activate the focused widget. |
| Escape, gamepad east button | Close the open popup or dialog, cancel a drag, or leave a text field. |

With nothing focused, the first press focuses the first widget, or the
last for Shift+Tab. Lists, tables, tabs and trees are one stop each and
use the arrows inside: Right and Left open and close the focused tree
branch. While a popup, menu or modal dialog is open, focus stays inside
it. The focus ring shows only after keyboard or gamepad input, and
hides again when the mouse moves.

To move focus from code, call `ctx.ui.set_focus(response.id)`, or set
`request_focus` in a text field's options. `ctx.ui.focused()` returns
the focused widget's id and `f.has_focus(id)` checks one.

## Text fields

`text_field(label, &mut text)` adds a single-line editor and
`text_area(label, &mut text, height)` a multi-line one that wraps to its
width. The response's `changed` says the text changed; in a single-line
field, `submitted` says the player pressed Enter. `text_field_with`
takes `TextOptions`:

| `TextOptions` field | Effect |
|---|---|
| `placeholder` | Dim text shown while the field is empty. |
| `mono` | Uses the monospaced font. |
| `frameless` | Draws no background or border. |
| `request_focus` | Takes keyboard focus this frame. |
| `max_len` | The most bytes the field accepts. |
| `caret_to_end` | Moves the caret to the end, after your code replaced the text. |

```rust,no_run
use yowie::ui::{Frame, TextOptions};

fn chat_input(f: &mut Frame<'_>, draft: &mut String, sent: &mut Vec<String>) {
    let options = TextOptions {
        placeholder: "Say something",
        max_len: Some(200),
        ..TextOptions::default()
    };
    if f.text_field_with("##chat", draft, &options).submitted && !draft.is_empty() {
        sent.push(std::mem::take(draft));
    }
}
```

The editors handle these keys. The shortcut key is Cmd on macOS and
Ctrl elsewhere.

| Keys | Effect |
|---|---|
| Arrows, Home, End | Move the caret; with Shift, extend the selection. |
| Ctrl or Alt with Left, Right, Backspace, Delete | Move or delete by word. |
| Shortcut or Ctrl with Home, End | Move to the start or end of the text. |
| Up, Down, PageUp, PageDown | Move between lines in a text area. |
| Shortcut+A, C, X, V | Select all, copy, cut, paste. |
| Shortcut+Z, Shortcut+Shift+Z or Shortcut+Y | Undo and redo, up to 100 steps. |
| Enter | Submits a field; starts a new line in a text area. |
| Escape | Drops focus. |

A double-click selects a word. Copy and paste use the system clipboard;
in headless runs they use an in-memory one. While a text field has
focus, the engine turns on the platform's input method and places its
candidate window at the caret, so players can type with input methods
for Chinese, Japanese, Korean and other languages. The text being
composed shows inline in the field until the player commits it.

## Drag and drop

To let the player drag something, call `drag_source(&response, payload,
label)` after the widget. Once the pointer moves a few units with the
button held, the drag starts and `label` follows the pointer. To accept
it, call `drop_target::<T>(&response)` after the receiving widget: it
returns the payload on the frame the button is released over that
widget, and outlines the widget while a matching drag is over it. The
payload can be any `'static` type; a target accepts only payloads of
its `T`. Escape cancels a drag.

```rust,no_run
use yowie::ui::Frame;

const ITEMS: [&str; 3] = ["Sword", "Shield", "Potion"];

fn equipment(f: &mut Frame<'_>, slots: &mut [Option<&'static str>; 2]) {
    f.columns(3, |f| {
        for item in ITEMS {
            let response = f.button(item);
            f.drag_source(&response, item, item);
        }
    });
    for (index, slot) in slots.iter_mut().enumerate() {
        let text = format!("Hand {}: {}", index + 1, slot.unwrap_or("empty"));
        let response = f.button(&format!("{text}##hand{index}"));
        if let Some(item) = f.drop_target::<&'static str>(&response) {
            *slot = Some(item);
        }
    }
}
```

Releasing a drag over its own source still reports `clicked` on that
widget; `drag_source` returns true while that widget's drag is in
progress, so check it to tell a drag from a click. `dragging::<T>()`
returns the payload in flight and `is_dragging()` says whether any drag
is under way.

## Themes

A `Theme` holds every colour and measure the widgets use. To change it,
call `ctx.ui.set_theme(theme)`; the next frame draws with it.

The built-in themes are `dark` (the default), `light`, `nord`,
`gruvbox`, `solarized-dark`, `solarized-light`, `sepia` and
`high-contrast`. `Theme::named(name)` returns one, or `None` for an
unknown name, and `theme_names()` lists them in menu order. The
`high-contrast` theme also draws thicker outlines and focus rings.

Panels and windows are translucent by default so the game shows through
them: `Theme::from_palette`, and so every built-in theme except
`high-contrast`, gives panels an alpha of 0.96. To make them opaque, set
the alpha of `theme.panel` to 1:

```rust
use yowie::ui::Theme;

fn solid_theme(name: &str) -> Theme {
    let mut theme = Theme::named(name).unwrap_or_default();
    theme.panel.a = 1.0;
    theme
}

fn main() {
    assert!(Theme::named("nord").is_some_and(|t| t.panel.a < 1.0));
    assert_eq!(solid_theme("nord").panel.a, 1.0);
}
```

To build a theme from your own colours, fill a `Palette` and call
`Theme::from_palette`. It derives the hover, pressed, field and track
colours, and picks black or white text for the accent colour. Then
change any field you want:

```rust
use yowie::Color;
use yowie::ui::{Palette, Theme};

fn marsh_theme() -> Theme {
    let mut theme = Theme::from_palette(Palette {
        background: Color::hex(0x1E2A1E),
        surface: Color::hex(0x2F3F2C),
        border: Color::hex(0x4C6B45),
        text: Color::hex(0xE4F0D8),
        text_dim: Color::hex(0x9DB08F),
        accent: Color::hex(0x9ACD32),
        title: Color::hex(0xC8E6A0),
    });
    theme.text_size = 16.0;
    theme.row_height = 28.0;
    theme
}

fn main() {
    assert_eq!(marsh_theme().text_size, 16.0);
}
```

| `Theme` field | Default | Meaning |
|---|---|---|
| `text`, `text_dim` | palette | Ordinary text; secondary, placeholder and disabled text. |
| `panel`, `border`, `title` | palette | Panel and window background (alpha 0.96), outlines, window titles. |
| `button` | palette `surface` | Idle button fill. |
| `button_hover`, `button_active` | derived | Hovered and pressed button fills. |
| `accent` | palette | Selections, focus rings and filled controls. |
| `on_accent` | derived | Text drawn on the accent colour. |
| `field`, `track` | derived | Text field and list background; slider, progress and scroll bar background. |
| `backdrop` | black at alpha 0.5 | Dimming behind modal dialogs. |
| `padding`, `spacing` | 8, 6 | Space inside panels and around button text; space between widgets. |
| `border_width`, `focus_width` | 1, 2 | Outline and focus ring thickness. |
| `row_height` | 26 | Minimum widget height. |
| `text_size`, `mono_size` | 15, 13 | Text sizes. |
| `skin` | `None` | Nine-slice textures; see [Skins](#skins). |

Measures are in view units. To use your own fonts, call
`ctx.ui.set_fonts(regular, mono)` with two fonts loaded as the
[text section of 2D graphics](graphics-2d.md#text) describes.

## Skins

To draw widgets from textures instead of flat colours, give the theme a
`Skin`. Each field is an optional `NineSlice` (see
[2D graphics](graphics-2d.md#nine-slices)); a widget whose slice is
`None` keeps drawing flat colours.

| `Skin` field | Draws |
|---|---|
| `panel` | Panels and windows. |
| `title` | Window title bars; falls back to `panel`. |
| `button` | Idle buttons, tinted to 90% brightness. |
| `button_hover` | Hovered buttons; falls back to `button` at full brightness. |
| `button_active` | Pressed buttons; falls back to `button` tinted to 70%. |
| `field` | Text fields and list backgrounds. |
| `track` | Slider, progress and scroll bar tracks. |
| `fill` | The filled part of sliders and progress bars, and scroll bar thumbs. |

Because slices are tinted by state, draw them in light colours. A
skinned panel draws its texture as it is, so its opacity comes from the
texture, not from `theme.panel`. Set `border_width` to 0 when the
textures draw their own edges.

```rust,no_run
use yowie::gfx::{NineSlice, TextureOptions};
use yowie::ui::{Skin, Theme};
use yowie::{Context, GameResult};

fn slice(ctx: &mut Context, path: &str) -> GameResult<Option<NineSlice>> {
    let texture = ctx.load_texture(path, TextureOptions::default())?;
    Ok(Some(NineSlice::uniform(texture, 4.0)))
}

fn wooden_theme(ctx: &mut Context) -> GameResult<Theme> {
    let skin = Skin {
        panel: slice(ctx, "ui/panel.png")?,
        button: slice(ctx, "ui/button.png")?,
        button_active: slice(ctx, "ui/button_down.png")?,
        field: slice(ctx, "ui/field.png")?,
        ..Skin::default()
    };
    let mut theme = Theme::dark().with_skin(skin);
    theme.border_width = 0.0;
    Ok(theme)
}
```

The gallery's `--skin` argument shows a skin built from small generated
bevel textures.

## Accessibility

`ctx.ui.accessibility()` returns the last frame's widgets as a list of
`AccessNode`s, in the order they were added. Each has the widget's
`id`, a `Role` (button, check box, slider, text field, menu item and so
on), its `name`, its `value` as text (a slider's number, a field's
contents), its `state` (focused, disabled, checked, expanded,
selected), its `rect`, and the index of its `parent`. The
`AccessTree` trait adds `find(role, name)` and `children(index)`. Use
the list to drive a screen reader bridge or text-to-speech, or to find
widgets in automated tests.

```rust,no_run
use yowie::Context;

/// What to announce when keyboard focus has moved to another widget.
fn focus_change(ctx: &Context, last: &mut String) -> Option<String> {
    let node = ctx.ui.accessibility().iter().find(|node| node.state.focused)?;
    if node.name == *last {
        return None;
    }
    *last = node.name.clone();
    Some(format!("{:?}: {}", node.role, node.name))
}
```

## Using the toolkit elsewhere

`Ui::draw_on(gfx, layer, input, body)` draws on a layer you choose
instead of `layers::UI`. To run the toolkit on your own renderer or in
tests, create a `Ui` with `Ui::new(theme)` and call `Ui::frame` with
your own implementation of the `Painter` trait.
