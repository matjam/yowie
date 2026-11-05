//! The drawing surface widgets paint on.
//!
//! [`Painter`] is the small set of operations the toolkit needs. The
//! engine's renderer implements it through [`GfxPainter`], which draws
//! through the 2D batch on [`layers::UI`](yowie_gfx::layers::UI); tests
//! implement it with a recorder so layout and interaction run without a
//! GPU.

use yowie_core::{Color, Rect, Vec2};
use yowie_gfx::{BlendMode, Camera2D, Font, Graphics, NineSlice, Sprite, TextStyle, Texture};

/// Which of the two bundled font roles text uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Face {
    /// The proportional interface font.
    #[default]
    Regular,
    /// The monospaced font, for the console and code.
    Mono,
}

/// What the toolkit draws with. Coordinates are view units with +Y down.
///
/// Draws are tagged with a depth through [`set_depth`](Painter::set_depth):
/// larger depths draw over smaller ones, and draws with equal depth keep
/// call order. The toolkit uses depth to group fills, images and text so
/// that a window's widgets batch into few draw calls.
pub trait Painter {
    /// The size of `text` set in `face` at `size`, wrapped to `wrap` when
    /// given.
    fn measure(&mut self, text: &str, face: Face, size: f32, wrap: Option<f32>) -> Vec2;
    /// Draws `text` with its top-left corner at `pos`.
    fn text(
        &mut self,
        text: &str,
        pos: Vec2,
        face: Face,
        size: f32,
        color: Color,
        wrap: Option<f32>,
    );
    /// Fills a rectangle.
    fn fill(&mut self, rect: Rect, color: Color);
    /// Outlines a rectangle inside its bounds.
    fn stroke(&mut self, rect: Rect, thickness: f32, color: Color);
    /// Draws a line segment.
    fn line(&mut self, a: Vec2, b: Vec2, thickness: f32, color: Color);
    /// Fills a convex polygon.
    fn polygon(&mut self, points: &[Vec2], color: Color);
    /// Draws a nine-slice over `dest`.
    fn nine_slice(&mut self, slice: &NineSlice, dest: Rect, tint: Color);
    /// Draws a texture (or the `source` texel rectangle of it) over `dest`.
    fn image(&mut self, texture: &Texture, dest: Rect, source: Option<Rect>, tint: Color);
    /// Limits later drawing to a rectangle, or removes the limit.
    fn set_clip(&mut self, clip: Option<Rect>);
    /// Sets the depth of later draws.
    fn set_depth(&mut self, depth: f32);
    /// The size of the drawing area in view units.
    fn view_size(&self) -> Vec2;
}

/// A [`Painter`] drawing through a [`Graphics`] on one layer.
///
/// Creating it resets the camera, clip, shader and blend mode to what
/// interface drawing needs; dropping it restores the layer, sort key, clip
/// and camera that were set before.
pub struct GfxPainter<'g> {
    gfx: &'g mut Graphics,
    regular: Font,
    mono: Font,
    saved: (i32, f32, Option<Rect>, Option<Camera2D>, BlendMode),
}

impl<'g> GfxPainter<'g> {
    /// Paints on `layer` with the bundled fonts.
    pub fn new(gfx: &'g mut Graphics, layer: i32) -> Self {
        let regular = gfx.default_font();
        let mono = gfx.default_mono_font();
        Self::with_fonts(gfx, layer, regular, mono)
    }

    /// Paints on `layer` with the given proportional and monospaced fonts.
    pub fn with_fonts(gfx: &'g mut Graphics, layer: i32, regular: Font, mono: Font) -> Self {
        let saved = (gfx.layer(), gfx.sort_key(), None, gfx.camera(), gfx.blend());
        gfx.set_camera(None);
        gfx.set_clip(None);
        gfx.set_shader(None);
        gfx.set_blend(BlendMode::Alpha);
        gfx.set_layer(layer);
        Self {
            gfx,
            regular,
            mono,
            saved,
        }
    }

    fn font(&self, face: Face) -> &Font {
        match face {
            Face::Regular => &self.regular,
            Face::Mono => &self.mono,
        }
    }
}

impl Drop for GfxPainter<'_> {
    fn drop(&mut self) {
        let (layer, key, clip, camera, blend) = self.saved;
        self.gfx.set_layer(layer);
        self.gfx.set_sort_key(key);
        self.gfx.set_clip(clip);
        self.gfx.set_camera(camera);
        self.gfx.set_blend(blend);
    }
}

fn style(size: f32, color: Color, wrap: Option<f32>) -> TextStyle {
    let s = TextStyle::new(size).color(color);
    match wrap {
        Some(w) => s.width(w),
        None => s,
    }
}

impl Painter for GfxPainter<'_> {
    fn measure(&mut self, text: &str, face: Face, size: f32, wrap: Option<f32>) -> Vec2 {
        let font = self.font(face).clone();
        self.gfx
            .measure_text(&font, text, &style(size, Color::WHITE, wrap))
    }

    fn text(
        &mut self,
        text: &str,
        pos: Vec2,
        face: Face,
        size: f32,
        color: Color,
        wrap: Option<f32>,
    ) {
        let font = self.font(face).clone();
        self.gfx
            .draw_text(&font, text, pos, &style(size, color, wrap));
    }

    fn fill(&mut self, rect: Rect, color: Color) {
        self.gfx.fill_rect(rect, color);
    }

    fn stroke(&mut self, rect: Rect, thickness: f32, color: Color) {
        self.gfx.stroke_rect(rect, thickness, color);
    }

    fn line(&mut self, a: Vec2, b: Vec2, thickness: f32, color: Color) {
        self.gfx.line(a, b, thickness, color);
    }

    fn polygon(&mut self, points: &[Vec2], color: Color) {
        self.gfx.fill_polygon(points, color);
    }

    fn nine_slice(&mut self, slice: &NineSlice, dest: Rect, tint: Color) {
        self.gfx.draw_nine_slice(slice, dest, tint);
    }

    fn image(&mut self, texture: &Texture, dest: Rect, source: Option<Rect>, tint: Color) {
        let mut sprite = Sprite::at(dest.min()).size(dest.size()).tint(tint);
        if let Some(s) = source {
            sprite = sprite.source(s);
        }
        self.gfx.draw_sprite(texture, &sprite);
    }

    fn set_clip(&mut self, clip: Option<Rect>) {
        self.gfx.set_clip(clip);
    }

    fn set_depth(&mut self, depth: f32) {
        self.gfx.set_sort_key(depth);
    }

    fn view_size(&self) -> Vec2 {
        self.gfx.view_size()
    }
}
