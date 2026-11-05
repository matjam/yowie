//! Debug drawing through a trait the renderer implements.

use yowie_core::{Color, Vec3};

/// Receives the lines and points a world wants drawn for debugging.
///
/// 2D worlds report positions with `z = 0`. Colours are sRGB. The physics
/// crate does not depend on the renderer; `yowie-gfx` implements this trait.
pub trait DebugDraw {
    /// Draws a line segment from `a` to `b`.
    fn line(&mut self, a: Vec3, b: Vec3, color: Color);

    /// Draws a point at `p`.
    fn point(&mut self, p: Vec3, color: Color);
}

/// Converts a rapier debug colour (hue in degrees, saturation, lightness,
/// alpha) to sRGB.
pub(crate) fn hsla_to_color(hsla: [f32; 4]) -> Color {
    let h = hsla[0].rem_euclid(360.0);
    let s = hsla[1].clamp(0.0, 1.0);
    let l = hsla[2].clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::rgba(r + m, g + m, b + m, hsla[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsla_primaries() {
        let cases = [
            ([0.0, 1.0, 0.5, 1.0], Color::RED),
            ([120.0, 1.0, 0.5, 1.0], Color::GREEN),
            ([240.0, 1.0, 0.5, 1.0], Color::BLUE),
            ([0.0, 0.0, 1.0, 1.0], Color::WHITE),
        ];
        for (hsla, want) in cases {
            let got = hsla_to_color(hsla);
            for (a, b) in got.to_array().iter().zip(want.to_array()) {
                assert!((a - b).abs() < 1e-4, "{hsla:?} -> {got:?}");
            }
        }
    }
}
