use super::*;
use crate::InputEvent;
use yowie_core::vec2;

fn key(s: &mut InputState, key: Key, down: bool) {
    s.apply(&if down {
        InputEvent::KeyDown { key, repeat: false }
    } else {
        InputEvent::KeyUp { key }
    });
}

fn pad(s: &mut InputState) -> GamepadId {
    let id = GamepadId(0);
    s.apply(&InputEvent::GamepadConnected {
        id,
        name: "Test".into(),
    });
    id
}

fn axis(s: &mut InputState, id: GamepadId, axis: GamepadAxis, value: f32) {
    s.apply(&InputEvent::GamepadAxis { id, axis, value });
}

#[test]
fn axis_action_sums_and_clamps() {
    let mut acts = ActionMap::new();
    let mx = acts.bind(
        "move_x",
        [
            Binding::from(Key::D),
            Source::Key(Key::A).negative(),
            Binding::from(GamepadAxis::LeftX),
            Source::PadAxis(GamepadAxis::LeftX).negative(),
        ],
    );
    let mut s = InputState::new();
    let id = pad(&mut s);
    let cases = [
        // (D, A, stick, want)
        (false, false, 0.0, 0.0),
        (true, false, 0.0, 1.0),
        (false, true, 0.0, -1.0),
        (true, true, 0.0, 0.0),
        (false, false, -0.6, -0.6),
        (false, false, 0.1, 0.0), // inside the dead zone
        (true, false, 0.7, 1.0),  // clamped
    ];
    for (d, a, stick, want) in cases {
        key(&mut s, Key::D, d);
        key(&mut s, Key::A, a);
        axis(&mut s, id, GamepadAxis::LeftX, stick);
        let got = acts.value(&s, mx);
        assert!((got - want).abs() < 1e-6, "{d} {a} {stick}: {got}");
    }
}

#[test]
fn vec2_composite_from_wasd_and_stick() {
    let mut acts = ActionMap::new();
    let mv = acts.bind_vec2("move", Binding2::wasd());
    acts.bind_vec2("move", Binding2::left_stick());
    let mut s = InputState::new();
    let id = pad(&mut s);
    key(&mut s, Key::W, true);
    assert_eq!(acts.vec2(&s, mv), vec2(0.0, -1.0));
    key(&mut s, Key::D, true);
    let v = acts.vec2(&s, "move");
    assert!((v.length() - 1.0).abs() < 1e-5, "diagonal is unit length");
    key(&mut s, Key::W, false);
    key(&mut s, Key::D, false);
    axis(&mut s, id, GamepadAxis::LeftY, 0.5);
    assert_eq!(acts.vec2(&s, mv), vec2(0.0, -0.5), "stick up is screen up");
}

#[test]
fn edges_ignore_a_second_source() {
    let mut acts = ActionMap::new();
    let jump = acts.bind("jump", [Key::Space, Key::J]);
    let mut s = InputState::new();
    key(&mut s, Key::Space, true);
    assert!(acts.pressed(&s, jump) && acts.down(&s, jump));
    s.end_update();
    key(&mut s, Key::J, true);
    assert!(!acts.pressed(&s, jump), "already held through Space");
    s.end_update();
    key(&mut s, Key::Space, false);
    assert!(!acts.released(&s, jump), "J still holds it");
    s.end_update();
    key(&mut s, Key::J, false);
    assert!(acts.released(&s, jump));
}

#[test]
fn axis_crossing_is_an_edge() {
    let mut acts = ActionMap::new();
    let fire = acts.bind("fire", [GamepadAxis::RightTrigger]);
    let mut s = InputState::new();
    let id = pad(&mut s);
    s.end_update();
    axis(&mut s, id, GamepadAxis::RightTrigger, 0.8);
    assert!(acts.pressed(&s, fire));
    s.end_update();
    assert!(!acts.pressed(&s, fire) && acts.down(&s, fire));
    axis(&mut s, id, GamepadAxis::RightTrigger, 0.0);
    assert!(acts.released(&s, fire));
}

#[test]
fn rebind_keeps_handles_and_listen_finds_input() {
    let mut acts = ActionMap::new();
    let jump = acts.bind("jump", [Key::Space]);
    let mut s = InputState::new();
    let id = pad(&mut s);
    s.end_update();
    axis(&mut s, id, GamepadAxis::LeftY, -0.9);
    let heard = acts.listen(&s);
    assert_eq!(heard, Some(Source::PadAxis(GamepadAxis::LeftY).negative()));
    s.end_update();
    key(&mut s, Key::K, true);
    let heard = acts.listen(&s);
    assert_eq!(heard, Some(Binding::from(Key::K)));
    if let Some(b) = heard {
        acts.rebind(jump, b);
    }
    assert!(acts.pressed(&s, jump));
    assert!(!acts.rebind_vec2("nothing", 0, Source::Key(Key::A)));
}

#[test]
fn serde_round_trip_preserves_order_and_handles() {
    let mut acts = ActionMap::new();
    let jump = acts.bind(
        "jump",
        [
            Binding::from(Key::Space),
            Binding::from(GamepadButton::South),
        ],
    );
    acts.bind_vec2("move", Binding2::arrows());
    acts.bind(
        "fire",
        [Source::PadAxis(GamepadAxis::RightTrigger).scaled(0.5)],
    );
    let json = serde_json::to_string(&acts).unwrap_or_default();
    assert!(
        json.starts_with(r#"{"jump":["key:Space","pad:South"],"move":{"vec2":"#),
        "{json}"
    );
    let loaded: ActionMap = serde_json::from_str(&json).unwrap_or_default();
    assert_eq!(loaded.names().collect::<Vec<_>>(), ["jump", "move", "fire"]);
    assert_eq!(loaded.bindings("move"), acts.bindings("move"));

    let mut target = ActionMap::new();
    let t_jump = target.bind("jump", [Key::J]);
    target.load_from(&loaded);
    assert_eq!(target.bindings(t_jump), acts.bindings(jump));
    assert!(serde_json::from_str::<ActionMap>(r#"{"x":["key:Nope"]}"#).is_err());
}
