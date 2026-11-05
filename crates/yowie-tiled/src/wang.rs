//! Wang sets (the terrain tool) and legacy terrain types.

use yowie_core::Color;

use crate::property::Properties;

/// Which positions of a tile a [`WangSet`] paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WangKind {
    /// Corners only.
    Corner,
    /// Edges only.
    Edge,
    /// Corners and edges.
    #[default]
    Mixed,
}

/// One colour of a [`WangSet`]. Colours are numbered from 1 in
/// [`WangTile::wang_id`], in the order they appear in [`WangSet::colors`].
#[derive(Clone, Debug, PartialEq)]
pub struct WangColor {
    /// Display name.
    pub name: String,
    /// Class set in the editor.
    pub class: String,
    /// The colour shown in the editor.
    pub color: Color,
    /// A representative tile's local id, or -1.
    pub tile: i32,
    /// Relative chance among variants; Tiled's default is 1.
    pub probability: f32,
    /// Custom properties.
    pub properties: Properties,
}

/// A tile's colours at its eight positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WangTile {
    /// Local id of the tile within the tileset.
    pub tile_id: u32,
    /// Colour indices (1-based, 0 for none) clockwise from north: N, NE, E,
    /// SE, S, SW, W, NW.
    pub wang_id: [u32; 8],
}

impl WangTile {
    /// The corner colours in clockwise order: NE, SE, SW, NW.
    pub fn corners(&self) -> [u32; 4] {
        let w = &self.wang_id;
        [w[1], w[3], w[5], w[7]]
    }

    /// The edge colours in clockwise order: N, E, S, W.
    pub fn edges(&self) -> [u32; 4] {
        let w = &self.wang_id;
        [w[0], w[2], w[4], w[6]]
    }
}

/// A terrain set: named colours and the tiles that carry them.
///
/// On hexagonal maps Tiled stores a hexagon's six sides in the same
/// eight-slot `wang_id`, one slot before the side's own direction; this
/// crate keeps the values as stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WangSet {
    /// Display name.
    pub name: String,
    /// Class set in the editor.
    pub class: String,
    /// Which positions the set paints.
    pub kind: WangKind,
    /// A representative tile's local id, or -1.
    pub tile: i32,
    /// The colours.
    pub colors: Vec<WangColor>,
    /// The tiles and their colours.
    pub tiles: Vec<WangTile>,
    /// Custom properties.
    pub properties: Properties,
}

impl WangSet {
    /// Returns the tiles whose position colours equal `wang_id`.
    pub fn tiles_matching(&self, wang_id: [u32; 8]) -> impl Iterator<Item = &WangTile> {
        self.tiles.iter().filter(move |t| t.wang_id == wang_id)
    }

    /// The selection weight of a tile: the product of the probabilities of
    /// the distinct colours it uses, so variants keep the balance set in the
    /// editor.
    pub fn weight(&self, tile: &WangTile) -> f32 {
        let mut seen = [false; 256];
        let mut weight = 1.0;
        for &c in &tile.wang_id {
            let idx = c as usize;
            if c > 0 && idx <= self.colors.len() && idx < 256 && !seen[idx] {
                seen[idx] = true;
                let p = self.colors[idx - 1].probability;
                if p > 0.0 {
                    weight *= p;
                }
            }
        }
        weight
    }
}

/// A terrain type of the legacy terrain system.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terrain {
    /// Display name.
    pub name: String,
    /// Local id of the representative tile, or -1.
    pub tile: i32,
    /// Custom properties.
    pub properties: Properties,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_edges_and_weight() {
        let color = |p| WangColor {
            name: String::new(),
            class: String::new(),
            color: Color::WHITE,
            tile: -1,
            probability: p,
            properties: Properties::default(),
        };
        let set = WangSet {
            colors: vec![color(2.0), color(0.5)],
            tiles: vec![WangTile {
                tile_id: 3,
                wang_id: [1, 2, 1, 2, 1, 2, 1, 2],
            }],
            ..Default::default()
        };
        let t = set.tiles[0];
        assert_eq!(t.corners(), [2, 2, 2, 2]);
        assert_eq!(t.edges(), [1, 1, 1, 1]);
        assert_eq!(set.weight(&t), 1.0);
        assert_eq!(set.tiles_matching([1, 2, 1, 2, 1, 2, 1, 2]).count(), 1);
        assert_eq!(set.tiles_matching([0; 8]).count(), 0);
    }
}
