//! Autotiling: picking tile frames from a terrain grid, so a map of plain
//! terrain ids draws with matching edges, corners and transitions.
//!
//! The game keeps one small integer per cell. A [`Mapper`] turns that into
//! frame indices for a tile sheet: whole maps at a time with
//! [`apply`](Mapper::apply), one changed cell at a time with
//! [`cell`](Mapper::cell), or one changed rectangle at a time with
//! [`region`](Mapper::region). The module is pure logic and needs no GPU;
//! frames are `Option<u32>` for [`Tilemap`](crate::Tilemap) or anything
//! else, and `None` means no tile.
//!
//! Five rule kinds cover the usual tilesets:
//!
//! - [`Rules::edge16`] matches the four edge neighbours and needs 16
//!   tiles: walls, pipes, fences.
//! - [`Rules::edge64`] is its hexagonal counterpart: the six neighbours of
//!   a hexagon, 64 tiles.
//! - [`Rules::blob47`] matches all eight neighbours, reduced to the 47
//!   distinct cases: the standard blob terrain set. [`expand_blob`]
//!   composes those 47 tiles from a six-tile template.
//! - [`Rules::corner16`] is the dual grid: each tile sits on a corner
//!   between four cells and needs 16 tiles.
//! - [`Rules::wang`] matches terrain colours on tile edges, corners or
//!   both, for any number of terrains meeting with proper transitions.
//!
//! Directions follow Tiled's clockwise order from north, see [`Dir`]. A
//! [`Layout`] says where each direction's neighbour lies: a square grid,
//! hexagons in staggered rows or columns or in axial coordinates, or an
//! isometric diamond grid. A hexagonal layout has six neighbours and no
//! diagonals, so it takes edge64 or Wang rules; blob47 and corner16 need
//! the eight neighbours of a square or isometric grid.
//! [`GridLayout::autotile_layout`](crate::GridLayout::autotile_layout)
//! names the layout matching a tilemap's grid.

mod blob;
mod layout;
mod mapper;
mod rules;
mod wang;

pub use blob::{blob_index, blob_masks, expand_blob};
pub use layout::{Dir, Layout};
pub use mapper::Mapper;
pub use rules::Rules;
pub use wang::{WangTile, WangType, wang_from_tiled};

#[cfg(test)]
mod tests;
