//! Drawing paths: tessellate, feather, paint and queue through the batch.

use std::collections::HashMap;

use yowie_core::color::linear_to_srgb;
use yowie_core::{Color, Vec2, vec2};

use super::build::Path;
use super::mesh::{self, Mesh};
use super::style::{Fill, Gradient, Paint, Stroke};
use crate::Graphics;
use crate::batch::RADIAL_SHADER;
use crate::pipelines::Vertex2D;
use crate::texture::{Filter, Texture, TextureOptions};

/// Colour ramps already built for gradients, by content.
#[derive(Debug, Default)]
pub(crate) struct PathCache {
    ramps: HashMap<u64, Texture>,
}

/// Texels in a gradient's colour ramp.
const RAMP_SIZE: u32 = 256;
/// Maps a position to a texture coordinate.
type UvFn = Box<dyn Fn(Vec2) -> Vec2>;
/// Most ramps kept before the cache starts over.
const MAX_RAMPS: usize = 64;
/// Width of the anti-aliasing fringe in output pixels.
const FRINGE_PIXELS: f32 = 0.75;
/// The largest distance a flattened curve may stray, in output pixels.
const TOLERANCE_PIXELS: f32 = 0.2;

impl Graphics {
    /// Output pixels per drawing unit under the current camera and
    /// transform; used to size curve flattening and the fringe.
    pub(crate) fn draw_scale(&self) -> f32 {
        let zoom = self.cursor.camera.map_or(1.0, |c| c.zoom.max(1e-3));
        (self.pixels_per_unit() * zoom * self.transform_scale()).max(1e-4)
    }

    /// Fills the inside of `path` as `fill` describes. Open subpaths are
    /// closed implicitly. Coordinates are drawing units, so the transform
    /// stack and camera apply.
    pub fn fill_path(&mut self, path: &Path, fill: &Fill) {
        let scale = self.draw_scale();
        let mut m = mesh::fill(path, fill.rule, TOLERANCE_PIXELS / scale);
        if fill.antialias {
            mesh::add_fringe(&mut m, FRINGE_PIXELS / scale);
        }
        self.emit_mesh(&m, &fill.paint);
    }

    /// Draws the outline of `path` as `stroke` describes.
    pub fn stroke_path(&mut self, path: &Path, stroke: &Stroke) {
        if stroke.width <= 0.0 || !stroke.width.is_finite() {
            return;
        }
        let scale = self.draw_scale();
        let mut m = mesh::stroke(path, stroke, TOLERANCE_PIXELS / scale);
        if stroke.antialias {
            mesh::add_fringe(&mut m, FRINGE_PIXELS / scale);
        }
        self.emit_mesh(&m, &stroke.paint);
    }

    /// The colour ramp texture for `gradient`, built on first use.
    fn ramp_texture(&mut self, gradient: &Gradient) -> Option<Texture> {
        let key = gradient.hash_key();
        if let Some(t) = self.path_cache.ramps.get(&key) {
            return Some(t.clone());
        }
        let mut pixels = Vec::with_capacity(RAMP_SIZE as usize * 4);
        for i in 0..RAMP_SIZE {
            let t = (i as f32 + 0.5) / RAMP_SIZE as f32;
            let c = gradient.sample(t);
            for v in &c[..3] {
                pixels.push((linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8);
            }
            pixels.push((c[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
        let options = TextureOptions {
            filter: Filter::Linear,
            wrap: gradient.spread,
            premultiplied: true,
            ..TextureOptions::default()
        };
        match Texture::from_pixels(self, RAMP_SIZE, 1, &pixels, options) {
            Ok(t) => {
                if self.path_cache.ramps.len() >= MAX_RAMPS {
                    self.path_cache.ramps.clear();
                }
                self.path_cache.ramps.insert(key, t.clone());
                Some(t)
            }
            Err(e) => {
                log::error!("yowie-gfx: gradient ramp failed: {e}");
                None
            }
        }
    }

    fn emit_mesh(&mut self, mesh: &Mesh, paint: &Paint) {
        if mesh.indices.is_empty() {
            return;
        }
        let white = self.white.clone();
        let (texture, base, uv, engine): (Texture, [f32; 4], UvFn, _) = match paint {
            Paint::Solid(c) => (
                white,
                self.vertex_color(*c),
                Box::new(|_| vec2(0.5, 0.5)),
                None,
            ),
            Paint::Linear {
                start,
                end,
                gradient,
            } => {
                let dir = *end - *start;
                let denom = dir.length_squared();
                let Some(ramp) = self.ramp_texture(gradient) else {
                    return;
                };
                let (start, inv) = (*start, if denom > 1e-12 { 1.0 / denom } else { 0.0 });
                (
                    ramp,
                    self.vertex_color(Color::WHITE),
                    Box::new(move |p| vec2((p - start).dot(dir) * inv, 0.5)),
                    None,
                )
            }
            Paint::Radial {
                center,
                radius,
                gradient,
            } => {
                if *radius <= 0.0 {
                    return;
                }
                let Some(ramp) = self.ramp_texture(gradient) else {
                    return;
                };
                let (center, inv) = (*center, 1.0 / *radius);
                (
                    ramp,
                    self.vertex_color(Color::WHITE),
                    Box::new(move |p| (p - center) * inv),
                    Some(RADIAL_SHADER),
                )
            }
            Paint::Texture(tp) => {
                let size = tp.texture.size().as_vec2() * tp.scale;
                let origin = tp.origin;
                (
                    tp.texture.clone(),
                    self.vertex_color(tp.tint),
                    Box::new(move |p| (p - origin) / size),
                    None,
                )
            }
        };
        let vertices: Vec<Vertex2D> = mesh
            .points
            .iter()
            .zip(&mesh.alpha)
            .map(|(p, a)| Vertex2D {
                position: p.to_array(),
                uv: uv(*p).to_array(),
                color: base.map(|c| c * a),
                mode: 0,
            })
            .collect();
        self.push_triangles_with(&texture, &vertices, &mesh.indices, engine);
    }
}
