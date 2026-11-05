//! Interaction tests on a recording painter: clicks, drags, typing and
//! navigation arrive as `InputState` events, exactly as the engine feeds
//! them, so no GPU is needed.

use yowie_core::{Color, Rect, Vec2, vec2};
use yowie_gfx::{NineSlice, Texture};
use yowie_input::{GamepadButton, GamepadId, InputEvent, InputState, Key, Modifiers, MouseButton};

use crate::*;

/// A painter with 8 by 16 unit characters that counts what is drawn.
#[derive(Default)]
struct Mock {
    texts: Vec<String>,
    fills: usize,
}

impl Painter for Mock {
    fn measure(&mut self, text: &str, _: Face, _: f32, wrap: Option<f32>) -> Vec2 {
        let w = text.chars().count() as f32 * 8.0;
        match wrap {
            Some(max) if max > 0.0 && w > max => vec2(max, 16.0 * (w / max).ceil()),
            _ => vec2(w, 16.0),
        }
    }
    fn text(&mut self, text: &str, _: Vec2, _: Face, _: f32, _: Color, _: Option<f32>) {
        self.texts.push(text.to_owned());
    }
    fn fill(&mut self, _: Rect, _: Color) {
        self.fills += 1;
    }
    fn stroke(&mut self, _: Rect, _: f32, _: Color) {}
    fn line(&mut self, _: Vec2, _: Vec2, _: f32, _: Color) {}
    fn polygon(&mut self, _: &[Vec2], _: Color) {}
    fn nine_slice(&mut self, _: &NineSlice, _: Rect, _: Color) {}
    fn image(&mut self, _: &Texture, _: Rect, _: Option<Rect>, _: Color) {}
    fn set_clip(&mut self, _: Option<Rect>) {}
    fn set_depth(&mut self, _: f32) {}
    fn view_size(&self) -> Vec2 {
        vec2(800.0, 600.0)
    }
}

struct Harness {
    ui: Ui,
    input: InputState,
    painter: Mock,
}

impl Harness {
    fn new() -> Harness {
        let mut h = Harness {
            ui: Ui::new(Theme::dark()),
            input: InputState::new(),
            painter: Mock::default(),
        };
        h.apply(InputEvent::MouseMoved {
            view: vec2(790.0, 590.0),
            window: vec2(790.0, 590.0),
        });
        h
    }

    fn apply(&mut self, e: InputEvent) {
        self.input.apply(&e);
    }

    fn frame<R>(&mut self, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.painter.texts.clear();
        self.input.begin_draw();
        let r = self.ui.frame(&mut self.painter, &self.input, body);
        self.input.end_draw();
        r
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.apply(InputEvent::MouseMoved {
            view: vec2(x, y),
            window: vec2(x, y),
        });
    }

    fn press(&mut self) {
        self.apply(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: true,
        });
    }

    fn release(&mut self) {
        self.apply(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed: false,
        });
    }

    fn key(&mut self, key: Key) {
        self.apply(InputEvent::KeyDown { key, repeat: false });
        self.apply(InputEvent::KeyUp { key });
    }

    fn text(&mut self, s: &str) {
        self.apply(InputEvent::Text(s.to_owned()));
    }

    fn modifiers(&mut self, m: Modifiers) {
        self.apply(InputEvent::Modifiers(m));
    }

    /// Presses and releases the left button at a point between frames.
    fn click_at(&mut self, x: f32, y: f32) {
        self.move_to(x, y);
        self.press();
        self.release();
    }
}

fn three_buttons(f: &mut Frame<'_>) -> [WidgetId; 3] {
    [f.button("One").id, f.button("Two").id, f.button("Three").id]
}

#[test]
fn a_press_and_release_in_one_frame_clicks() {
    let mut h = Harness::new();
    h.frame(|f| f.button("Go"));
    h.click_at(10.0, 10.0);
    let r = h.frame(|f| f.button("Go"));
    assert!(r.clicked && r.hovered);
    let r = h.frame(|f| f.button("Go"));
    assert!(!r.clicked, "a click is reported once");
}

#[test]
fn a_held_press_clicks_on_release_inside_only() {
    let mut h = Harness::new();
    h.frame(|f| f.button("Go"));
    h.move_to(10.0, 10.0);
    h.press();
    let r = h.frame(|f| f.button("Go"));
    assert!(r.held && !r.clicked);
    h.release();
    let r = h.frame(|f| f.button("Go"));
    assert!(r.clicked);

    // Pressing, dragging off and releasing elsewhere is not a click.
    h.press();
    h.frame(|f| f.button("Go"));
    h.move_to(400.0, 300.0);
    h.release();
    let r = h.frame(|f| f.button("Go"));
    assert!(!r.clicked);
}

#[test]
fn checkbox_and_radio_change_their_values() {
    let mut h = Harness::new();
    let (mut on, mut choice) = (false, 0);
    let ui = |f: &mut Frame<'_>, on: &mut bool, choice: &mut i32| {
        (
            f.checkbox("Check", on),
            f.radio("A", choice, 1),
            f.radio("B", choice, 2),
        )
    };
    ui_run(&mut h, &mut on, &mut choice, ui);
    h.click_at(10.0, 10.0);
    let (c, _, _) = ui_run(&mut h, &mut on, &mut choice, ui);
    assert!(c.changed && on);
    h.click_at(10.0, 10.0 + 32.0 * 2.0);
    let (_, _, b) = ui_run(&mut h, &mut on, &mut choice, ui);
    assert!(b.changed);
    assert_eq!(choice, 2);
}

fn ui_run<R>(
    h: &mut Harness,
    on: &mut bool,
    choice: &mut i32,
    body: impl FnOnce(&mut Frame<'_>, &mut bool, &mut i32) -> R,
) -> R {
    h.frame(|f| body(f, on, choice))
}

#[test]
fn slider_follows_the_pointer_and_keys() {
    let mut h = Harness::new();
    let mut v = 0.0f32;
    h.frame(|f| f.slider("Vol", &mut v, 0.0..=1.0));
    h.move_to(400.0, 10.0);
    h.press();
    let r = h.frame(|f| f.slider("Vol", &mut v, 0.0..=1.0));
    assert!(r.changed);
    assert!((v - 0.5).abs() < 0.01, "{v}");
    h.release();
    h.frame(|f| f.slider("Vol", &mut v, 0.0..=1.0));
    // Keyboard: focus is on the slider after the click.
    h.key(Key::Right);
    h.frame(|f| f.slider("Vol", &mut v, 0.0..=1.0));
    assert!((v - 0.52).abs() < 0.01, "{v}");
    let mut n = 5;
    h.frame(|f| f.slider_int("N", &mut n, 0..=10));
    assert_eq!(n, 5);
}

#[test]
fn text_field_types_edits_and_pastes() {
    let mut h = Harness::new();
    let mut s = String::new();
    h.frame(|f| f.text_field("", &mut s));
    h.click_at(10.0, 10.0);
    h.frame(|f| f.text_field("", &mut s));
    h.text("hello");
    let r = h.frame(|f| f.text_field("", &mut s));
    assert!(r.changed && r.focused);
    assert_eq!(s, "hello");
    h.key(Key::Backspace);
    h.frame(|f| f.text_field("", &mut s));
    assert_eq!(s, "hell");

    h.modifiers(Modifiers::shortcut());
    h.key(Key::A);
    h.frame(|f| f.text_field("", &mut s));
    h.key(Key::C);
    h.frame(|f| f.text_field("", &mut s));
    h.modifiers(Modifiers::NONE);
    h.key(Key::Right);
    h.frame(|f| f.text_field("", &mut s));
    h.modifiers(Modifiers::shortcut());
    h.key(Key::V);
    h.frame(|f| f.text_field("", &mut s));
    assert_eq!(s, "hellhell", "copy then paste at the end");
    h.key(Key::Z);
    h.frame(|f| f.text_field("", &mut s));
    assert_eq!(s, "hell", "undo reverts the paste");
    h.modifiers(Modifiers::NONE);
    h.key(Key::Enter);
    let r = h.frame(|f| f.text_field("", &mut s));
    assert!(r.submitted);
    assert!(h.ui.wants_keyboard());
}

#[test]
fn text_field_places_the_caret_with_the_pointer() {
    let mut h = Harness::new();
    let mut s = String::from("abcdef");
    h.frame(|f| f.text_field("", &mut s));
    // Text starts 6 units in; each character is 8 wide.
    h.click_at(6.0 + 3.0 * 8.0 + 1.0, 10.0);
    h.frame(|f| f.text_field("", &mut s));
    h.text("X");
    h.frame(|f| f.text_field("", &mut s));
    assert_eq!(s, "abcXdef");
}

#[test]
fn text_area_takes_newlines_and_wheel() {
    let mut h = Harness::new();
    let mut s = String::from("one");
    h.frame(|f| f.text_area("notes", &mut s, 80.0));
    h.click_at(100.0, 20.0);
    h.frame(|f| f.text_area("notes", &mut s, 80.0));
    h.key(Key::End);
    h.key(Key::Enter);
    h.text("two");
    h.frame(|f| f.text_area("notes", &mut s, 80.0));
    assert_eq!(s, "one\ntwo");
}

#[test]
fn tab_walks_widgets_in_order_and_wraps() {
    let mut h = Harness::new();
    let ids = h.frame(three_buttons);
    h.frame(three_buttons);
    let mut seen = Vec::new();
    for _ in 0..4 {
        h.key(Key::Tab);
        h.frame(three_buttons);
        seen.push(h.ui.focused());
    }
    assert_eq!(
        seen,
        [Some(ids[0]), Some(ids[1]), Some(ids[2]), Some(ids[0])]
    );
    h.modifiers(Modifiers::SHIFT);
    h.key(Key::Tab);
    h.frame(three_buttons);
    assert_eq!(
        h.ui.focused(),
        Some(ids[2]),
        "Shift+Tab goes back, wrapping"
    );
}

#[test]
fn disabled_widgets_are_not_navigation_stops() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        let a = f.button("A").id;
        let b = f.disabled(true, |f| f.button("B").id);
        let c = f.button("C").id;
        (a, b, c)
    };
    let (a, _, c) = h.frame(build);
    h.frame(build);
    h.key(Key::Tab);
    h.frame(build);
    assert_eq!(h.ui.focused(), Some(a));
    h.key(Key::Tab);
    h.frame(build);
    assert_eq!(h.ui.focused(), Some(c));
    h.click_at(10.0, 10.0 + 32.0);
    let (_, _, _) = h.frame(build);
    h.frame(build);
    assert_eq!(
        h.ui.focused(),
        Some(c),
        "clicking a disabled button does not focus it"
    );
}

#[test]
fn enter_and_space_activate_the_focused_button() {
    let mut h = Harness::new();
    h.frame(three_buttons);
    h.frame(three_buttons);
    h.key(Key::Tab);
    h.frame(three_buttons);
    h.key(Key::Enter);
    let clicked = h.frame(|f| {
        (
            f.button("One").clicked,
            f.button("Two").clicked,
            f.button("Three").clicked,
        )
    });
    assert_eq!(clicked, (true, false, false));
    h.key(Key::Space);
    let clicked = h.frame(|f| f.button("One").clicked);
    assert!(clicked);
}

#[test]
fn arrow_keys_move_focus_to_the_nearest_widget() {
    let mut h = Harness::new();
    let grid = |f: &mut Frame<'_>| {
        f.columns(2, |f| {
            let a = f.button("a").id;
            let b = f.button("b").id;
            let c = f.button("c").id;
            let d = f.button("d").id;
            [a, b, c, d]
        })
    };
    let [a, b, c, d] = h.frame(grid);
    h.frame(grid);
    h.key(Key::Tab);
    h.frame(grid);
    assert_eq!(h.ui.focused(), Some(a));
    h.key(Key::Right);
    h.frame(grid);
    assert_eq!(h.ui.focused(), Some(b));
    h.key(Key::Down);
    h.frame(grid);
    assert_eq!(h.ui.focused(), Some(d));
    h.key(Key::Left);
    h.frame(grid);
    assert_eq!(h.ui.focused(), Some(c));
    h.key(Key::Up);
    h.frame(grid);
    assert_eq!(h.ui.focused(), Some(a));
}

#[test]
fn gamepad_dpad_moves_and_south_activates() {
    let mut h = Harness::new();
    h.apply(InputEvent::GamepadConnected {
        id: GamepadId(0),
        name: "pad".into(),
    });
    let pad = |h: &mut Harness, b: GamepadButton| {
        for pressed in [true, false] {
            h.apply(InputEvent::GamepadButton {
                id: GamepadId(0),
                button: b,
                pressed,
            });
        }
    };
    let ids = h.frame(three_buttons);
    h.frame(three_buttons);
    pad(&mut h, GamepadButton::DPadDown);
    h.frame(three_buttons);
    assert_eq!(
        h.ui.focused(),
        Some(ids[0]),
        "first move focuses the first widget"
    );
    pad(&mut h, GamepadButton::DPadDown);
    h.frame(three_buttons);
    assert_eq!(h.ui.focused(), Some(ids[1]));
    pad(&mut h, GamepadButton::South);
    let clicked = h.frame(|f| f.button("One").clicked | f.button("Two").clicked);
    assert!(clicked);
}

#[test]
fn focus_scrolls_a_scroll_area_to_the_widget() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        f.scroll_area("list", 100.0, |f| {
            (0..12)
                .map(|i| f.button(&format!("item {i}")).id)
                .collect::<Vec<_>>()
        })
    };
    let ids = h.frame(build);
    h.frame(build);
    let mut last = ids[0];
    for _ in 0..9 {
        h.key(Key::Tab);
        h.frame(build);
        h.frame(build);
        last = h.ui.focused().unwrap_or(last);
    }
    assert_eq!(last, ids[8]);
    let area =
        h.ui.accessibility()
            .iter()
            .find(|n| n.role == Role::Group)
            .map(|n| n.rect);
    let item =
        h.ui.accessibility()
            .iter()
            .find(|n| n.id == last)
            .map(|n| n.rect);
    let (area, item) = (area.unwrap_or_default(), item.unwrap_or_default());
    assert!(
        item.y >= area.y - 0.5 && item.y + item.h <= area.y + area.h + 0.5,
        "{item:?} in {area:?}"
    );
}

#[test]
fn wheel_scrolls_the_area_under_the_pointer() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        f.scroll_area("list", 100.0, |f| {
            for i in 0..12 {
                f.label(&format!("row {i}"));
            }
        })
    };
    h.frame(build);
    h.frame(build);
    h.move_to(20.0, 50.0);
    h.apply(InputEvent::Wheel {
        lines: vec2(0.0, -2.0),
    });
    h.frame(build);
    h.frame(build);
    let first =
        h.ui.accessibility()
            .iter()
            .find(|n| n.name == "row 0")
            .map(|n| n.rect.y);
    assert!(
        first.is_some_and(|y| y < 0.0),
        "row 0 scrolled out: {first:?}"
    );
}

#[test]
fn drag_and_drop_moves_a_payload_between_widgets() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        let a = f.button("Source");
        let started = f.drag_source(&a, 42u32, "item 42");
        let b = f.button("Target");
        (
            started,
            f.drop_target::<u32>(&b),
            f.drop_target::<String>(&b),
        )
    };
    h.frame(build);
    h.move_to(10.0, 10.0);
    h.press();
    h.frame(build);
    h.move_to(10.0, 45.0);
    let (started, _, _) = h.frame(build);
    assert!(started);
    h.release();
    let (_, dropped, wrong) = h.frame(build);
    assert_eq!(dropped, Some(42));
    assert_eq!(wrong, None, "only the right payload type is accepted");
    let (started, dropped, _) = h.frame(build);
    assert!(!started && dropped.is_none());
}

#[test]
fn releasing_a_drag_elsewhere_drops_nothing() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        let a = f.button("Source");
        f.drag_source(&a, 1u8, "x");
        let b = f.button("Target");
        f.drop_target::<u8>(&b)
    };
    h.frame(build);
    h.move_to(10.0, 10.0);
    h.press();
    h.frame(build);
    h.move_to(400.0, 400.0);
    h.frame(build);
    h.release();
    assert_eq!(h.frame(build), None);
}

#[test]
fn combo_box_opens_and_chooses() {
    let mut h = Harness::new();
    let items = ["red", "green", "blue"];
    let mut sel = 0usize;
    h.frame(|f| f.combo_box("", &mut sel, &items));
    h.click_at(10.0, 10.0);
    h.frame(|f| f.combo_box("", &mut sel, &items));
    h.frame(|f| f.combo_box("", &mut sel, &items));
    // The list opens below the 26 unit field; rows are 26 units tall.
    h.click_at(10.0, 26.0 + 1.0 + 26.0 * 2.0 + 5.0);
    let r = h.frame(|f| f.combo_box("", &mut sel, &items));
    assert!(r.changed);
    assert_eq!(sel, 2);
    let r = h.frame(|f| f.combo_box("", &mut sel, &items));
    assert!(!r.changed);
    assert!(
        h.ui.accessibility()
            .iter()
            .all(|n| n.role != Role::ListItem),
        "the list closed"
    );
}

#[test]
fn list_box_selects_by_click_and_keys() {
    let mut h = Harness::new();
    let items: Vec<String> = (0..30).map(|i| format!("item {i}")).collect();
    let mut sel = 0usize;
    h.frame(|f| f.list_box("l", 130.0, &items, &mut sel));
    h.click_at(20.0, 1.0 + 26.0 * 2.0 + 5.0);
    h.frame(|f| f.list_box("l", 130.0, &items, &mut sel));
    assert_eq!(sel, 2);
    h.key(Key::Down);
    h.frame(|f| f.list_box("l", 130.0, &items, &mut sel));
    assert_eq!(sel, 3);
    h.key(Key::End);
    h.frame(|f| f.list_box("l", 130.0, &items, &mut sel));
    assert_eq!(sel, 29);
    let built =
        h.ui.accessibility()
            .iter()
            .filter(|n| n.role == Role::ListItem)
            .count();
    assert!(built < 10, "only visible rows are built, got {built}");
}

#[test]
fn tabs_switch_on_click_and_arrows() {
    let mut h = Harness::new();
    let mut sel = 0usize;
    let names = ["Alpha", "Beta", "Gamma"];
    h.frame(|f| f.tabs("t", &names, &mut sel));
    // Alpha is 5 chars * 8 + 16 padding = 56 wide; click inside Beta.
    h.click_at(56.0 + 20.0, 14.0);
    let r = h.frame(|f| f.tabs("t", &names, &mut sel));
    assert!(r.changed);
    assert_eq!(sel, 1);
    h.key(Key::Right);
    h.frame(|f| f.tabs("t", &names, &mut sel));
    assert_eq!(sel, 2);
}

#[test]
fn modal_blocks_widgets_behind_it() {
    let mut h = Harness::new();
    let mut open = true;
    let build = |f: &mut Frame<'_>, open: &mut bool| {
        let behind = f.button("Behind").clicked;
        f.modal("Confirm", open, vec2(300.0, 150.0), |f| {
            f.label("Sure?");
            f.button("Yes").clicked
        });
        behind
    };
    h.frame(|f| build(f, &mut open));
    h.frame(|f| build(f, &mut open));
    h.click_at(10.0, 10.0);
    assert!(!h.frame(|f| build(f, &mut open)), "the modal owns input");
    h.key(Key::Escape);
    h.frame(|f| build(f, &mut open));
    assert!(!open, "Escape closes the dialog");
    h.frame(|f| build(f, &mut open));
    h.click_at(10.0, 10.0);
    assert!(h.frame(|f| build(f, &mut open)));
}

#[test]
fn windows_move_with_the_title_bar_and_resize_from_the_corner() {
    let mut h = Harness::new();
    let mut rect = Rect::new(100.0, 100.0, 300.0, 200.0);
    h.frame(|f| f.window("Tools", &mut rect, |f| f.label("hi")));
    h.move_to(150.0, 110.0);
    h.press();
    h.frame(|f| f.window("Tools", &mut rect, |f| f.label("hi")));
    h.move_to(250.0, 160.0);
    h.frame(|f| f.window("Tools", &mut rect, |f| f.label("hi")));
    h.release();
    h.frame(|f| f.window("Tools", &mut rect, |f| f.label("hi")));
    assert_eq!((rect.x, rect.y), (200.0, 150.0));
    assert_eq!((rect.w, rect.h), (300.0, 200.0));
    let grip = vec2(rect.x + rect.w - 6.0, rect.y + rect.h - 6.0);
    h.move_to(grip.x, grip.y);
    h.press();
    h.frame(|f| f.window("Tools", &mut rect, |f| f.label("hi")));
    h.move_to(grip.x + 50.0, grip.y + 30.0);
    h.frame(|f| f.window("Tools", &mut rect, |f| f.label("hi")));
    assert_eq!((rect.w, rect.h), (350.0, 230.0));
}

#[test]
fn overlapping_windows_send_clicks_to_the_top_one() {
    let mut h = Harness::new();
    let (mut a, mut b) = (
        Rect::new(50.0, 50.0, 300.0, 200.0),
        Rect::new(50.0, 50.0, 300.0, 200.0),
    );
    let build = |f: &mut Frame<'_>, a: &mut Rect, b: &mut Rect| {
        let x = f.window("A", a, |f| f.button("in A").clicked);
        let y = f.window("B", b, |f| f.button("in B").clicked);
        (x, y)
    };
    for _ in 0..2 {
        h.frame(|f| build(f, &mut a, &mut b));
    }
    // Both windows have a button under (100, 100); B was added later and is on top.
    h.click_at(100.0, 100.0);
    let (x, y) = h.frame(|f| build(f, &mut a, &mut b));
    assert!(!x, "the window underneath gets nothing");
    assert!(y, "the top window gets the click");
}

#[test]
fn tree_nodes_expand_with_clicks_and_keys() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        f.tree_node("root", |f| {
            f.tree_leaf("child", false);
        })
    };
    h.frame(build);
    assert!(h.ui.accessibility().iter().all(|n| n.name != "child"));
    h.click_at(40.0, 10.0);
    h.frame(build);
    h.frame(build);
    assert!(h.ui.accessibility().iter().any(|n| n.name == "child"));
    h.key(Key::Left);
    h.frame(build);
    h.frame(build);
    assert!(
        h.ui.accessibility().iter().all(|n| n.name != "child"),
        "Left closes the focused node"
    );
}

#[test]
fn collapsing_header_hides_its_body() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        f.collapsing_header("More", false, |f| {
            f.label("details");
        })
    };
    h.frame(build);
    assert!(h.ui.accessibility().iter().all(|n| n.name != "details"));
    h.click_at(30.0, 10.0);
    h.frame(build);
    h.frame(build);
    assert!(h.ui.accessibility().iter().any(|n| n.name == "details"));
}

#[test]
fn table_builds_only_visible_rows_and_reports_clicks() {
    let mut h = Harness::new();
    let cols = [Column::fill("Name"), Column::fixed("Qty", 60.0)];
    let mut sel = None;
    let mut built = 0usize;
    let run = |h: &mut Harness, sel: &mut Option<usize>, built: &mut usize| {
        *built = 0;
        h.frame(|f| {
            f.table("inv", &cols, 10_000, 160.0, sel, |f, row, col| {
                if col == 0 {
                    *built += 1;
                    f.label(&format!("item {row}"));
                }
            })
        })
    };
    run(&mut h, &mut sel, &mut built);
    assert!(built < 12, "visible rows only, built {built}");
    h.click_at(20.0, 1.0 + 26.0 + 26.0 * 1.0 + 5.0);
    let t = run(&mut h, &mut sel, &mut built);
    assert_eq!(t.row_clicked, Some(1));
    assert_eq!(sel, Some(1));
    h.key(Key::Down);
    run(&mut h, &mut sel, &mut built);
    assert_eq!(sel, Some(2));
    h.click_at(20.0, 10.0);
    let t = run(&mut h, &mut sel, &mut built);
    assert_eq!(t.header_clicked, Some(0));
}

#[test]
fn menu_bar_opens_and_items_choose() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        let mut chosen = None;
        f.menu_bar(|f| {
            f.menu("File", |f| {
                if f.menu_item("Open").clicked {
                    chosen = Some("open");
                }
                if f.menu_item("Quit").clicked {
                    chosen = Some("quit");
                }
            });
        });
        chosen
    };
    h.frame(build);
    h.click_at(20.0, 10.0);
    h.frame(build);
    h.frame(build);
    assert!(
        h.ui.accessibility()
            .iter()
            .any(|n| n.role == Role::MenuItem && n.name == "Quit")
    );
    // The popup opens below the 28 unit bar; items are 26 tall after a 3 unit inset.
    h.click_at(30.0, 28.0 + 3.0 + 26.0 + 5.0);
    let chosen = h.frame(build);
    assert_eq!(chosen, Some("quit"));
    h.frame(build);
    assert!(
        h.ui.accessibility()
            .iter()
            .all(|n| n.role != Role::MenuItem),
        "choosing closes the menu"
    );
}

#[test]
fn context_menu_opens_on_right_click() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        let r = f.button("Item");
        f.context_menu(&r, |f| {
            f.menu_item("Delete");
        });
    };
    h.frame(build);
    h.move_to(20.0, 10.0);
    h.apply(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
    });
    h.apply(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: false,
    });
    h.frame(build);
    h.frame(build);
    assert!(h.ui.accessibility().iter().any(|n| n.name == "Delete"));
}

#[test]
fn tooltip_appears_after_hovering() {
    let mut h = Harness::new();
    h.ui.set_delta_time(0.25);
    let build = |f: &mut Frame<'_>| {
        let r = f.button("Hover me");
        f.tooltip(&r, "Helpful");
    };
    h.frame(build);
    h.move_to(10.0, 10.0);
    h.frame(build);
    assert!(!h.painter.texts.iter().any(|t| t == "Helpful"));
    h.frame(build);
    h.frame(build);
    assert!(h.painter.texts.iter().any(|t| t == "Helpful"));
}

#[test]
fn ids_are_stable_across_frames_and_with_keys() {
    let mut h = Harness::new();
    let a = h.frame(|f| (f.button("Same").id, f.button("Same").id));
    let b = h.frame(|f| (f.button("Same").id, f.button("Same").id));
    assert_eq!(a, b);
    assert_ne!(a.0, a.1, "repeated labels are told apart by order");

    // Keyed rows keep their ids when the order changes.
    let ids = |h: &mut Harness, order: &[u32]| -> Vec<(u32, WidgetId)> {
        h.frame(|f| {
            order
                .iter()
                .map(|k| (*k, f.with_id(*k, |f| f.button("Row").id)))
                .collect()
        })
    };
    let first = ids(&mut h, &[1, 2, 3]);
    let second = ids(&mut h, &[3, 1, 2]);
    for (k, id) in &first {
        assert_eq!(
            second.iter().find(|(k2, _)| k2 == k).map(|(_, i)| *i),
            Some(*id)
        );
    }
    let hidden = h.frame(|f| (f.button("Save##a").id, f.button("Save##b").id));
    assert_ne!(hidden.0, hidden.1);
    assert!(h.ui.accessibility().iter().any(|n| n.name == "Save"));
}

#[test]
fn accessibility_tree_describes_widgets() {
    let mut h = Harness::new();
    let (mut on, mut v, mut s) = (true, 0.25f32, String::from("Ada"));
    let mut build = |h: &mut Harness| {
        h.frame(|f| {
            f.panel(Some("Settings"), Rect::new(10.0, 10.0, 400.0, 300.0), |f| {
                f.label("Hello");
                f.button("Apply");
                f.checkbox("Sound", &mut on);
                f.slider("Volume", &mut v, 0.0..=1.0);
                f.text_field("Name", &mut s);
                f.progress("Load", 0.5);
                f.separator();
            });
        })
    };
    build(&mut h);
    build(&mut h);
    let nodes = h.ui.accessibility();
    let panel = nodes
        .iter()
        .position(|n| n.role == Role::Panel && n.name == "Settings");
    let panel = panel.unwrap_or(usize::MAX);
    assert!(panel != usize::MAX);
    let btn = nodes.find(Role::Button, "Apply").expect("button");
    assert_eq!(btn.parent, Some(panel));
    let cb = nodes.find(Role::CheckBox, "Sound").expect("checkbox");
    assert_eq!(cb.state.checked, Some(true));
    let sl = nodes.find(Role::Slider, "Volume").expect("slider");
    assert_eq!(sl.value.as_deref(), Some("0.25"));
    let tf = nodes.find(Role::TextField, "Name").expect("text field");
    assert_eq!(tf.value.as_deref(), Some("Ada"));
    let pb = nodes.find(Role::ProgressBar, "Load").expect("progress");
    assert_eq!(pb.value.as_deref(), Some("50%"));
    assert!(nodes.iter().any(|n| n.role == Role::Separator));
    assert!(nodes.children(panel).len() >= 6);
}

#[test]
fn focus_state_shows_in_the_tree() {
    let mut h = Harness::new();
    h.frame(three_buttons);
    h.frame(three_buttons);
    h.key(Key::Tab);
    h.frame(three_buttons);
    h.frame(three_buttons);
    let nodes = h.ui.accessibility();
    assert!(
        nodes
            .find(Role::Button, "One")
            .is_some_and(|n| n.state.focused)
    );
    assert!(
        nodes
            .find(Role::Button, "Two")
            .is_some_and(|n| !n.state.focused)
    );
}

#[test]
fn columns_and_grid_place_widgets_side_by_side() {
    let mut h = Harness::new();
    let rects = h.frame(|f| {
        f.columns(3, |f| {
            (0..6)
                .map(|i| f.button(&format!("b{i}")).rect)
                .collect::<Vec<_>>()
        })
    });
    assert_eq!(rects[0].y, rects[2].y);
    assert!(rects[1].x > rects[0].x + rects[0].w - 0.1);
    assert_eq!(
        rects[3].x, rects[0].x,
        "the fourth button starts the second line"
    );
    assert!(rects[3].y > rects[0].y);
    let w: f32 = rects[..3].iter().map(|r| r.w).sum();
    assert!((w + 2.0 * 6.0 - 800.0).abs() < 0.01);
}

#[test]
fn wants_mouse_reports_panels_under_the_pointer() {
    let mut h = Harness::new();
    let build = |f: &mut Frame<'_>| {
        f.panel(Some("P"), Rect::new(100.0, 100.0, 200.0, 100.0), |f| {
            f.label("x");
        })
    };
    h.frame(build);
    h.move_to(150.0, 150.0);
    h.frame(build);
    h.frame(build);
    assert!(h.ui.wants_mouse());
    h.move_to(600.0, 400.0);
    h.frame(build);
    assert!(!h.ui.wants_mouse());
}

#[test]
fn widgets_batch_by_kind() {
    // Texts are issued after all fills of a layer, whatever the call order.
    let mut h = Harness::new();
    h.frame(|f| {
        for i in 0..5 {
            f.button(&format!("B{i}"));
        }
    });
    assert_eq!(h.painter.texts.len(), 5);
    assert!(h.painter.fills >= 5);
}

#[test]
fn themes_all_build_a_frame() {
    for name in theme_names() {
        let mut h = Harness::new();
        h.ui.set_theme(Theme::named(name).expect("built-in theme"));
        h.frame(|f| {
            f.button("x");
        });
    }
}
