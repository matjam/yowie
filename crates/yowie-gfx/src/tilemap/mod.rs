//! Tile layers on square, hexagonal and isometric grids.
//!
//! - [`GridLayout`] places cells in the plane and converts both ways
//!   between cell coordinates and world positions.
//! - [`Tile`] is a sheet frame with flips and rotations; [`Tilemap`] is a
//!   layer of tiles over one [`SpriteSheet`](crate::SpriteSheet), with
//!   animated tiles.
//! - [`Tilemap::draw`] culls to the camera's visible rectangle and queues
//!   the layer as one draw call.

mod anim;
mod draw;
mod layout;
mod map;
mod tile;

pub use anim::{TileAnimation, TileAnimations};
pub use layout::{GridKind, GridLayout, Stagger};
pub use map::Tilemap;
pub use tile::{Tile, TileFlip};
