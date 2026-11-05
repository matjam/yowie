//! Drawing laid-out text: glyph quads, distance-field quads and
//! decoration lines, through the sprite batch.

use yowie_core::{Affine2, Color, Rect, Vec2, vec2};

use super::TextLayout;
use super::atlas::AtlasKind;
use super::layout::GlyphRec;
use super::sdf::SDF_EM;
use crate::Graphics;
use crate::batch::SDF_SHADER;

impl Graphics {
    /// Queues a laid-out text with its top-left corner at `position`. See
    /// [`layout_text`](Self::layout_text). The transform stack, camera,
    /// blend mode, layer and clip apply as for any 2D draw.
    pub fn draw_layout(&mut self, layout: &TextLayout, position: Vec2) {
        let d = &layout.data;
        if d.glyphs.is_empty() && d.decos.is_empty() {
            return;
        }
        if d.sideways {
            // Turn the layout a quarter turn clockwise about its corner and
            // move it back into the positive quadrant.
            self.push_transform();
            self.translate(position);
            self.cursor.transform *=
                Affine2::from_cols(vec2(0.0, 1.0), vec2(-1.0, 0.0), vec2(d.size.x, 0.0));
            self.draw_layout_at(layout, Vec2::ZERO);
            self.pop_transform();
        } else {
            self.draw_layout_at(layout, position);
        }
    }

    fn draw_layout_at(&mut self, layout: &TextLayout, position: Vec2) {
        let scale = self.draw_scale();
        // Without a camera or transform, start on a whole pixel so glyphs
        // keep the sharpness they were rasterised with.
        let snap = self.cursor.camera.is_none() && self.cursor.transform == Affine2::IDENTITY;
        let origin = if snap {
            (position * scale).round()
        } else {
            position * scale
        };
        for g in &layout.data.glyphs {
            if g.glyph.w <= 0.0 && !g.hyphen {
                continue;
            }
            let color = tint(layout, g);
            if layout.sdf && self.draw_sdf_glyph(g, position, color) {
                continue;
            }
            self.draw_raster_glyph(g, origin, scale, color);
        }
        for deco in &layout.data.decos {
            let r = deco.rect;
            let color = deco
                .color
                .map_or(layout.color, |c| c.with_alpha(c.a * layout.color.a));
            let (x, y, w, h) = if snap {
                let px = |v: f32| (v * scale).round() / scale;
                (
                    px(position.x + r.x),
                    px(position.y + r.y),
                    px(r.w).max(1.0 / scale),
                    px(r.h).max(1.0 / scale),
                )
            } else {
                (position.x + r.x, position.y + r.y, r.w, r.h)
            };
            self.fill_rect(Rect::new(x, y, w, h), color);
        }
    }

    /// Queues one glyph from its distance tile. False when the glyph has
    /// no tile, so the caller draws it from the ordinary atlas.
    fn draw_sdf_glyph(&mut self, g: &GlyphRec, position: Vec2, color: Color) -> bool {
        let slot = match self.text.sdf_glyph(
            &self.shared,
            g.glyph.font_id,
            g.glyph.glyph_id,
            g.glyph.font_weight,
        ) {
            Ok(Some(s)) => s,
            Ok(None) => return false,
            Err(e) => {
                log::error!("yowie-gfx: distance tile upload failed: {e}");
                return false;
            }
        };
        let Some(atlas) = &self.text.sdf else {
            return false;
        };
        let (texture, atlas_size) = (atlas.texture.clone(), atlas.size().as_vec2());
        let size = g.glyph.font_size;
        let k = size / SDF_EM;
        let pen_x = g.left() + size * g.glyph.x_offset;
        let baseline = g.line_y + g.dy + g.glyph.y - size * g.glyph.y_offset;
        let x0 = position.x + pen_x + slot.left * k;
        let y0 = position.y + baseline - slot.top * k;
        let (w, h) = (slot.w as f32 * k, slot.h as f32 * k);
        let corners = [
            vec2(x0, y0),
            vec2(x0 + w, y0),
            vec2(x0 + w, y0 + h),
            vec2(x0, y0 + h),
        ];
        let (u0, v0) = (slot.x as f32 / atlas_size.x, slot.y as f32 / atlas_size.y);
        let (u1, v1) = (
            (slot.x + slot.w) as f32 / atlas_size.x,
            (slot.y + slot.h) as f32 / atlas_size.y,
        );
        let uvs = [vec2(u0, v0), vec2(u1, v0), vec2(u1, v1), vec2(u0, v1)];
        let c = self.vertex_color(color);
        self.push_quad_with(&texture, corners, uvs, c, 1, Some(SDF_SHADER));
        true
    }

    /// Queues one glyph rasterised at the output's pixel size.
    fn draw_raster_glyph(&mut self, g: &GlyphRec, origin: Vec2, scale: f32, color: Color) {
        let phys = g
            .glyph
            .physical((g.dx * scale, (g.line_y + g.dy) * scale), scale);
        let slot = match self.text.glyph(&self.shared, phys.cache_key) {
            Ok(Some(s)) => s,
            Ok(None) => return,
            Err(e) => {
                log::error!("yowie-gfx: glyph upload failed: {e}");
                return;
            }
        };
        if slot.w == 0 || slot.h == 0 {
            return;
        }
        let (texture, atlas_size, tint, mode) = match slot.kind {
            AtlasKind::Color => match &self.text.color {
                Some(a) => (
                    a.texture.clone(),
                    a.size(),
                    self.vertex_color(Color::WHITE.with_alpha(color.a)),
                    0,
                ),
                None => return,
            },
            _ => (
                self.text.mask.texture.clone(),
                self.text.mask.size(),
                self.vertex_color(color),
                1,
            ),
        };
        let x0 = origin.x + (phys.x + slot.left) as f32;
        let y0 = origin.y + (phys.y - slot.top) as f32;
        let (w, h) = (slot.w as f32, slot.h as f32);
        let corners = [
            vec2(x0, y0),
            vec2(x0 + w, y0),
            vec2(x0 + w, y0 + h),
            vec2(x0, y0 + h),
        ]
        .map(|p| p / scale);
        let a = atlas_size.as_vec2();
        let (u0, v0) = (slot.x as f32 / a.x, slot.y as f32 / a.y);
        let (u1, v1) = (
            (slot.x + slot.w) as f32 / a.x,
            (slot.y + slot.h) as f32 / a.y,
        );
        let uvs = [vec2(u0, v0), vec2(u1, v0), vec2(u1, v1), vec2(u0, v1)];
        self.push_quad(&texture, corners, uvs, tint, mode);
    }
}

/// The colour a glyph draws in: its span's, or the layout's.
fn tint(layout: &TextLayout, g: &GlyphRec) -> Color {
    match g.color {
        Some(c) => c.with_alpha(c.a * layout.color.a),
        None => layout.color,
    }
}
