//! The map and its lookups.

use yowie_core::{Color, Vec2};

use crate::gid::Gid;
use crate::layer::{Layer, LayerKind};
use crate::property::Properties;
use crate::tileset::{Tile, Tileset};

/// How a map's grid is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Square cells.
    #[default]
    Orthogonal,
    /// Diamond cells on a diagonal grid.
    Isometric,
    /// Isometric cells with alternate rows or columns shifted.
    Staggered,
    /// Hexagonal cells.
    Hexagonal,
}

/// The order tiles are drawn in a tile layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderOrder {
    /// Left to right, top to bottom.
    #[default]
    RightDown,
    /// Left to right, bottom to top.
    RightUp,
    /// Right to left, top to bottom.
    LeftDown,
    /// Right to left, bottom to top.
    LeftUp,
}

/// The axis along which staggered or hexagonal cells are shifted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaggerAxis {
    /// Columns are shifted.
    X,
    /// Rows are shifted.
    Y,
}

/// Which rows or columns are shifted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaggerIndex {
    /// Odd rows or columns are shifted.
    Odd,
    /// Even rows or columns are shifted.
    Even,
}

/// A parsed Tiled map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    /// Grid layout.
    pub orientation: Orientation,
    /// Tile draw order.
    pub render_order: RenderOrder,
    /// Width in tiles. For infinite maps this is 0 in most files; use the
    /// layers' bounds.
    pub width: u32,
    /// Height in tiles; see `width`.
    pub height: u32,
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height in pixels.
    pub tile_height: u32,
    /// True when layers are stored in chunks and have no fixed size.
    pub infinite: bool,
    /// Stagger axis on staggered and hexagonal maps.
    pub stagger_axis: Option<StaggerAxis>,
    /// Stagger index on staggered and hexagonal maps.
    pub stagger_index: Option<StaggerIndex>,
    /// Length in pixels of the hexagon side along the stagger axis; 0 unless
    /// the map is hexagonal.
    pub hex_side_length: u32,
    /// Background colour, if set.
    pub background: Option<Color>,
    /// Origin the layers' parallax factors are relative to, in pixels.
    pub parallax_origin: Vec2,
    /// Class set in the editor.
    pub class: String,
    /// Layers in draw order, bottom first.
    pub layers: Vec<Layer>,
    /// Tilesets sorted by `first_gid`.
    pub tilesets: Vec<Tileset>,
    /// Custom properties.
    pub properties: Properties,
}

/// A layer with the state of its enclosing groups applied.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedLayer<'a> {
    /// The layer. It is never a group.
    pub layer: &'a Layer,
    /// Own offset plus the offsets of all enclosing groups, in pixels.
    pub offset: Vec2,
    /// Own opacity times the opacities of all enclosing groups.
    pub opacity: f32,
    /// True only if the layer and all enclosing groups are visible.
    pub visible: bool,
    /// Own tint times the tints of all enclosing groups; `None` if none set.
    pub tint: Option<Color>,
    /// Own parallax times the parallax of all enclosing groups.
    pub parallax: Vec2,
}

impl Map {
    /// Finds the tileset a global id belongs to and the tile's local id in it.
    /// Returns `None` for an empty cell or an id past every tileset.
    pub fn tileset_for(&self, gid: Gid) -> Option<(&Tileset, u32)> {
        let id = gid.tile_id();
        if id == 0 {
            return None;
        }
        let idx = self
            .tilesets
            .partition_point(|t| t.first_gid <= id)
            .checked_sub(1)?;
        let ts = &self.tilesets[idx];
        let local = id - ts.first_gid;
        (ts.tile_count == 0 || local < ts.tile_count).then_some((ts, local))
    }

    /// Returns the extra data attached to the tile behind a global id.
    pub fn tile(&self, gid: Gid) -> Option<&Tile> {
        let (ts, local) = self.tileset_for(gid)?;
        ts.tiles.get(&local)
    }

    /// Finds the first layer called `name`, searching groups depth first.
    pub fn find_layer(&self, name: &str) -> Option<&Layer> {
        fn find<'a>(layers: &'a [Layer], name: &str) -> Option<&'a Layer> {
            layers.iter().find_map(|l| {
                if l.name == name {
                    return Some(l);
                }
                match &l.kind {
                    LayerKind::Group(children) => find(children, name),
                    _ => None,
                }
            })
        }
        find(&self.layers, name)
    }

    /// Lists every non-group layer in draw order with its group state
    /// applied.
    pub fn walk_layers(&self) -> Vec<ResolvedLayer<'_>> {
        fn walk<'a>(
            layers: &'a [Layer],
            parent: &ResolvedLayer<'a>,
            out: &mut Vec<ResolvedLayer<'a>>,
        ) {
            for layer in layers {
                let tint = match (parent.tint, layer.tint) {
                    (Some(a), Some(b)) => {
                        Some(Color::rgba(a.r * b.r, a.g * b.g, a.b * b.b, a.a * b.a))
                    }
                    (a, b) => a.or(b),
                };
                let me = ResolvedLayer {
                    layer,
                    offset: parent.offset + layer.offset,
                    opacity: parent.opacity * layer.opacity,
                    visible: parent.visible && layer.visible,
                    tint,
                    parallax: parent.parallax * layer.parallax,
                };
                match &layer.kind {
                    LayerKind::Group(children) => walk(children, &me, out),
                    _ => out.push(me),
                }
            }
        }
        let Some(first) = self.layers.first() else {
            return Vec::new();
        };
        let root = ResolvedLayer {
            layer: first,
            offset: Vec2::ZERO,
            opacity: 1.0,
            visible: true,
            tint: None,
            parallax: Vec2::ONE,
        };
        let mut out = Vec::new();
        walk(&self.layers, &root, &mut out);
        out
    }

    /// The resolved tile layers, in draw order.
    pub fn tile_layers(&self) -> Vec<ResolvedLayer<'_>> {
        self.walk_layers()
            .into_iter()
            .filter(|r| matches!(r.layer.kind, LayerKind::Tiles(_)))
            .collect()
    }

    /// The resolved object layers, in order.
    pub fn object_layers(&self) -> Vec<ResolvedLayer<'_>> {
        self.walk_layers()
            .into_iter()
            .filter(|r| matches!(r.layer.kind, LayerKind::Objects(_)))
            .collect()
    }
}
