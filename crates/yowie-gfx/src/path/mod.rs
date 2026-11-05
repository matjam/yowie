//! Vector paths: lines, curves and arcs, filled and stroked with lyon.
//!
//! Build a [`Path`] with [`PathBuilder`] or a shape constructor, then draw
//! it with [`Graphics::fill_path`](crate::Graphics::fill_path) and
//! [`Graphics::stroke_path`](crate::Graphics::stroke_path). Paints are
//! solid colours, linear and radial gradients (colour ramps sampled in
//! linear light) and textures. Edges are anti-aliased by a fringe about
//! one pixel wide whose coverage falls to zero at its outer edge; turn it
//! off with `antialias(false)`. Drawing follows the current blend mode,
//! transform stack, camera, clip, layer and sort key like any other 2D
//! draw.

mod build;
mod draw;
mod mesh;
mod style;

pub use build::{Path, PathBuilder};
pub(crate) use draw::PathCache;
pub use style::{
    Cap, Dash, Fill, FillRule, Gradient, GradientStop, Join, Paint, Stroke, TexturePaint,
};
