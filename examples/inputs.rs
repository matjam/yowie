//! Everything the input state reports: keys held, mouse buttons, wheel,
//! position in view units and window pixels, raw motion while the cursor is
//! locked (C toggles, Escape releases), full screen (F), typed text with
//! input-method composition, up to four gamepads, and an action map moving
//! a dot with WASD, the arrows or a stick (Space or the south button
//! flashes it).

use yowie::gfx::{Font, TextStyle};
use yowie::input::{
    ActionId, ActionMap, Binding, Binding2, GamepadAxis, GamepadButton, Key, MouseButton,
};
use yowie::{Color, Config, Context, CursorGrab, Game, GameResult, Rect, Vec2, vec2};

const KEY_ROWS: [&[Key]; 6] = [
    &[
        Key::Escape,
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
        Key::F11,
        Key::F12,
    ],
    &[
        Key::Grave,
        Key::Digit1,
        Key::Digit2,
        Key::Digit3,
        Key::Digit4,
        Key::Digit5,
        Key::Digit6,
        Key::Digit7,
        Key::Digit8,
        Key::Digit9,
        Key::Digit0,
        Key::Minus,
        Key::Equal,
        Key::Backspace,
    ],
    &[
        Key::Tab,
        Key::Q,
        Key::W,
        Key::E,
        Key::R,
        Key::T,
        Key::Y,
        Key::U,
        Key::I,
        Key::O,
        Key::P,
        Key::LeftBracket,
        Key::RightBracket,
        Key::Backslash,
    ],
    &[
        Key::CapsLock,
        Key::A,
        Key::S,
        Key::D,
        Key::F,
        Key::G,
        Key::H,
        Key::J,
        Key::K,
        Key::L,
        Key::Semicolon,
        Key::Apostrophe,
        Key::Enter,
    ],
    &[
        Key::LeftShift,
        Key::Z,
        Key::X,
        Key::C,
        Key::V,
        Key::B,
        Key::N,
        Key::M,
        Key::Comma,
        Key::Period,
        Key::Slash,
        Key::RightShift,
    ],
    &[
        Key::LeftControl,
        Key::LeftAlt,
        Key::LeftSuper,
        Key::Space,
        Key::RightSuper,
        Key::RightAlt,
        Key::Left,
        Key::Up,
        Key::Down,
        Key::Right,
    ],
];

struct Inputs {
    font: Font,
    actions: ActionMap,
    movement: ActionId,
    jump: ActionId,
    dot: Vec2,
    flash: f32,
    typed: String,
    wheel: Vec2,
    motion: Vec2,
    grabbed: bool,
}

impl Game for Inputs {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let mut actions = ActionMap::new();
        let movement = actions.bind_vec2("move", Binding2::wasd());
        actions.bind_vec2("move", Binding2::arrows());
        actions.bind_vec2("move", Binding2::left_stick());
        let jump = actions.bind(
            "jump",
            [
                Binding::from(Key::Space),
                Binding::from(GamepadButton::South),
            ],
        );
        ctx.set_text_input(true);
        Ok(Inputs {
            font: ctx.gfx.default_mono_font(),
            actions,
            movement,
            jump,
            dot: vec2(820.0, 380.0),
            flash: 0.0,
            typed: String::new(),
            wheel: Vec2::ZERO,
            motion: Vec2::ZERO,
            grabbed: false,
        })
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = ctx.time.delta();
        let input = &ctx.input;
        self.dot += self.actions.vec2(input, self.movement) * 240.0 * dt;
        self.dot = self.dot.clamp(vec2(700.0, 300.0), vec2(940.0, 460.0));
        if self.actions.pressed(input, self.jump) {
            self.flash = 1.0;
        }
        self.flash = (self.flash - dt * 2.0).max(0.0);
        self.typed.push_str(input.text());
        if input.key_pressed(Key::Backspace) {
            self.typed.pop();
        }
        if self.typed.chars().count() > 60 {
            self.typed = self.typed.chars().skip(20).collect();
        }
        self.wheel += input.wheel();
        self.motion += input.mouse_motion();
        let (c, esc, f) = (
            input.key_pressed(Key::C),
            input.key_pressed(Key::Escape),
            input.key_pressed(Key::F),
        );
        if c || (esc && self.grabbed) {
            self.grabbed = c && !self.grabbed;
            let grab = if self.grabbed {
                CursorGrab::Locked
            } else {
                CursorGrab::None
            };
            if let Err(e) = ctx.set_cursor_grab(grab) {
                println!("cursor grab: {e}");
            }
            ctx.set_cursor_visible(!self.grabbed);
        }
        if f {
            let on = !ctx.fullscreen();
            ctx.set_fullscreen(on);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let (g, input) = (&mut ctx.gfx, &ctx.input);
        let label = TextStyle::new(13.0).color(Color::rgb8(220, 220, 230));
        let dim = Color::rgb8(50, 56, 72);
        let lit = Color::rgb8(250, 190, 70);

        // The keyboard.
        for (row, keys) in KEY_ROWS.iter().enumerate() {
            let mut x = 20.0 + row as f32 * 12.0;
            for key in keys.iter() {
                let w = if short(*key).len() > 2 { 58.0 } else { 40.0 };
                let r = Rect::new(x, 20.0 + row as f32 * 44.0, w, 38.0);
                g.fill_rect(r, if input.key_down(*key) { lit } else { dim });
                g.draw_text(&self.font, short(*key), r.min() + vec2(5.0, 4.0), &label);
                x += w + 4.0;
            }
        }

        // The mouse.
        let m = input.mouse();
        let text = format!(
            "mouse view {:.0},{:.0}  window {:.0},{:.0}\nwheel {:.1},{:.1}  motion {:.0},{:.0}\ncursor {} (C toggles, Esc releases)  F full screen",
            m.x,
            m.y,
            input.mouse_window().x,
            input.mouse_window().y,
            self.wheel.x,
            self.wheel.y,
            self.motion.x,
            self.motion.y,
            if self.grabbed { "locked" } else { "free" },
        );
        g.draw_text(&self.font, &text, vec2(20.0, 300.0), &label);
        for (i, b) in MouseButton::ALL.into_iter().enumerate() {
            let r = Rect::new(20.0 + i as f32 * 84.0, 360.0, 80.0, 26.0);
            g.fill_rect(r, if input.mouse_down(b) { lit } else { dim });
            g.draw_text(&self.font, b.name(), r.min() + vec2(6.0, 5.0), &label);
        }

        // Typed text and composition.
        let typed = format!("typed: {}_{}", self.typed, input.composition());
        g.draw_text(&self.font, &typed, vec2(20.0, 400.0), &label);

        // Gamepads.
        for slot in 0..4u8 {
            let pad = input.gamepad(yowie::input::GamepadId(slot));
            let x = 20.0 + slot as f32 * 170.0;
            let name = if pad.connected() {
                pad.name()
            } else {
                "no gamepad"
            };
            g.draw_text(
                &self.font,
                &format!("pad {slot}: {name}"),
                vec2(x, 440.0),
                &label,
            );
            for (stick, (ax, ay)) in [
                (GamepadAxis::LeftX, GamepadAxis::LeftY),
                (GamepadAxis::RightX, GamepadAxis::RightY),
            ]
            .into_iter()
            .enumerate()
            {
                let c = vec2(x + 30.0 + stick as f32 * 70.0, 490.0);
                g.stroke_circle(c, 22.0, 2.0, dim);
                let p = c + vec2(pad.axis(ax), -pad.axis(ay)) * 20.0;
                g.fill_circle(p, 5.0, lit);
            }
            let pressed = GamepadButton::ALL.iter().filter(|b| pad.down(**b)).count();
            let trig = format!(
                "L{:.1} R{:.1} {pressed}b",
                pad.axis(GamepadAxis::LeftTrigger),
                pad.axis(GamepadAxis::RightTrigger)
            );
            g.draw_text(&self.font, &trig, vec2(x, 516.0), &label);
        }

        // The action map's dot.
        g.stroke_rect(Rect::new(690.0, 290.0, 260.0, 180.0), 2.0, dim);
        let color = Color::rgb8(90, 200, 250).lerp(Color::WHITE, self.flash);
        g.fill_circle(self.dot, 10.0 + self.flash * 8.0, color);
        g.draw_text(
            &self.font,
            "move: WASD, arrows, stick",
            vec2(700.0, 476.0),
            &label,
        );
        Ok(())
    }
}

/// A key's label on the drawn keyboard.
fn short(key: Key) -> &'static str {
    match key {
        Key::Escape => "Esc",
        Key::Backspace => "Bksp",
        Key::LeftBracket => "[",
        Key::RightBracket => "]",
        Key::Backslash => "\\",
        Key::CapsLock => "Caps",
        Key::Semicolon => ";",
        Key::Apostrophe => "'",
        Key::Grave => "`",
        Key::Minus => "-",
        Key::Equal => "=",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::LeftShift | Key::RightShift => "Shift",
        Key::LeftControl => "Ctrl",
        Key::LeftAlt | Key::RightAlt => "Alt",
        Key::LeftSuper | Key::RightSuper => "Super",
        Key::Left => "<",
        Key::Right => ">",
        Key::Up => "^",
        Key::Down => "v",
        other => other.name(),
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(22, 24, 32),
        ..Config::new("Yowie inputs").with_size(960, 540)
    };
    yowie::run::<Inputs>(config)
}
