//! 2D lights with shadows: lit sprites, point and spot lights, and
//! occluders.
//!
//! The game describes the lighting once per frame during `draw`:
//! [`Graphics::set_lights_2d`] gives the ambient colour and the lights,
//! [`Graphics::add_occluder`] adds shapes that block them, and
//! [`Graphics::draw_lit`] draws sprites that the lights shade. Lit and
//! unlit sprites mix freely in one frame and sort by layer and sort key
//! together.
//!
//! ```no_run
//! # use yowie_gfx::{Graphics, Light2D, Sprite, Texture};
//! # use yowie_core::{Color, Rect, vec2};
//! # fn draw(gfx: &mut Graphics, floor: &Texture, floor_normal: &Texture) {
//! gfx.set_lights_2d(
//!     Color::rgb8(30, 30, 45),
//!     &[Light2D::point(vec2(200.0, 150.0), 220.0, Color::rgb8(255, 200, 120)).shadows(true)],
//! );
//! gfx.add_occluder_rect(Rect::new(260.0, 140.0, 40.0, 40.0));
//! gfx.draw_lit(floor, Some(floor_normal), &Sprite::at(vec2(0.0, 0.0)));
//! # }
//! ```
//!
//! # How shadows work
//!
//! Each shadow-casting light gets a polar shadow map built on the CPU: 512
//! directions around the light, each holding the distance to the nearest
//! occluder edge (`shadow.rs`). The lit shader reads the map along the
//! direction from the light to the pixel and darkens the pixel beyond the
//! stored distance, averaging five taps across the light's softness for a
//! soft edge. A frame whose shadowed lights and occluders equal the
//! previous frame's reuses the maps already on the GPU.
//!
//! Lights, occluders and lit sprites are all in drawing coordinates, the
//! same space as sprite positions (world units under a 2D camera). The
//! transform stack moves a lit sprite's vertices but not lights or
//! occluders.

mod render;
mod shadow;
mod types;

pub use types::{Light2D, MAX_LIGHTS_2D, Spot2D};

pub(crate) use render::LitRenderer;

use shadow::Occluders;
use yowie_core::{Color, Rect, Vec2};

use crate::Graphics;
use crate::batch::{DrawState, LIT_SHADER};
use crate::blend::BlendMode;
use crate::pipelines::Vertex2D;
use crate::sprite::{Sprite, uv_quad};
use crate::texture::{Texture, TextureOptions};

/// A stretch of lit sprites sharing textures, layer, sort key and view.
#[derive(Debug)]
pub(crate) struct LitRun {
    pub(crate) albedo: Texture,
    pub(crate) normal: Texture,
    pub(crate) first: u32,
    pub(crate) count: u32,
    list: usize,
    layer: i32,
    key: f32,
    view: u32,
}

/// The frame's lighting description and lit geometry.
#[derive(Debug)]
pub(crate) struct LitState {
    pub(crate) ambient: Color,
    pub(crate) lights: Vec<Light2D>,
    pub(crate) occluders: Occluders,
    pub(crate) runs: Vec<LitRun>,
    pub(crate) vertices: Vec<Vertex2D>,
    pub(crate) indices: Vec<u32>,
    flat_normal: Option<Texture>,
    pub(crate) renderer: Option<LitRenderer>,
}

impl Default for LitState {
    fn default() -> Self {
        Self {
            ambient: Color::WHITE,
            lights: Vec::new(),
            occluders: Occluders::new(),
            runs: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            flat_normal: None,
            renderer: None,
        }
    }
}

impl LitState {
    pub(crate) fn begin_frame(&mut self) {
        self.ambient = Color::WHITE;
        self.lights.clear();
        self.occluders.clear();
        self.runs.clear();
        self.vertices.clear();
        self.indices.clear();
    }
}

impl Graphics {
    /// Sets the lighting for the frame's lit sprites: the ambient colour
    /// (sRGB) that reaches everything, and up to [`MAX_LIGHTS_2D`] lights.
    /// Lights past the limit are dropped, so pass the nearest first. The
    /// lighting resets every frame, and every lit sprite in the frame sees
    /// the same set whatever the order of the calls.
    pub fn set_lights_2d(&mut self, ambient: Color, lights: &[Light2D]) {
        self.lit.ambient = ambient;
        self.lit.lights.clear();
        let n = lights.len().min(MAX_LIGHTS_2D);
        if n < lights.len() {
            log::debug!("{} lights past the limit of {MAX_LIGHTS_2D}", lights.len() - n);
        }
        self.lit.lights.extend_from_slice(&lights[..n]);
    }

    /// Adds a shadow caster for this frame: a closed polygon of at least
    /// three points, or a single wall of two. Lights with
    /// [`shadows`](Light2D::shadows) are blocked by it. Points are in
    /// drawing coordinates. Occluders reset every frame. The cost is the
    /// edges near each shadowed light times the shadowed lights, so add the
    /// walls near the action rather than a whole level.
    pub fn add_occluder(&mut self, polygon: &[Vec2]) {
        self.lit.occluders.add(polygon);
    }

    /// Adds a rectangular shadow caster; see
    /// [`add_occluder`](Self::add_occluder).
    pub fn add_occluder_rect(&mut self, rect: Rect) {
        let (a, b) = (rect.min(), rect.min() + rect.size());
        self.add_occluder(&[a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]);
    }

    /// Adds the placed outline of `sprite` drawn with `texture` as a
    /// shadow caster, so a crate sprite casts the shadow of its box.
    pub fn add_occluder_sprite(&mut self, texture: &Texture, sprite: &Sprite) {
        let corners = sprite.corners(texture.size().as_vec2());
        self.add_occluder(&corners);
    }

    /// Queues a sprite shaded by the frame's lights. `normal` is a
    /// tangent-space normal map made with [`TextureOptions::data`] set and
    /// drawn over the same rectangle; `None` lights the sprite as a flat
    /// surface facing the viewer. Normals are not rotated with the
    /// sprite. Lit sprites blend as ordinary transparency whatever the
    /// current blend mode, and ignore the current shader.
    pub fn draw_lit(&mut self, texture: &Texture, normal: Option<&Texture>, sprite: &Sprite) {
        let ts = texture.size().as_vec2();
        let corners = sprite.corners(ts);
        let t = self.cursor.transform;
        let placed = corners.map(|c| t.transform_point2(c));
        if let Some(bounds) = Rect::bounding(placed)
            && !bounds.intersects(&self.cached_visible_area())
        {
            return;
        }
        let normal = match normal {
            Some(n) => n.clone(),
            None => self.flat_normal(),
        };
        let src = sprite.source.unwrap_or(Rect::new(0.0, 0.0, ts.x, ts.y));
        let uvs = uv_quad(texture, src, sprite.flip_x, sprite.flip_y);
        let color = sprite.tint.to_linear_premultiplied();
        let view = self.view_slot();
        let sc = self.tables.views[view as usize].scissor;
        if sc[2] == 0 || sc[3] == 0 {
            return;
        }
        let list = self.current_index();
        let (layer, key) = (self.cursor.layer, self.cursor.sort_key);
        let lit = &mut self.lit;
        let base = lit.vertices.len() as u32;
        for i in 0..4 {
            lit.vertices.push(Vertex2D {
                position: placed[i].to_array(),
                uv: uvs[i].to_array(),
                color,
                mode: 0,
            });
        }
        let first = lit.indices.len() as u32;
        lit.indices
            .extend([0, 1, 2, 0, 2, 3].map(|i| i + base));
        let extends = lit.runs.last().is_some_and(|r| {
            r.list == list
                && r.layer == layer
                && r.key.to_bits() == key.to_bits()
                && r.view == view
                && r.albedo.id() == texture.id()
                && r.normal.id() == normal.id()
        }) && self.lists[list].items.last().is_some_and(|item| {
            item.state.shader == LIT_SHADER
                && item.state.texture as usize == self.lit.runs.len() - 1
        });
        if extends {
            if let Some(r) = self.lit.runs.last_mut() {
                r.count += 6;
            }
            return;
        }
        let slot = self.lit.runs.len() as u32;
        self.lit.runs.push(LitRun {
            albedo: texture.clone(),
            normal,
            first,
            count: 6,
            list,
            layer,
            key,
            view,
        });
        let state = DrawState {
            texture: slot,
            shader: LIT_SHADER,
            blend: BlendMode::Alpha,
            view,
        };
        self.lists[list].push_marker(state, layer, key);
    }

    /// The 1 by 1 normal map that points straight at the viewer.
    fn flat_normal(&mut self) -> Texture {
        if let Some(t) = &self.lit.flat_normal {
            return t.clone();
        }
        let options = TextureOptions {
            data: true,
            ..TextureOptions::default()
        };
        // A 1 by 1 texture of valid size always creates; if the device is
        // lost the white texture stands in and lights the sprite flat.
        let t = Texture::from_pixels(self, 1, 1, &[128, 128, 255, 255], options)
            .unwrap_or_else(|_| self.white.clone());
        self.lit.flat_normal = Some(t.clone());
        t
    }
}
