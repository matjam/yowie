//! The renderer: 2D sprites, shapes and text today; 3D scenes and
//! post-processing are added to the same frame by later work.
//!
//! [`Graphics`] owns the wgpu device and the frame's output, a window
//! surface or a headless image. Drawing calls queue work while the game
//! draws; [`Graphics::end_frame`] renders it.
//!
//! # Conventions
//!
//! 2D coordinates are view units with the origin at the top-left and +Y
//! down. Without a fixed view a view unit is a logical point of the window;
//! with one ([`ViewSettings::fixed`]) the view is scaled into the window
//! with letterboxing or integer scaling, and still renders at the window's
//! full pixel resolution. Angles are radians, clockwise on screen. Colours
//! passed in are sRGB with straight alpha ([`yowie_core::Color`]); the
//! renderer converts them to linear light and premultiplies them, blends in
//! linear light, and stores textures premultiplied.
//!
//! # The 2D batch
//!
//! Every 2D draw (sprites, shapes, nine-slices, glyphs) appends triangles
//! to one vertex stream per output. Draws carry a layer
//! ([`Graphics::set_layer`]) and a sort key ([`Graphics::set_sort_key`]);
//! the stream is sorted by layer, then sort key, then call order, and runs
//! that share a texture, shader, blend mode and view become one draw call.
//! Pipelines and bind groups are set once per run, never per sprite.
//!
//! # The frame
//!
//! A frame renders its outputs in order: first every render texture drawn
//! to with [`Graphics::draw_to`], in the order those scopes finished, then
//! the screen. Each output runs these passes, in this order:
//!
//! 1. **3D scene passes** (extension point, none yet). A 3D renderer records
//!    its shadow, opaque and transparent passes into its own HDR colour and
//!    depth targets here. Its draws are queued during `draw` like 2D ones
//!    and resolved in `end_frame`. The place is marked "Pass 1" in
//!    `Graphics::end_frame` (`crates/yowie-gfx/src/frame.rs`).
//! 2. **Post-processing chain** (extension point, none yet). Effects read
//!    the scene targets and the last one writes the output. When this pass
//!    runs, the 2D pass must load the output rather than clear it: set the
//!    list's `clear` to `None` for that frame. Marked "Pass 2" in the same
//!    function.
//! 3. **2D overlay** (`Pass2D` in `frame.rs`). The sprite batch drawn over
//!    whatever the scene left, or over the clear colour.
//!
//! The immediate-mode UI and the debug console draw through the 2D batch on
//! high layers, so they need no pass of their own: UI uses
//! [`layers::UI`], the console [`layers::CONSOLE`] and the engine's
//! frame-timing overlay [`layers::DEBUG`], all in screen space (no camera).
//! Particles and tilemaps draw through the same batch with
//! [`Graphics::draw_sprite`] and [`Graphics::draw_triangles`] on whatever
//! layer the game picks; a run of them sharing one texture is one draw
//! call. Static geometry that should skip the per-frame upload (render
//! bundles, instanced particles) belongs in its own pass beside the 2D
//! one.
//!
//! Render textures run the same pass list, so a 3D scene rendered into a
//! texture (a minimap, a portrait) uses the same hooks.

#![warn(missing_docs)]

mod batch;
mod blend;
mod camera;
mod device;
mod engine_shaders;
mod error;
mod frame;
mod geometry;
mod graphics;
mod half;
mod image_data;
mod mipmap;
mod nine_slice;
mod output;
mod path;
mod pipelines;
mod pixels;
mod readback;
mod render_texture;
pub mod scene;
mod shader;
mod shapes;
mod shared;
mod sprite;
mod state;
mod text;
mod texture;
mod texture_gpu;
mod texture_upload;
mod transform_stack;
mod view;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod gpu_tests_reload;
#[cfg(test)]
mod gpu_tests_text;
#[cfg(test)]
mod gpu_tests_textfx;
#[cfg(test)]
mod gpu_tests_vector;

// 2D world features: camera rig, sheets, atlases, Aseprite, colour
// matrices, tilemaps, autotiling and Tiled levels.
mod aseprite;
mod atlas;
pub mod autotile;
mod camera_rig;
mod color_matrix;
mod sheet;
mod tiled_level;
mod tilemap;

pub use aseprite::{AseSlice, AseSliceKey, AseTag, AsepriteData, AsepriteSheet};
pub use atlas::{Atlas, AtlasBuilder, AtlasPacker, PackRect};
pub use camera_rig::CameraRig;
pub use color_matrix::{ColorMatrix, ColorMatrixShader};
pub use sheet::{SheetGrid, SpriteSheet, sheet_flipbook};
pub use tiled_level::{LevelLayer, LevelLayerKind, TiledLevel};
pub use tilemap::{
    GridKind, GridLayout, Stagger, Tile, TileAnimation, TileAnimations, TileFlip, Tilemap,
};

pub use blend::BlendMode;
pub use camera::Camera2D;
pub use device::{adapter_available, default_adapter_info};
pub use error::{GfxError, Result};
pub use geometry::Vertex;
pub use graphics::{FrameStats, Graphics, GraphicsSettings};
pub use image_data::ImageData;
pub use nine_slice::NineSlice;
pub use path::{
    Cap, Dash, Fill, FillRule, Gradient, GradientStop, Join, Paint, Path, PathBuilder, Stroke,
    TexturePaint,
};
pub use render_texture::{RenderTexture, RenderTextureOptions};
pub use shader::Shader;
pub use sprite::Sprite;
pub use text::{
    Caret, Font, Language, Orientation, RichText, SpanStyle, TextAlign, TextLayout, TextStyle,
};
pub use texture::{Filter, Texture, TextureOptions, Wrap};
pub use view::{ScaleMode, ViewMapping, ViewSettings};

pub use image;
pub use wgpu;

/// Layers the engine and its UI draw on, so game layers can sit below or
/// between them.
pub mod layers {
    /// The layer immediate-mode UI draws on.
    pub const UI: i32 = 1 << 24;
    /// The layer the debug console draws on, above the UI.
    pub const CONSOLE: i32 = 1 << 25;
    /// The layer the engine's frame-timing overlay draws on, above
    /// everything.
    pub const DEBUG: i32 = 1 << 26;
}
