use super::*;
use yowie_core::vec2;

fn press(s: &mut InputState, key: Key) {
    s.apply(&InputEvent::KeyDown { key, repeat: false });
}

fn release(s: &mut InputState, key: Key) {
    s.apply(&InputEvent::KeyUp { key });
}

#[test]
fn press_between_updates_is_seen_by_the_first_update_only() {
    let mut s = InputState::new();
    press(&mut s, Key::Space);
    // Three updates run in one frame: only the first sees the edge.
    let seen: Vec<bool> = (0..3)
        .map(|_| {
            let p = s.key_pressed(Key::Space);
            s.end_update();
            p
        })
        .collect();
    assert_eq!(seen, [true, false, false]);
    assert!(s.key_down(Key::Space));
}

#[test]
fn press_survives_frames_without_updates() {
    let mut s = InputState::new();
    press(&mut s, Key::A);
    // Two frames draw with no update between them (turn-based or a fast
    // display); the update that finally runs still sees the press.
    for _ in 0..2 {
        s.begin_draw();
        s.end_draw();
    }
    assert!(s.key_pressed(Key::A));
}

#[test]
fn draw_sees_each_edge_once_per_frame() {
    let mut s = InputState::new();
    press(&mut s, Key::A);
    s.end_update();
    s.end_update();
    s.begin_draw();
    assert!(s.key_pressed(Key::A), "the frame covers both updates");
    s.end_draw();
    s.begin_draw();
    assert!(!s.key_pressed(Key::A), "a second frame does not repeat it");
    s.end_draw();
}

#[test]
fn tap_within_one_interval_reports_both_edges() {
    let mut s = InputState::new();
    press(&mut s, Key::J);
    release(&mut s, Key::J);
    assert!(s.key_pressed(Key::J));
    assert!(s.key_released(Key::J));
    assert!(!s.key_down(Key::J));
}

#[test]
fn repeats_are_separate_from_presses() {
    let cases = [
        // (events, pressed, repeated)
        (vec![(false, true)], true, false),
        (vec![(false, true), (true, true)], true, true),
        (vec![(true, true)], true, false), // a repeat with no press first counts as a press
    ];
    for (events, want_pressed, want_repeated) in cases {
        let mut s = InputState::new();
        for (repeat, _) in &events {
            s.apply(&InputEvent::KeyDown {
                key: Key::Down,
                repeat: *repeat,
            });
        }
        assert_eq!(s.key_pressed(Key::Down), want_pressed, "{events:?}");
        assert_eq!(s.key_repeated(Key::Down), want_repeated, "{events:?}");
    }
}

#[test]
fn held_time_counts_updates() {
    let mut s = InputState::new();
    s.set_update_step(0.5);
    press(&mut s, Key::F);
    s.end_update();
    s.end_update();
    assert_eq!(s.key_held(Key::F), 1.0);
    release(&mut s, Key::F);
    s.end_update();
    assert_eq!(s.key_held(Key::F), 0.0);
}

#[test]
fn mouse_delta_wheel_and_double_click() {
    let mut s = InputState::new();
    let at = |v: Vec2| InputEvent::MouseMoved {
        view: v,
        window: v * 2.0,
    };
    s.apply(&at(vec2(10.0, 10.0)));
    s.apply(&at(vec2(13.0, 14.0)));
    assert_eq!(s.mouse_delta(), vec2(3.0, 4.0));
    assert_eq!(s.mouse_window(), vec2(26.0, 28.0));
    s.apply(&InputEvent::Wheel {
        lines: vec2(0.0, 1.5),
    });
    assert_eq!(s.wheel(), vec2(0.0, 1.5));
    let click = |s: &mut InputState| {
        for pressed in [true, false] {
            s.apply(&InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed,
            });
        }
    };
    click(&mut s);
    s.end_update();
    click(&mut s);
    assert!(s.mouse_double_clicked(MouseButton::Left));
    s.end_update();
    click(&mut s);
    assert!(
        !s.mouse_double_clicked(MouseButton::Left),
        "a third click starts a new pair"
    );
}

#[test]
fn text_drops_control_characters() {
    let mut s = InputState::new();
    s.apply(&InputEvent::Text("a\r".into()));
    s.apply(&InputEvent::Text("é".into()));
    assert_eq!(s.text(), "aé");
    s.end_update();
    assert_eq!(s.text(), "");
}

#[test]
fn focus_loss_releases_everything() {
    let mut s = InputState::new();
    press(&mut s, Key::W);
    s.apply(&InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
    });
    s.end_update();
    s.apply(&InputEvent::FocusLost);
    assert!(!s.key_down(Key::W));
    assert!(s.key_released(Key::W));
    assert!(s.mouse_released(MouseButton::Right));
}

#[test]
fn gamepad_edges_and_connection() {
    let mut s = InputState::new();
    let id = GamepadId(1);
    s.apply(&InputEvent::GamepadConnected {
        id,
        name: "Pad".into(),
    });
    s.apply(&InputEvent::GamepadButton {
        id,
        button: GamepadButton::South,
        pressed: true,
    });
    s.apply(&InputEvent::GamepadAxis {
        id,
        axis: crate::GamepadAxis::LeftX,
        value: 2.0,
    });
    let pad = s.gamepad(id);
    assert!(pad.connected() && pad.just_connected());
    assert!(pad.pressed(GamepadButton::South) && pad.down(GamepadButton::South));
    assert_eq!(pad.axis(crate::GamepadAxis::LeftX), 1.0);
    assert_eq!(s.gamepads().count(), 1);
    s.end_update();
    assert!(!s.gamepad(id).pressed(GamepadButton::South));
    assert!(!s.gamepad(GamepadId(9)).connected());
}
