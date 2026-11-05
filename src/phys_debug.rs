//! Drawing physics debug geometry into a 3D scene.

use yowie_core::{Color, Vec3};
use yowie_gfx::scene::Scene;
use yowie_phys::DebugDraw;

/// Passes `yowie_phys` debug drawing to a scene's lines: hand it to
/// `World3D::debug_draw` inside [`Graphics::scene`](yowie_gfx::Graphics::scene).
///
/// ```no_run
/// # fn f(world: &mut yowie::phys::World3D, s: &mut yowie::gfx::scene::Scene) {
/// world.debug_draw(&mut yowie::SceneDebugDraw::new(s));
/// # }
/// ```
pub struct SceneDebugDraw<'s, 'a> {
    scene: &'s mut Scene<'a>,
    /// Draws over everything instead of hiding behind geometry.
    pub on_top: bool,
    /// Half the size of the cross drawn for a point, in world units.
    pub point_size: f32,
}

impl<'s, 'a> SceneDebugDraw<'s, 'a> {
    /// An adapter drawing into `scene`, hidden by geometry in front.
    pub fn new(scene: &'s mut Scene<'a>) -> Self {
        Self {
            scene,
            on_top: false,
            point_size: 0.05,
        }
    }
}

impl DebugDraw for SceneDebugDraw<'_, '_> {
    fn line(&mut self, a: Vec3, b: Vec3, color: Color) {
        if self.on_top {
            self.scene.line_on_top(a, b, color);
        } else {
            self.scene.line(a, b, color);
        }
    }

    fn point(&mut self, p: Vec3, color: Color) {
        let s = self.point_size;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.line(p - axis * s, p + axis * s, color);
        }
    }
}
