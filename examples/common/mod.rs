//! Small drawing helpers the examples share: panels, labels, bars and
//! buttons. Real games use `yowie-ui`; these keep the examples standalone.

#![allow(dead_code)]

use yowie::gfx::{Font, TextStyle};
use yowie::input::MouseButton;
use yowie::{Color, Context, Rect, Vec2, vec2};

pub const PANEL: Color = Color::rgba(0.07, 0.08, 0.11, 0.92);
pub const EDGE: Color = Color::rgb(0.25, 0.28, 0.36);
pub const TEXT: Color = Color::rgb(0.82, 0.85, 0.92);
pub const DIM: Color = Color::rgb(0.5, 0.54, 0.62);
pub const ACCENT: Color = Color::rgb(0.35, 0.65, 1.0);

/// Draws a filled, outlined panel.
pub fn panel(ctx: &mut Context, rect: Rect) {
    ctx.gfx.fill_rect(rect, PANEL);
    ctx.gfx.stroke_rect(rect, 1.0, EDGE);
}

/// Draws one line of text with its top-left corner at `at`.
pub fn label(ctx: &mut Context, font: &Font, text: &str, at: Vec2, size: f32, color: Color) {
    ctx.gfx
        .draw_text(font, text, at, &TextStyle::new(size).color(color));
}

/// Draws a horizontal bar filled to `fraction` (0 to 1).
pub fn bar(ctx: &mut Context, rect: Rect, fraction: f32, color: Color) {
    ctx.gfx.fill_rect(rect, Color::rgb(0.14, 0.15, 0.2));
    let w = rect.w * fraction.clamp(0.0, 1.0);
    ctx.gfx
        .fill_rect(Rect::new(rect.x, rect.y, w, rect.h), color);
    ctx.gfx.stroke_rect(rect, 1.0, EDGE);
}

/// Draws a button and returns true on the frame the left mouse button is
/// pressed over it.
pub fn button(ctx: &mut Context, font: &Font, rect: Rect, text: &str) -> bool {
    let over = rect.contains(ctx.input.mouse());
    let fill = if over {
        Color::rgb(0.2, 0.26, 0.38)
    } else {
        Color::rgb(0.13, 0.16, 0.24)
    };
    ctx.gfx.fill_rect(rect, fill);
    ctx.gfx.stroke_rect(rect, 1.0, EDGE);
    label(
        ctx,
        font,
        text,
        vec2(rect.x + 8.0, rect.y + (rect.h - 15.0) * 0.5),
        15.0,
        TEXT,
    );
    over && ctx.input.mouse_pressed(MouseButton::Left)
}
