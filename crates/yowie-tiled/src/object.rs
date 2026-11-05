//! Objects on object layers and in tile collision groups.

use yowie_core::{Color, Rect, Vec2};

use crate::gid::Gid;
use crate::property::Properties;

/// Horizontal alignment of a text object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HAlign {
    /// Align to the left edge.
    #[default]
    Left,
    /// Centre horizontally.
    Center,
    /// Align to the right edge.
    Right,
    /// Stretch lines to the full width.
    Justify,
}

/// Vertical alignment of a text object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    /// Align to the top edge.
    #[default]
    Top,
    /// Centre vertically.
    Center,
    /// Align to the bottom edge.
    Bottom,
}

/// The content and style of a text object.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    /// The text.
    pub text: String,
    /// Font family name; Tiled's default is `sans-serif`.
    pub font_family: String,
    /// Font size in pixels.
    pub pixel_size: f32,
    /// Wrap lines at the object's width.
    pub wrap: bool,
    /// Text colour.
    pub color: Color,
    /// Bold style.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Horizontal alignment.
    pub h_align: HAlign,
    /// Vertical alignment.
    pub v_align: VAlign,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_family: "sans-serif".to_string(),
            pixel_size: 16.0,
            wrap: false,
            color: Color::BLACK,
            bold: false,
            italic: false,
            h_align: HAlign::Left,
            v_align: VAlign::Top,
        }
    }
}

/// What an [`Object`] is.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A rectangle of the object's width and height.
    Rectangle,
    /// An ellipse inscribed in the object's rectangle.
    Ellipse,
    /// A single point at the object's position.
    Point,
    /// A closed polygon; vertices are pixels relative to the object's position.
    Polygon(Vec<Vec2>),
    /// An open polyline; vertices are pixels relative to the object's position.
    Polyline(Vec<Vec2>),
    /// A text box.
    Text(Text),
    /// A tile drawn at the object's position, anchored at its bottom-left.
    Tile(Gid),
}

/// A shape, tile or text placed on an object layer, or a collision shape in
/// a tile.
///
/// Positions and sizes are pixels; y grows downward. A tile object is
/// anchored at its bottom-left corner and every other object at its
/// top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    /// Unique id within the map.
    pub id: u32,
    /// Display name.
    pub name: String,
    /// Class (called type before Tiled 1.9).
    pub class: String,
    /// X position in pixels.
    pub x: f32,
    /// Y position in pixels.
    pub y: f32,
    /// Width in pixels; 0 for points and polygons.
    pub width: f32,
    /// Height in pixels; 0 for points and polygons.
    pub height: f32,
    /// Rotation in degrees, clockwise, about the anchor.
    pub rotation: f32,
    /// False when the object is hidden in the editor.
    pub visible: bool,
    /// What the object is.
    pub shape: Shape,
    /// Custom properties.
    pub properties: Properties,
}

impl Object {
    /// The stored position and size as a rectangle. It ignores rotation,
    /// polygon extents and the tile-object anchor.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }
}
