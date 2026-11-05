//! The console drawn on a headless GPU and driven by input events.
//! Skipped without a GPU adapter.

use yowie_core::{uvec2, vec2};
use yowie_gfx::{Graphics, GraphicsSettings};
use yowie_input::{InputEvent, InputState, Key};

use crate::{Console, ConsoleOptions};

struct Rig {
    gfx: Graphics,
    input: InputState,
    console: Console,
}

impl Rig {
    fn new() -> Option<Rig> {
        if !yowie_gfx::adapter_available() {
            println!("skipping GPU test: no GPU adapter");
            return None;
        }
        let gfx = Graphics::new_headless(uvec2(640, 360), GraphicsSettings::default()).ok()?;
        let mut input = InputState::new();
        input.apply(&InputEvent::MouseMoved {
            view: vec2(600.0, 340.0),
            window: vec2(600.0, 340.0),
        });
        Some(Rig {
            gfx,
            input,
            console: Console::new(ConsoleOptions::default()),
        })
    }

    fn frame(&mut self) {
        self.gfx.begin_frame();
        self.input.begin_draw();
        self.console.draw(&mut self.gfx, &self.input, 1.0 / 60.0);
        self.input.end_draw();
        let _ = self.gfx.end_frame();
    }

    fn key(&mut self, key: Key) {
        self.input
            .apply(&InputEvent::KeyDown { key, repeat: false });
        self.input.apply(&InputEvent::KeyUp { key });
    }

    fn type_line(&mut self, text: &str) {
        self.input.apply(&InputEvent::Text(text.to_owned()));
        self.frame();
        self.key(Key::Enter);
        self.frame();
    }
}

#[test]
fn backtick_opens_the_console_and_a_typed_command_runs() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let speed = rig.console.float("player.speed", 1.0, "");
    rig.frame();
    assert!(!rig.console.is_open());
    rig.key(Key::Grave);
    rig.frame();
    assert!(rig.console.is_open());
    rig.frame();
    rig.type_line("set player.speed 7");
    assert_eq!(speed.get(), 7.0);
    let lines = rig.console.lines(10);
    assert!(lines.iter().any(|l| l.text == "> set player.speed 7"));
    assert!(lines.iter().any(|l| l.text == "player.speed = 7"));
    rig.key(Key::Grave);
    rig.frame();
    assert!(!rig.console.is_open());
}

#[test]
fn the_backtick_is_not_typed_into_the_line() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    rig.input.apply(&InputEvent::Text("`".into()));
    rig.key(Key::Grave);
    rig.frame();
    rig.frame();
    rig.type_line("echo ok");
    assert!(rig.console.lines(10).iter().any(|l| l.text == "> echo ok"));
}

#[test]
fn bindings_fire_only_while_the_console_is_shut() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let hits = rig.console.int("hits", 0, "");
    rig.console.bind(Key::F5, "set hits 1");
    rig.key(Key::F5);
    rig.frame();
    assert_eq!(hits.get(), 1);
    hits.set(0);
    rig.key(Key::Grave);
    rig.frame();
    rig.key(Key::F5);
    rig.frame();
    assert_eq!(hits.get(), 0, "typing F5 into an open console runs nothing");
}

#[test]
fn history_and_completion_edit_the_line() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    rig.key(Key::Grave);
    rig.frame();
    rig.frame();
    rig.type_line("echo first");
    rig.input.apply(&InputEvent::Text("ec".into()));
    rig.frame();
    rig.key(Key::Tab);
    rig.frame();
    assert_eq!(rig.console.core.line, "echo ");
    rig.key(Key::Up);
    rig.frame();
    assert_eq!(rig.console.core.line, "echo first");
}

#[test]
fn panels_open_on_f4_and_draw_every_tab() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    rig.console.add_panel("Game", |f| {
        f.label("game panel");
    });
    rig.console.add_info("seed", || "1234".to_owned());
    rig.console.record_frame(crate::FrameTimes {
        fps: 60.0,
        frame_ms: 16.6,
        update_ms: 1.0,
        draw_ms: 2.0,
        ..crate::FrameTimes::default()
    });
    rig.key(Key::F4);
    rig.frame();
    assert!(rig.console.panels_open());
    let names = rig.console.core.panel_names();
    assert_eq!(names, ["Engine", "GPU", "Input", "Services", "Game"]);
    for i in 0..names.len() {
        rig.console.core.tab = i;
        rig.frame();
        rig.frame();
    }
    rig.console.execute("panels Services");
    assert_eq!(rig.console.core.tab, 3);
    rig.key(Key::F4);
    rig.frame();
    assert!(!rig.console.panels_open());
}

#[test]
fn a_disabled_console_draws_nothing_and_ignores_keys() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    rig.console = Console::disabled();
    rig.key(Key::Grave);
    rig.frame();
    assert!(!rig.console.is_open());
}
