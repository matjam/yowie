//! Raw triangles through the sprite batch, for particles, tilemaps and any
//! other geometry a game builds itself.

use yowie_core::{Color, Vec2};

use crate::Graphics;
use crate::pipelines::Vertex2D;
use crate::texture::Texture;

/// One vertex of game-built geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Position in drawing coordinates.
    pub position: Vec2,
    /// Texture coordinate, 0 to 1 across the texture.
    pub uv: Vec2,
    /// Colour multiplied with the texture, sRGB with straight alpha.
    pub color: Color,
}

impl Vertex {
    /// A vertex at `position` with texture coordinate `uv` and `color`.
    pub fn new(position: Vec2, uv: Vec2, color: Color) -> Self {
        Self {
            position,
            uv,
            color,
        }
    }
}

impl Graphics {
    /// Queues triangles: `indices` name three vertices per triangle,
    /// counting from the first of `vertices`. `None` for the texture draws
    /// the vertex colours alone. Triangles with an index past the end of
    /// `vertices` are dropped.
    pub fn draw_triangles(
        &mut self,
        texture: Option<&Texture>,
        vertices: &[Vertex],
        indices: &[u32],
    ) {
        let n = vertices.len() as u32;
        let valid: Vec<u32> = indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| t.iter().all(|i| *i < n))
            .flatten()
            .copied()
            .collect();
        let converted: Vec<Vertex2D> = vertices
            .iter()
            .map(|v| Vertex2D {
                position: v.position.to_array(),
                uv: v.uv.to_array(),
                color: self.vertex_color(v.color),
                mode: 0,
            })
            .collect();
        let texture = texture.cloned().unwrap_or_else(|| self.white.clone());
        self.push_triangles(&texture, &converted, &valid);
    }
}
