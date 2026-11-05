//! Sprites: textured quads placed, turned, scaled, tinted and flipped.

use yowie_core::{Color, Rect, Vec2, vec2};

use crate::Graphics;
use crate::texture::Texture;

/// One textured quad. `Sprite::at(pos)` and the builder methods cover the
/// common cases:
///
/// ```ignore
/// gfx.draw_sprite(&tex, &Sprite::at(pos).centered().rotation(angle).tint(Color::RED));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// Where the origin point lands, in drawing coordinates.
    pub position: Vec2,
    /// The drawn size before scaling; `None` uses the source rectangle's
    /// size in texels.
    pub size: Option<Vec2>,
    /// Scale applied about the origin.
    pub scale: Vec2,
    /// Clockwise turn about the origin, in radians.
    pub rotation: f32,
    /// The pivot as a fraction of the size: `(0, 0)` is the top-left,
    /// `(0.5, 0.5)` the centre.
    pub origin: Vec2,
    /// The part of the texture to draw, in texels; `None` draws all of it.
    pub source: Option<Rect>,
    /// Multiplies the texture's colour, sRGB with straight alpha.
    pub tint: Color,
    /// Mirrors the image left to right.
    pub flip_x: bool,
    /// Mirrors the image top to bottom.
    pub flip_y: bool,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            size: None,
            scale: Vec2::ONE,
            rotation: 0.0,
            origin: Vec2::ZERO,
            source: None,
            tint: Color::WHITE,
            flip_x: false,
            flip_y: false,
        }
    }
}

impl Sprite {
    /// A sprite with its top-left corner at `position`.
    pub fn at(position: Vec2) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }

    /// Sets the drawn size before scaling.
    pub fn size(self, size: Vec2) -> Self {
        Self {
            size: Some(size),
            ..self
        }
    }

    /// Sets the scale.
    pub fn scale(self, scale: Vec2) -> Self {
        Self { scale, ..self }
    }

    /// Sets a uniform scale.
    pub fn scale_by(self, scale: f32) -> Self {
        Self {
            scale: Vec2::splat(scale),
            ..self
        }
    }

    /// Sets the clockwise rotation in radians.
    pub fn rotation(self, rotation: f32) -> Self {
        Self { rotation, ..self }
    }

    /// Sets the pivot as a fraction of the size.
    pub fn origin(self, origin: Vec2) -> Self {
        Self { origin, ..self }
    }

    /// Puts the pivot at the centre, so `position` is the sprite's middle.
    pub fn centered(self) -> Self {
        self.origin(vec2(0.5, 0.5))
    }

    /// Sets the source rectangle in texels.
    pub fn source(self, source: Rect) -> Self {
        Self {
            source: Some(source),
            ..self
        }
    }

    /// Sets the tint.
    pub fn tint(self, tint: Color) -> Self {
        Self { tint, ..self }
    }

    /// Mirrors left to right.
    pub fn flip_x(self, flip: bool) -> Self {
        Self {
            flip_x: flip,
            ..self
        }
    }

    /// Mirrors top to bottom.
    pub fn flip_y(self, flip: bool) -> Self {
        Self {
            flip_y: flip,
            ..self
        }
    }

    /// The four placed corners, clockwise from the top-left of the image,
    /// for a texture of `texture_size` texels.
    pub fn corners(&self, texture_size: Vec2) -> [Vec2; 4] {
        let src = self
            .source
            .unwrap_or(Rect::new(0.0, 0.0, texture_size.x, texture_size.y));
        let size = self.size.unwrap_or(src.size());
        let rot = Vec2::from_angle(self.rotation);
        let local = [
            vec2(0.0, 0.0),
            vec2(1.0, 0.0),
            vec2(1.0, 1.0),
            vec2(0.0, 1.0),
        ];
        local.map(|c| self.position + rot.rotate((c - self.origin) * size * self.scale))
    }

    /// The axis-aligned rectangle enclosing the placed sprite.
    pub fn bounds(&self, texture_size: Vec2) -> Rect {
        Rect::bounding(self.corners(texture_size)).unwrap_or(Rect::ZERO)
    }
}

/// Texture coordinates for a texel rectangle, clockwise from the top-left,
/// with optional mirroring.
pub(crate) fn uv_quad(texture: &Texture, src: Rect, flip_x: bool, flip_y: bool) -> [Vec2; 4] {
    let ts = texture.size().as_vec2();
    let (mut u0, mut v0) = (src.x / ts.x, src.y / ts.y);
    let (mut u1, mut v1) = ((src.x + src.w) / ts.x, (src.y + src.h) / ts.y);
    if flip_x {
        std::mem::swap(&mut u0, &mut u1);
    }
    if flip_y {
        std::mem::swap(&mut v0, &mut v1);
    }
    [vec2(u0, v0), vec2(u1, v0), vec2(u1, v1), vec2(u0, v1)]
}

impl Graphics {
    /// Queues a sprite. Sprites wholly outside the visible area are dropped.
    pub fn draw_sprite(&mut self, texture: &Texture, sprite: &Sprite) {
        let ts = texture.size().as_vec2();
        let corners = sprite.corners(ts);
        if let Some(bounds) = Rect::bounding(corners)
            && !bounds.intersects(&self.cached_visible_area())
        {
            return;
        }
        let src = sprite.source.unwrap_or(Rect::new(0.0, 0.0, ts.x, ts.y));
        let uvs = uv_quad(texture, src, sprite.flip_x, sprite.flip_y);
        let color = self.vertex_color(sprite.tint);
        self.push_quad(texture, corners, uvs, color, 0);
    }

    /// Queues a whole texture at its own size with its top-left corner at
    /// `position`.
    pub fn draw_texture(&mut self, texture: &Texture, position: Vec2) {
        self.draw_sprite(texture, &Sprite::at(position));
    }

    /// Queues a whole texture stretched over `dest`, tinted.
    pub fn draw_texture_rect(&mut self, texture: &Texture, dest: Rect, tint: Color) {
        self.draw_sprite(
            texture,
            &Sprite::at(dest.min()).size(dest.size()).tint(tint),
        );
    }

    /// The part of drawing space that is on screen: the camera's visible
    /// rectangle, or the whole view without a camera.
    pub fn visible_area(&self) -> Rect {
        let view = self.view_size();
        match &self.cursor.camera {
            Some(c) => c.visible_rect(view),
            None => Rect::new(0.0, 0.0, view.x, view.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_follow_origin_rotation_and_scale() {
        let ts = vec2(10.0, 20.0);
        let cases = [
            (
                Sprite::at(vec2(5.0, 5.0)),
                [
                    vec2(5.0, 5.0),
                    vec2(15.0, 5.0),
                    vec2(15.0, 25.0),
                    vec2(5.0, 25.0),
                ],
            ),
            (
                Sprite::at(vec2(0.0, 0.0)).centered().scale_by(2.0),
                [
                    vec2(-10.0, -20.0),
                    vec2(10.0, -20.0),
                    vec2(10.0, 20.0),
                    vec2(-10.0, 20.0),
                ],
            ),
            (
                Sprite::at(vec2(0.0, 0.0))
                    .rotation(std::f32::consts::FRAC_PI_2)
                    .size(vec2(4.0, 2.0)),
                [
                    vec2(0.0, 0.0),
                    vec2(0.0, 4.0),
                    vec2(-2.0, 4.0),
                    vec2(-2.0, 0.0),
                ],
            ),
        ];
        for (sprite, want) in cases {
            let got = sprite.corners(ts);
            for (g, w) in got.iter().zip(want) {
                assert!((*g - w).length() < 1e-4, "{sprite:?}: {got:?}");
            }
        }
    }
}
