//! Layers: tiles, objects, images and groups.

use yowie_core::{Color, Vec2};

use crate::gid::Gid;
use crate::object::Object;
use crate::property::Properties;

/// One layer of a map.
///
/// A layer inside a group inherits the group's offset, opacity, visibility
/// and tint: offsets add, opacities multiply and tints multiply. The model
/// keeps each layer's own values; [`Map::walk_layers`](crate::Map::walk_layers)
/// applies the inheritance.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// Unique id within the map.
    pub id: u32,
    /// Display name.
    pub name: String,
    /// Class set in the editor.
    pub class: String,
    /// False when the layer is hidden.
    pub visible: bool,
    /// Opacity from 0.0 to 1.0.
    pub opacity: f32,
    /// Offset in pixels relative to the parent.
    pub offset: Vec2,
    /// Parallax factor per axis; 1.0 scrolls with the camera, 0.0 stays fixed.
    pub parallax: Vec2,
    /// Colour multiplied into the layer when drawn, if set.
    pub tint: Option<Color>,
    /// Custom properties.
    pub properties: Properties,
    /// What the layer holds.
    pub kind: LayerKind,
}

/// The contents of a [`Layer`].
#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    /// A grid of tiles.
    Tiles(TileLayer),
    /// Objects.
    Objects(ObjectLayer),
    /// One image.
    Image(ImageLayer),
    /// Child layers, in draw order.
    Group(Vec<Layer>),
}

/// A block of cells in an infinite map's tile layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    /// Column of the chunk's first cell, in tiles; may be negative.
    pub x: i32,
    /// Row of the chunk's first cell, in tiles; may be negative.
    pub y: i32,
    /// Width in tiles.
    pub width: u32,
    /// Height in tiles.
    pub height: u32,
    /// `width * height` cells, row-major.
    pub data: Vec<Gid>,
}

/// A grid of tile ids.
///
/// A finite map's layer stores all cells in `data`. An infinite map's layer
/// stores them in `chunks`, leaves `data` empty, and sets `x`, `y`, `width`
/// and `height` to the bounds of its chunks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileLayer {
    /// Column of the first cell, in tiles; non-zero only on infinite maps.
    pub x: i32,
    /// Row of the first cell, in tiles; non-zero only on infinite maps.
    pub y: i32,
    /// Width in tiles.
    pub width: u32,
    /// Height in tiles.
    pub height: u32,
    /// `width * height` cells, row-major; empty for infinite maps.
    pub data: Vec<Gid>,
    /// Chunks of an infinite map's layer.
    pub chunks: Vec<Chunk>,
}

impl TileLayer {
    /// Returns the cell at map coordinates `(x, y)`, or an empty [`Gid`]
    /// outside the layer.
    pub fn gid_at(&self, x: i32, y: i32) -> Gid {
        if self.chunks.is_empty() {
            return cell(&self.data, self.x, self.y, self.width, self.height, x, y);
        }
        for c in &self.chunks {
            let found = cell(&c.data, c.x, c.y, c.width, c.height, x, y);
            if !found.is_empty() {
                return found;
            }
        }
        Gid(0)
    }

    /// Copies the layer into one dense row-major grid covering its bounds.
    /// For a finite layer this is `data` itself.
    pub fn dense(&self) -> Vec<Gid> {
        if self.chunks.is_empty() {
            return self.data.clone();
        }
        let mut out = vec![Gid(0); self.width as usize * self.height as usize];
        for c in &self.chunks {
            for row in 0..c.height as usize {
                let dst = (c.y - self.y) as usize + row;
                let start = dst * self.width as usize + (c.x - self.x) as usize;
                let src = &c.data[row * c.width as usize..(row + 1) * c.width as usize];
                out[start..start + src.len()].copy_from_slice(src);
            }
        }
        out
    }
}

fn cell(data: &[Gid], ox: i32, oy: i32, w: u32, h: u32, x: i32, y: i32) -> Gid {
    let (dx, dy) = (i64::from(x) - i64::from(ox), i64::from(y) - i64::from(oy));
    if dx < 0 || dy < 0 || dx >= i64::from(w) || dy >= i64::from(h) {
        return Gid(0);
    }
    data.get(dy as usize * w as usize + dx as usize)
        .copied()
        .unwrap_or(Gid(0))
}

/// The order in which an object layer's objects are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawOrder {
    /// By y position, lowest first.
    #[default]
    TopDown,
    /// In the order they are stored.
    Index,
}

/// A layer of objects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectLayer {
    /// The objects in file order.
    pub objects: Vec<Object>,
    /// The draw order.
    pub draw_order: DrawOrder,
    /// Colour the editor shows shapes in, if set.
    pub color: Option<Color>,
}

/// A layer that shows one image.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageLayer {
    /// Image path relative to the map's directory; empty when unset.
    pub image: String,
    /// Colour to treat as transparent, if set.
    pub transparent_color: Option<Color>,
    /// Repeat the image horizontally.
    pub repeat_x: bool,
    /// Repeat the image vertically.
    pub repeat_y: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gid_at_and_dense_for_chunks() {
        let chunk = |x, y, base: u32| Chunk {
            x,
            y,
            width: 2,
            height: 2,
            data: (0..4).map(|i| Gid(base + i)).collect(),
        };
        let layer = TileLayer {
            x: -2,
            y: 0,
            width: 4,
            height: 2,
            data: Vec::new(),
            chunks: vec![chunk(-2, 0, 1), chunk(0, 0, 11)],
        };
        let cases = [
            (-2, 0, 1),
            (-1, 1, 4),
            (0, 0, 11),
            (1, 1, 14),
            (2, 0, 0),
            (-3, 0, 0),
        ];
        for (x, y, want) in cases {
            assert_eq!(layer.gid_at(x, y), Gid(want), "({x},{y})");
        }
        let dense: Vec<u32> = layer.dense().iter().map(|g| g.0).collect();
        assert_eq!(dense, [1, 2, 11, 12, 3, 4, 13, 14]);
    }
}
