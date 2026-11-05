//! Rule sets: how a neighbourhood maps to tile frames.

use std::collections::HashSet;

use super::blob::blob_index;
use super::wang::{WangTile, WangType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Edge16,
    Edge64,
    Corner16,
    Blob47,
    Wang,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Weighted {
    pub(super) frame: u32,
    pub(super) weight: f64,
}

/// Maps a cell's neighbourhood to tile frames. Build one with a
/// constructor, then hand it to a [`Mapper`](super::Mapper).
///
/// Frames are indices into the tile sheet. `None` leaves a case empty.
#[derive(Clone, Debug)]
pub struct Rules {
    pub(super) kind: Kind,
    pub(super) terrain: i32,
    pub(super) friends: HashSet<i32>,
    pub(super) slots: Vec<Vec<Weighted>>,
    pub(super) wang_type: WangType,
    pub(super) wang: Vec<WangTile>,
}

impl Rules {
    fn masked(kind: Kind, terrain: i32, frames: &[Option<u32>]) -> Self {
        Self {
            kind,
            terrain,
            friends: HashSet::new(),
            slots: frames
                .iter()
                .map(|f| {
                    f.map(|frame| Weighted { frame, weight: 1.0 })
                        .into_iter()
                        .collect()
                })
                .collect(),
            wang_type: WangType::Corners,
            wang: Vec::new(),
        }
    }

    /// Rules matching the four edge neighbours of cells with the given
    /// terrain id, for walls, pipes and fences. `frames` is indexed by a
    /// 4-bit mask of connected neighbours: north 1, east 2, south 4, west
    /// 8.
    pub fn edge16(terrain: i32, frames: [Option<u32>; 16]) -> Self {
        Self::masked(Kind::Edge16, terrain, &frames)
    }

    /// The hexagonal counterpart of [`edge16`](Self::edge16): rules
    /// matching the six neighbours of cells with the given terrain id.
    /// `frames` is indexed by a 6-bit mask of connected neighbours, bit
    /// `i` being the `i`-th direction of [`Layout::dirs`](super::Layout::dirs):
    /// for the rows layouts north-east, east, south-east, south-west,
    /// west and north-west; for the columns layouts north, north-east,
    /// south-east, south, south-west and north-west.
    pub fn edge64(terrain: i32, frames: [Option<u32>; 64]) -> Self {
        Self::masked(Kind::Edge64, terrain, &frames)
    }

    /// Dual-grid rules for cells with the given terrain id. Each output
    /// tile sits on a corner between four cells, so the mapper emits a
    /// grid one wider and one taller than the map; draw that tilemap
    /// shifted up and left by half a tile. `frames` is indexed by a 4-bit
    /// mask of matching cells around the corner: north-west 1, north-east
    /// 2, south-west 4, south-east 8. Mask 0 never produces a tile.
    pub fn corner16(terrain: i32, frames: [Option<u32>; 16]) -> Self {
        Self::masked(Kind::Corner16, terrain, &frames)
    }

    /// Rules matching all eight neighbours of cells with the given terrain
    /// id, reduced to the 47 distinct cases (a diagonal only matters when
    /// both edges beside it connect). `frames` is in canonical order,
    /// ascending by normalised mask; [`blob_index`] maps any raw mask to
    /// its place there, and [`expand_blob`](super::expand_blob) emits
    /// tiles in this order.
    pub fn blob47(terrain: i32, frames: [Option<u32>; 47]) -> Self {
        Self::masked(Kind::Blob47, terrain, &frames)
    }

    /// Rules from a Wang tile set. Cells hold terrain colours, 0 for
    /// empty. For each cell the mapper works out the colour every relevant
    /// position should show, then places the tile matching the most
    /// positions; a complete set always matches exactly, an incomplete one
    /// falls back to the closest tile. Where two terrains meet, the higher
    /// colour wins. A cell whose own and surrounding colours are all 0
    /// gets no tile.
    ///
    /// A hexagonal layout has six sides and no diagonals, so every type
    /// matches those six positions, in the direction slots the layout
    /// uses. An empty set maps every cell to no tile.
    pub fn wang(wang_type: WangType, tiles: Vec<WangTile>) -> Self {
        Self {
            kind: Kind::Wang,
            terrain: 0,
            friends: HashSet::new(),
            slots: Vec::new(),
            wang_type,
            wang: tiles,
        }
    }

    /// Makes neighbours of other terrains count as connected, so grass
    /// rules join up against road cells without drawing their tiles.
    pub fn connect(mut self, terrains: impl IntoIterator<Item = i32>) -> Self {
        self.friends.extend(terrains);
        self
    }

    /// Adds an alternative frame for one neighbourhood, chosen by cell
    /// position with the given weight; the frame passed to the constructor
    /// keeps weight 1. `mask` is the scheme's mask: 4 bits for edge16 and
    /// corner16, 6 for edge64, the raw 8-bit mask for blob47. A
    /// non-positive weight counts as 1; an out-of-range mask is ignored.
    /// Wang rules take variants as extra tiles instead.
    pub fn variant(mut self, mask: usize, frame: u32, weight: f64) -> Self {
        let weight = if weight > 0.0 { weight } else { 1.0 };
        let slot = if self.kind == Kind::Blob47 && mask < 256 {
            blob_index(mask as u8)
        } else {
            mask
        };
        if let Some(s) = self.slots.get_mut(slot) {
            s.push(Weighted { frame, weight });
        }
        self
    }

    pub(super) fn connected(&self, neighbour: i32, terrain: i32) -> bool {
        neighbour == terrain || self.friends.contains(&neighbour)
    }
}
