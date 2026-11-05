//! Translating winit's keys, buttons and modifiers into Yowie's.

use winit::event::MouseButton as WButton;
use winit::keyboard::{KeyCode, ModifiersState};
use yowie_input::{Key, Modifiers, MouseButton};

/// The Yowie key for a winit physical key code, if Yowie names it.
pub(crate) fn key(code: KeyCode) -> Option<Key> {
    use KeyCode as C;
    Some(match code {
        C::KeyA => Key::A,
        C::KeyB => Key::B,
        C::KeyC => Key::C,
        C::KeyD => Key::D,
        C::KeyE => Key::E,
        C::KeyF => Key::F,
        C::KeyG => Key::G,
        C::KeyH => Key::H,
        C::KeyI => Key::I,
        C::KeyJ => Key::J,
        C::KeyK => Key::K,
        C::KeyL => Key::L,
        C::KeyM => Key::M,
        C::KeyN => Key::N,
        C::KeyO => Key::O,
        C::KeyP => Key::P,
        C::KeyQ => Key::Q,
        C::KeyR => Key::R,
        C::KeyS => Key::S,
        C::KeyT => Key::T,
        C::KeyU => Key::U,
        C::KeyV => Key::V,
        C::KeyW => Key::W,
        C::KeyX => Key::X,
        C::KeyY => Key::Y,
        C::KeyZ => Key::Z,
        C::Digit0 => Key::Digit0,
        C::Digit1 => Key::Digit1,
        C::Digit2 => Key::Digit2,
        C::Digit3 => Key::Digit3,
        C::Digit4 => Key::Digit4,
        C::Digit5 => Key::Digit5,
        C::Digit6 => Key::Digit6,
        C::Digit7 => Key::Digit7,
        C::Digit8 => Key::Digit8,
        C::Digit9 => Key::Digit9,
        C::F1 => Key::F1,
        C::F2 => Key::F2,
        C::F3 => Key::F3,
        C::F4 => Key::F4,
        C::F5 => Key::F5,
        C::F6 => Key::F6,
        C::F7 => Key::F7,
        C::F8 => Key::F8,
        C::F9 => Key::F9,
        C::F10 => Key::F10,
        C::F11 => Key::F11,
        C::F12 => Key::F12,
        C::F13 => Key::F13,
        C::F14 => Key::F14,
        C::F15 => Key::F15,
        C::F16 => Key::F16,
        C::F17 => Key::F17,
        C::F18 => Key::F18,
        C::F19 => Key::F19,
        C::F20 => Key::F20,
        C::F21 => Key::F21,
        C::F22 => Key::F22,
        C::F23 => Key::F23,
        C::F24 => Key::F24,
        C::Space => Key::Space,
        C::Enter => Key::Enter,
        C::Escape => Key::Escape,
        C::Tab => Key::Tab,
        C::Backspace => Key::Backspace,
        C::Delete => Key::Delete,
        C::Insert => Key::Insert,
        C::Home => Key::Home,
        C::End => Key::End,
        C::PageUp => Key::PageUp,
        C::PageDown => Key::PageDown,
        C::ArrowLeft => Key::Left,
        C::ArrowRight => Key::Right,
        C::ArrowUp => Key::Up,
        C::ArrowDown => Key::Down,
        C::Minus => Key::Minus,
        C::Equal => Key::Equal,
        C::BracketLeft => Key::LeftBracket,
        C::BracketRight => Key::RightBracket,
        C::Backslash => Key::Backslash,
        C::Semicolon => Key::Semicolon,
        C::Quote => Key::Apostrophe,
        C::Backquote => Key::Grave,
        C::Comma => Key::Comma,
        C::Period => Key::Period,
        C::Slash => Key::Slash,
        C::IntlBackslash => Key::IntlBackslash,
        C::IntlRo => Key::IntlRo,
        C::IntlYen => Key::IntlYen,
        C::CapsLock => Key::CapsLock,
        C::NumLock => Key::NumLock,
        C::ScrollLock => Key::ScrollLock,
        C::PrintScreen => Key::PrintScreen,
        C::Pause => Key::Pause,
        C::ContextMenu => Key::ContextMenu,
        C::ShiftLeft => Key::LeftShift,
        C::ShiftRight => Key::RightShift,
        C::ControlLeft => Key::LeftControl,
        C::ControlRight => Key::RightControl,
        C::AltLeft => Key::LeftAlt,
        C::AltRight => Key::RightAlt,
        C::SuperLeft => Key::LeftSuper,
        C::SuperRight => Key::RightSuper,
        C::Numpad0 => Key::Keypad0,
        C::Numpad1 => Key::Keypad1,
        C::Numpad2 => Key::Keypad2,
        C::Numpad3 => Key::Keypad3,
        C::Numpad4 => Key::Keypad4,
        C::Numpad5 => Key::Keypad5,
        C::Numpad6 => Key::Keypad6,
        C::Numpad7 => Key::Keypad7,
        C::Numpad8 => Key::Keypad8,
        C::Numpad9 => Key::Keypad9,
        C::NumpadDecimal => Key::KeypadDecimal,
        C::NumpadDivide => Key::KeypadDivide,
        C::NumpadMultiply => Key::KeypadMultiply,
        C::NumpadSubtract => Key::KeypadSubtract,
        C::NumpadAdd => Key::KeypadAdd,
        C::NumpadEnter => Key::KeypadEnter,
        C::NumpadEqual => Key::KeypadEqual,
        _ => return None,
    })
}

/// Yowie's modifier set for winit's.
pub(crate) fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers::NONE
        .with(Modifiers::SHIFT, state.shift_key())
        .with(Modifiers::CONTROL, state.control_key())
        .with(Modifiers::ALT, state.alt_key())
        .with(Modifiers::SUPER, state.super_key())
}

/// The Yowie mouse button for a winit one, if Yowie names it.
pub(crate) fn mouse_button(button: WButton) -> Option<MouseButton> {
    Some(match button {
        WButton::Left => MouseButton::Left,
        WButton::Right => MouseButton::Right,
        WButton::Middle => MouseButton::Middle,
        WButton::Back => MouseButton::Back,
        WButton::Forward => MouseButton::Forward,
        WButton::Other(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations() {
        let keys = [
            (KeyCode::KeyW, Some(Key::W)),
            (KeyCode::ArrowUp, Some(Key::Up)),
            (KeyCode::NumpadEnter, Some(Key::KeypadEnter)),
            (KeyCode::Quote, Some(Key::Apostrophe)),
            (KeyCode::MediaPlayPause, None),
        ];
        for (code, want) in keys {
            assert_eq!(key(code), want, "{code:?}");
        }
        let m = modifiers(ModifiersState::SHIFT | ModifiersState::SUPER);
        assert!(m.contains(Modifiers::SHIFT | Modifiers::SUPER));
        assert!(!m.contains(Modifiers::CONTROL));
        assert_eq!(mouse_button(WButton::Other(9)), None);
    }
}
