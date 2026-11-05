//! Tilesets and their tiles.

use std::collections::BTreeMap;

use yowie_core::{Color, Rect, Vec2};

use crate::object::Object;
use crate::property::Properties;
use crate::wang::{Terrain, WangSet};

/// The picture a tileset or tile is cut from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TilesetImage {
    /// Path relative to the map's directory (for an external tileset, already
    /// rebased from the tileset's own directory).
    pub source: String,
    /// Width in pixels, 0 when the file does not say.
    pub width: u32,
    /// Height in pixels, 0 when the file does not say.
    pub height: u32,
    /// Colour to treat as transparent, if set.
    pub transparent_color: Option<Color>,
}

/// One step of a tile animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Local id of the tile to show.
    pub tile_id: u32,
    /// How long to show it, in seconds.
    pub duration: f32,
}

/// The extra data a tileset attaches to one tile. Only tiles that have any
/// appear in [`Tileset::tiles`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    /// Local id within the tileset.
    pub id: u32,
    /// Class set in the editor.
    pub class: String,
    /// Relative chance of being picked by random brushes and terrain fill.
    pub probability: f32,
    /// The tile's own image, in image-collection tilesets.
    pub image: Option<TilesetImage>,
    /// Animation frames; empty when the tile is static.
    pub animation: Vec<Frame>,
    /// Collision shapes from the tile's object group.
    pub collision: Vec<Object>,
    /// Legacy terrain colour indices (into [`Tileset::terrains`]) at the
    /// top-left, top-right, bottom-left and bottom-right corners; `-1` for none.
    pub terrain: Option<[i32; 4]>,
    /// Custom properties.
    pub properties: Properties,
}

/// A set of tiles cut from one image grid, or a collection of single-tile
/// images.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tileset {
    /// Global id of the first tile. Zero for a tileset parsed on its own.
    pub first_gid: u32,
    /// For an external tileset, the path the map names it by.
    pub source: Option<String>,
    /// Display name.
    pub name: String,
    /// Class set in the editor.
    pub class: String,
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height in pixels.
    pub tile_height: u32,
    /// Gap between tiles in the image, in pixels.
    pub spacing: u32,
    /// Border around the tiles in the image, in pixels.
    pub margin: u32,
    /// Number of tiles.
    pub tile_count: u32,
    /// Tiles per image row; 0 for image-collection tilesets.
    pub columns: u32,
    /// The shared image; `None` for image-collection tilesets.
    pub image: Option<TilesetImage>,
    /// Drawing offset applied to every tile, in pixels.
    pub tile_offset: Vec2,
    /// Per-tile data by local id.
    pub tiles: BTreeMap<u32, Tile>,
    /// Terrain sets from the editor's terrain tool.
    pub wang_sets: Vec<WangSet>,
    /// Terrain types of the legacy (pre-1.5) terrain system.
    pub terrains: Vec<Terrain>,
    /// Custom properties.
    pub properties: Properties,
}

impl Tileset {
    /// Returns the pixel rectangle of tile `local_id` in the shared image,
    /// or `None` for an image-collection tileset or an id past the end.
    pub fn tile_rect(&self, local_id: u32) -> Option<Rect> {
        if self.columns == 0 || (self.tile_count > 0 && local_id >= self.tile_count) {
            return None;
        }
        let (col, row) = (local_id % self.columns, local_id / self.columns);
        let step_x = (self.tile_width + self.spacing) as f32;
        let step_y = (self.tile_height + self.spacing) as f32;
        Some(Rect::new(
            self.margin as f32 + col as f32 * step_x,
            self.margin as f32 + row as f32 * step_y,
            self.tile_width as f32,
            self.tile_height as f32,
        ))
    }

    /// Returns the properties of tile `local_id`, if it has any entry.
    pub fn tile(&self, local_id: u32) -> Option<&Tile> {
        self.tiles.get(&local_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_rect_uses_margin_and_spacing() {
        let ts = Tileset {
            tile_width: 16,
            tile_height: 16,
            margin: 1,
            spacing: 2,
            columns: 2,
            tile_count: 4,
            ..Default::default()
        };
        assert_eq!(ts.tile_rect(0), Some(Rect::new(1.0, 1.0, 16.0, 16.0)));
        assert_eq!(ts.tile_rect(3), Some(Rect::new(19.0, 19.0, 16.0, 16.0)));
        assert_eq!(ts.tile_rect(4), None);
        assert_eq!(Tileset::default().tile_rect(0), None);
    }
}
