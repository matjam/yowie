//! Drawing a 2D world's debug lines through the renderer.

use yowie_core::{Color, Vec2, Vec3};
use yowie_gfx::Graphics;

use crate::{DebugDraw, World2D};

/// A [`DebugDraw`] that draws 2D debug lines and points with the
/// renderer's shapes. The z component of every position is ignored.
///
/// Draw it inside the camera scope the world is drawn in, so the debug
/// shapes line up with the sprites:
///
/// ```ignore
/// gfx.with_camera(&camera, |g| world.debug_draw_to(g));
/// ```
///
/// Line thickness is in world units. [`GfxDebugDraw::new`] picks one output
/// pixel under the current camera, so lines stay thin however far the
/// camera is zoomed.
pub struct GfxDebugDraw<'a> {
    gfx: &'a mut Graphics,
    /// Line thickness in drawing units.
    pub thickness: f32,
    /// Diameter of a drawn point in drawing units.
    pub point_size: f32,
}

impl<'a> GfxDebugDraw<'a> {
    /// A drawer with lines one output pixel thick and points four pixels
    /// across, for the camera currently set on `gfx`.
    pub fn new(gfx: &'a mut Graphics) -> Self {
        let zoom = gfx
            .camera()
            .map_or(1.0, |c| if c.zoom > 0.0 { c.zoom } else { 1.0 });
        let pixel = 1.0 / (gfx.pixels_per_unit().max(1e-3) * zoom);
        Self {
            gfx,
            thickness: pixel,
            point_size: pixel * 4.0,
        }
    }
}

fn xy(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.y)
}

impl DebugDraw for GfxDebugDraw<'_> {
    fn line(&mut self, a: Vec3, b: Vec3, color: Color) {
        self.gfx.line(xy(a), xy(b), self.thickness, color);
    }

    fn point(&mut self, p: Vec3, color: Color) {
        self.gfx.fill_circle(xy(p), self.point_size * 0.5, color);
    }
}

impl World2D {
    /// Draws the world's colliders, joints and contacts as lines through
    /// `gfx`, in whatever camera `gfx` currently has.
    pub fn debug_draw_to(&mut self, gfx: &mut Graphics) {
        let mut drawer = GfxDebugDraw::new(gfx);
        self.debug_draw(&mut drawer);
    }
}
