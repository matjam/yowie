//! Cell grids with A* pathfinding, Dijkstra maps, lines, field of view and
//! flood fill.
//!
//! Positions are [`IVec2`] with `x` to the right and `y` down; cell
//! `(0, 0)` is the top left. Costs are `f32`: a finite non-negative cost is
//! the price of stepping onto a cell, and [`BLOCKED`] (or any negative or
//! NaN value) forbids the step.
//!
//! - [`Grid`] stores cells and finds [4 or 8 neighbours](Neighbourhood).
//! - [`Pathfinder`] and [`astar`] find least-cost paths; [`dijkstra`],
//!   [`flee_map`] and [`downhill`] build and follow distance maps.
//! - [`line`](fn@line) and [`supercover_line`] trace lines between cells.
//! - [`fov`] computes what an observer sees with symmetric shadowcasting.
//! - [`flood_fill`] collects a connected region.
//!
//! ```
//! use yowie_core::ivec2;
//! use yowie_grid::{BLOCKED, Diagonal, astar};
//!
//! // A wall at x = 2 with a gap at y = 3.
//! let cost = |_from, to: yowie_core::IVec2| {
//!     if to.x == 2 && to.y != 3 { BLOCKED } else { 1.0 }
//! };
//! let path = astar(5, 5, ivec2(0, 0), ivec2(4, 0), Diagonal::Never, cost).unwrap();
//! assert_eq!(path.cost, 10.0);
//! ```

#![warn(missing_docs)]

mod dijkstra;
mod fill;
mod fov;
mod grid;
mod heap;
mod line;
mod path;
mod step;

pub use dijkstra::{dijkstra, downhill, flee_map};
pub use fill::flood_fill;
pub use fov::{fov, fov_cells};
pub use grid::{DIRS4, DIRS8, Grid, Neighbourhood};
pub use line::{line, supercover_line};
pub use path::{Path, Pathfinder, astar};
pub use step::{BLOCKED, Diagonal};
pub use yowie_core::IVec2;
