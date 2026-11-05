//! Wang tile sets: terrain colours on tile edges, corners or both.

use std::collections::HashMap;

use super::layout::{Dir, Layout};
use super::rules::Weighted;

/// Which positions of a Wang tile carry terrain colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WangType {
    /// The four diagonal positions; the usual form for overlapping
    /// terrains.
    Corners,
    /// The four edge positions; the usual form for paths and pipes.
    Edges,
    /// All eight positions.
    Full,
}

/// One tile of a Wang set: its frame and the terrain colour at each of the
/// eight positions, in [`Dir`] order. Colour 0 means the position is
/// empty. Tiles with equal colours are variants; `weight` biases the
/// choice among them and a value of 0 or less counts as 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WangTile {
    /// The sheet frame.
    pub frame: u32,
    /// The colour at north, north-east, east, south-east, south,
    /// south-west, west and north-west.
    pub colors: [i32; 8],
    /// Relative probability among tiles with the same colours.
    pub weight: f64,
}

impl WangTile {
    /// A tile with weight 1.
    pub fn new(frame: u32, colors: [i32; 8]) -> Self {
        Self {
            frame,
            colors,
            weight: 1.0,
        }
    }

    pub(super) fn weighted(&self) -> Weighted {
        Weighted {
            frame: self.frame,
            weight: if self.weight > 0.0 { self.weight } else { 1.0 },
        }
    }
}

const CORNERS: [Dir; 4] = [Dir::NE, Dir::SE, Dir::SW, Dir::NW];
const EDGES: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

/// The positions a Wang type matches under a layout.
pub(super) fn positions(t: WangType, layout: Layout) -> &'static [Dir] {
    if layout.is_hex() {
        return layout.dirs();
    }
    match t {
        WangType::Corners => &CORNERS,
        WangType::Edges => &EDGES,
        WangType::Full => &Dir::ALL,
    }
}

/// Wang tiles grouped by the colours they show at the matched positions,
/// so a cell whose wanted colours some tile matches exactly finds its
/// candidates without scanning the set.
#[derive(Clone, Debug, Default)]
pub(super) struct WangIndex {
    exact: HashMap<[i32; 8], Vec<Weighted>>,
}

impl WangIndex {
    pub(super) fn build(tiles: &[WangTile], positions: &[Dir]) -> Self {
        let mut exact: HashMap<[i32; 8], Vec<Weighted>> = HashMap::with_capacity(tiles.len());
        for tile in tiles {
            let mut key = [0; 8];
            for p in positions {
                key[p.index()] = tile.colors[p.index()];
            }
            exact.entry(key).or_default().push(tile.weighted());
        }
        Self { exact }
    }

    pub(super) fn exact(&self, want: &[i32; 8]) -> Option<&[Weighted]> {
        self.exact.get(want).map(Vec::as_slice)
    }
}

/// The best-scoring tiles for a wanted colour vector when no tile matches
/// exactly, in set order.
pub(super) fn closest(tiles: &[WangTile], positions: &[Dir], want: &[i32; 8]) -> Vec<Weighted> {
    let score = |t: &WangTile| {
        positions
            .iter()
            .filter(|p| t.colors[p.index()] == want[p.index()])
            .count()
    };
    let best = tiles.iter().map(score).max();
    tiles
        .iter()
        .filter(|t| Some(score(t)) == best)
        .map(WangTile::weighted)
        .collect()
}
