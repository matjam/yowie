//! Debug drawing of a 3D world.

use rapier3d::math::Vector;
use rapier3d::pipeline::{DebugRenderBackend, DebugRenderObject};

use super::convert::unvec;
use super::world::World3D;
use crate::debug::{DebugDraw, hsla_to_color};

struct Adapter<'a>(&'a mut dyn DebugDraw);

impl DebugRenderBackend for Adapter<'_> {
    fn draw_line(&mut self, _object: DebugRenderObject, a: Vector, b: Vector, color: [f32; 4]) {
        self.0.line(unvec(a), unvec(b), hsla_to_color(color));
    }
}

impl World3D {
    /// Sends the outline of every collider, the joints and the contact
    /// points to `draw`. Sleeping bodies are drawn darker.
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
