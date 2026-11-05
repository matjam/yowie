//! Drawing the fluid tank over the scene with the 2D renderer.

use yowie::gfx::Graphics;
use yowie::phys::soft::Fluid2D;
use yowie::{Color, vec2};

use crate::scene::{POST_RADIUS, TANK, post_centre};

/// Draws the tank, one disc per fluid particle and the post. Denser liquid
/// is deeper blue; the spray at the surface is paler.
pub fn draw(g: &mut Graphics, fluid: &Fluid2D) {
    g.fill_rect(TANK, Color::rgba(0.03, 0.05, 0.09, 0.85));
    let radius = fluid.spacing() * 0.75;
    for (i, p) in fluid.positions().iter().enumerate() {
        let d = (fluid.density(i) / fluid.rest_density()).clamp(0.0, 1.0);
        let color = Color::rgba(0.35 - 0.25 * d, 0.6 - 0.2 * d, 1.0, 0.9);
        g.fill_circle(vec2(p.x, p.y), radius, color);
    }
    g.fill_circle(post_centre(), POST_RADIUS, Color::rgb(0.5, 0.5, 0.55));
    g.stroke_circle(post_centre(), POST_RADIUS, 2.0, Color::rgb(0.8, 0.8, 0.85));
    g.stroke_rect(TANK, 2.0, Color::rgb(0.45, 0.5, 0.62));
}
