//! Drawing particles through the renderer's sprite batch.

use yowie_core::{Rect, Vec2, vec2};
use yowie_gfx::{BlendMode, Graphics, Vertex};

use crate::System;

impl System {
    /// Queues every live particle as one triangle list on the system's
    /// texture, so the whole system is one draw call. The emitter's blend
    /// mode and layer apply for the call and are restored afterwards.
    /// Particles outside the visible area are skipped.
    pub fn draw(&self, gfx: &mut Graphics) {
        if self.live.is_empty() {
            return;
        }
        let e = self.emitter();
        let offset = if e.world_space {
            Vec2::ZERO
        } else {
            self.position()
        };
        let aspect = self.aspect();
        let visible = gfx.visible_area();
        let mut vertices = Vec::with_capacity(self.live.len() * 4);
        let mut indices = Vec::with_capacity(self.live.len() * 6);
        let frame_count = match (&self.sheet, e.frames.len()) {
            (Some(s), 0) => s.count() as usize,
            (Some(_), n) => n,
            _ => 0,
        };
        for p in &self.live {
            let t = p.t();
            let width = p.size * e.size_over_life.at(t);
            let mut color = e.color_at(t) * p.tint;
            color.a *= e.alpha_over_life.at(t);
            if width <= 0.0 || color.a <= 0.0 {
                continue;
            }
            let centre = p.position + offset;
            let half = vec2(width, width * aspect) * 0.5;
            let reach = half.length();
            if !visible.intersects(&Rect::from_center(centre, Vec2::splat(reach * 2.0))) {
                continue;
            }
            let rot = Vec2::from_angle(p.rotation);
            let uv = match &self.sheet {
                Some(sheet) if frame_count > 0 => {
                    let u = if e.frame_over_life.is_empty() {
                        t
                    } else {
                        e.frame_over_life.at(t)
                    };
                    let slot = ((u * frame_count as f32) as usize).min(frame_count - 1);
                    let frame = e.frames.get(slot).map_or(slot as u32, |f| *f);
                    sheet.frame_uv(frame).unwrap_or((Vec2::ZERO, Vec2::ONE))
                }
                _ => (Vec2::ZERO, Vec2::ONE),
            };
            let corners = [
                (vec2(-half.x, -half.y), vec2(uv.0.x, uv.0.y)),
                (vec2(half.x, -half.y), vec2(uv.1.x, uv.0.y)),
                (vec2(half.x, half.y), vec2(uv.1.x, uv.1.y)),
                (vec2(-half.x, half.y), vec2(uv.0.x, uv.1.y)),
            ];
            let base = vertices.len() as u32;
            for (c, uv) in corners {
                vertices.push(Vertex::new(centre + rot.rotate(c), uv, color));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        if indices.is_empty() {
            return;
        }
        let texture = self
            .sheet
            .as_ref()
            .map(|s| s.texture())
            .or(self.texture.as_ref());
        let blend: BlendMode = e.blend.into();
        gfx.with_layer(e.layer, |g| {
            g.with_blend(blend, |g| g.draw_triangles(texture, &vertices, &indices));
        });
    }
}
