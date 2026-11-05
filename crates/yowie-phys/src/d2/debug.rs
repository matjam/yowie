//! Debug drawing of a 2D world.

use rapier2d::math::Vector;
use rapier2d::pipeline::{DebugRenderBackend, DebugRenderObject};
use yowie_core::Vec3;

use super::world::World2D;
use crate::debug::{DebugDraw, hsla_to_color};

struct Adapter<'a>(&'a mut dyn DebugDraw);

impl DebugRenderBackend for Adapter<'_> {
    fn draw_line(&mut self, _object: DebugRenderObject, a: Vector, b: Vector, color: [f32; 4]) {
        self.0.line(
            Vec3::new(a.x, a.y, 0.0),
            Vec3::new(b.x, b.y, 0.0),
            hsla_to_color(color),
        );
    }
}

impl World2D {
    /// Sends the outline of every collider, the joints and the contact
    /// points to `draw`, with `z = 0`. Sleeping bodies are drawn darker.
    pub fn debug_draw(&mut self, draw: &mut dyn DebugDraw) {
        let mut backend = Adapter(draw);
        let pw = &self.pw;
        self.debug.render(
            &mut backend,
            &pw.bodies,
            &pw.colliders,
            &pw.impulse_joints,
            &pw.multibody_joints,
            &pw.narrow_phase,
            &pw.soft_bodies,
        );
    }
}
