//! Nine-slice drawing: a framed image stretched over any rectangle while its
//! corners keep their size.

use yowie_core::{Color, Rect, vec2};

use crate::Graphics;
use crate::sprite::uv_quad;
use crate::texture::Texture;

/// A texture region cut into a 3 by 3 grid by four border widths. The
/// corners draw at their own size, the edges stretch along one axis and
/// the centre stretches along both, so one small image draws a panel or a
/// speech bubble of any size.
#[derive(Clone, Debug)]
pub struct NineSlice {
    /// The image.
    pub texture: Texture,
    /// The part of the texture holding the frame, in texels; `None` uses
    /// all of it.
    pub source: Option<Rect>,
    /// Left border width in texels.
    pub left: f32,
    /// Top border height in texels.
    pub top: f32,
    /// Right border width in texels.
    pub right: f32,
    /// Bottom border height in texels.
    pub bottom: f32,
}

impl NineSlice {
    /// A nine-slice over the whole texture with the same border on every
    /// side.
    pub fn uniform(texture: Texture, border: f32) -> Self {
        Self {
            texture,
            source: None,
            left: border,
            top: border,
            right: border,
            bottom: border,
        }
    }
}

/// Splits `[start, start + len]` at the two borders, shrinking them in
/// proportion when they do not fit.
fn cuts(start: f32, len: f32, a: f32, b: f32) -> [f32; 4] {
    let scale = if a + b > len && a + b > 0.0 {
        len / (a + b)
    } else {
        1.0
    };
    [
        start,
        start + a * scale,
        start + len - b * scale,
        start + len,
    ]
}

impl Graphics {
    /// Queues a nine-slice stretched over `dest`, tinted. Borders draw at
    /// one view unit per texel and shrink together when `dest` is smaller
    /// than they are.
    pub fn draw_nine_slice(&mut self, slice: &NineSlice, dest: Rect, tint: Color) {
        let ts = slice.texture.size().as_vec2();
        let src = slice.source.unwrap_or(Rect::new(0.0, 0.0, ts.x, ts.y));
        let sx = cuts(src.x, src.w, slice.left, slice.right);
        let sy = cuts(src.y, src.h, slice.top, slice.bottom);
        let dx = cuts(dest.x, dest.w, slice.left, slice.right);
        let dy = cuts(dest.y, dest.h, slice.top, slice.bottom);
        let color = self.vertex_color(tint);
        let texture = slice.texture.clone();
        for row in 0..3 {
            for col in 0..3 {
                let d = Rect::from_min_max(vec2(dx[col], dy[row]), vec2(dx[col + 1], dy[row + 1]));
                let s = Rect::from_min_max(vec2(sx[col], sy[row]), vec2(sx[col + 1], sy[row + 1]));
                if d.is_empty() || s.is_empty() {
                    continue;
                }
                let corners = [d.min(), vec2(d.x + d.w, d.y), d.max(), vec2(d.x, d.y + d.h)];
                self.push_quad(
                    &texture,
                    corners,
                    uv_quad(&texture, s, false, false),
                    color,
                    0,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_shrink_borders_that_do_not_fit() {
        assert_eq!(cuts(0.0, 100.0, 10.0, 20.0), [0.0, 10.0, 80.0, 100.0]);
        assert_eq!(cuts(5.0, 15.0, 10.0, 20.0), [5.0, 10.0, 10.0, 20.0]);
    }
}
