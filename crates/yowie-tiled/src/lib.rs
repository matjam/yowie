//! Maps from the Tiled editor, read into plain data.
//!
//! The crate parses TMX and TSX (XML) and TMJ and TSJ (JSON) files into a
//! [`Map`]. It needs no GPU: drawing a map is the renderer's job, using
//! the layers, tile ids and tileset image rectangles this crate provides.
//!
//! A map holds:
//!
//! - tile layers in CSV or base64 (uncompressed, zlib, gzip or zstd), with
//!   flip, diagonal and hexagonal rotation flags in each [`Gid`], and
//!   infinite maps as [`Chunk`]s;
//! - object layers with rectangles, ellipses, points, polygons, polylines,
//!   text and tile objects ([`Shape`]);
//! - image layers and group layers, with offsets, parallax, opacity and tint
//!   ([`Map::walk_layers`] applies a group's state to its children);
//! - embedded and external tilesets, tile animations, collision shapes,
//!   [`WangSet`]s and legacy [`Terrain`]s;
//! - typed custom properties ([`Properties`]);
//! - object templates (TX and TJ files), resolved while the map loads:
//!   an object that names a template has the template's fields, its own
//!   fields win, properties merge by name, and a tile object's gid is moved
//!   into the map's numbering ([`Object::template`] keeps the file name).
//!
//! Orthogonal, isometric, staggered and hexagonal maps parse the same way;
//! see [`Orientation`] for the layout fields.
//!
//! # Loading
//!
//! ```
//! use yowie_tiled::{Gid, LayerKind, Map};
//!
//! let tmx = br#"<map orientation="orthogonal" width="2" height="1" tilewidth="16" tileheight="16">
//!   <tileset firstgid="1" name="t" tilewidth="16" tileheight="16" tilecount="4" columns="2">
//!     <image source="t.png" width="32" height="32"/>
//!   </tileset>
//!   <layer id="1" name="ground" width="2" height="1"><data encoding="csv">1,2</data></layer>
//! </map>"#;
//! let map = Map::parse_embedded(tmx)?;
//! let LayerKind::Tiles(ground) = &map.layers[0].kind else { unreachable!() };
//! let (tileset, local) = map.tileset_for(ground.gid_at(1, 0)).expect("tile 2 exists");
//! assert_eq!((tileset.name.as_str(), local), ("t", 1));
//! assert_eq!(tileset.tile_rect(local).map(|r| r.x), Some(16.0));
//! # Ok::<(), yowie_tiled::Error>(())
//! ```
//!
//! To load from files, use [`Map::load`] with a [`yowie_asset::Vfs`], or
//! register [`MapLoader`] with [`yowie_asset::Assets`] to get background
//! loading and hot reload.

#![warn(missing_docs)]

mod data;
mod error;
mod gid;
mod layer;
mod load;
mod map;
mod object;
mod parse;
mod property;
mod tileset;
mod wang;
mod xml;

pub use error::{Error, Result};
pub use gid::{Gid, TileFlags};
pub use layer::{Chunk, DrawOrder, ImageLayer, Layer, LayerKind, ObjectLayer, TileLayer};
pub use load::MapLoader;
pub use map::{Map, Orientation, RenderOrder, ResolvedLayer, StaggerAxis, StaggerIndex};
pub use object::{HAlign, Object, Shape, Text, VAlign};
pub use property::{Properties, PropertyValue, parse_color};
pub use tileset::{Frame, Tile, Tileset, TilesetImage};
pub use wang::{Terrain, WangColor, WangKind, WangSet, WangTile};

#[cfg(test)]
mod tests;
