//! A tile: a sheet frame with flips.

use yowie_core::{Vec2, vec2};

/// How a tile's picture is mirrored or turned.
///
/// The flags follow Tiled: the picture is transposed first when
/// `diagonal` is set, then mirrored left to right for `horizontal`, then
/// top to bottom for `vertical`. A diagonal flip with a horizontal flip is
/// a quarter turn clockwise. `rotate_120` turns the picture 120 degrees
/// clockwise about the tile's middle and is meant for hexagonal maps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TileFlip {
    /// Mirror left to right.
    pub horizontal: bool,
    /// Mirror top to bottom.
    pub vertical: bool,
    /// Transpose the picture (swap its x and y axes).
    pub diagonal: bool,
    /// Turn the picture 120 degrees clockwise.
    pub rotate_120: bool,
}

impl TileFlip {
    /// No change.
    pub const NONE: Self = Self {
        horizontal: false,
        vertical: false,
        diagonal: false,
        rotate_120: false,
    };

    /// A quarter turn clockwise.
    pub const ROTATE_90: Self = Self {
        horizontal: true,
        vertical: false,
        diagonal: true,
        rotate_120: false,
    };

    /// A half turn.
    pub const ROTATE_180: Self = Self {
        horizontal: true,
        vertical: true,
        diagonal: false,
        rotate_120: false,
    };

    /// A quarter turn counter-clockwise.
    pub const ROTATE_270: Self = Self {
        horizontal: false,
        vertical: true,
        diagonal: true,
        rotate_120: false,
    };

    /// Whether the flip changes nothing.
    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// Where the picture's corner is sampled for each corner of the tile
    /// quad, clockwise from the top-left, as fractions of the frame.
    pub(crate) fn sample_corners(&self) -> [Vec2; 4] {
        let quad = [
            vec2(0.0, 0.0),
            vec2(1.0, 0.0),
            vec2(1.0, 1.0),
            vec2(0.0, 1.0),
        ];
        quad.map(|q| {
            let (mut x, mut y) = (q.x, q.y);
            if self.horizontal {
                x = 1.0 - x;
            }
            if self.vertical {
                y = 1.0 - y;
            }
            if self.diagonal {
                std::mem::swap(&mut x, &mut y);
            }
            vec2(x, y)
        })
    }
}

/// One cell of a [`Tilemap`](crate::Tilemap): a frame of the map's sheet,
/// or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    /// The sheet frame, or [`Tile::NO_FRAME`] for an empty cell.
    pub frame: u32,
    /// Mirroring and turning.
    pub flip: TileFlip,
}

impl Default for Tile {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Tile {
    /// The frame value of an empty cell.
    pub const NO_FRAME: u32 = u32::MAX;

    /// A cell that draws nothing.
    pub const EMPTY: Self = Self {
        frame: Self::NO_FRAME,
        flip: TileFlip::NONE,
    };

    /// A tile showing sheet frame `frame`, unflipped.
    pub const fn new(frame: u32) -> Self {
        Self {
            frame,
            flip: TileFlip::NONE,
        }
    }

    /// The same tile with another flip.
    pub const fn flipped(self, flip: TileFlip) -> Self {
        Self { flip, ..self }
    }

    /// Whether the cell draws nothing.
    pub fn is_empty(&self) -> bool {
        self.frame == Self::NO_FRAME
    }
}

impl From<Option<u32>> for Tile {
    fn from(frame: Option<u32>) -> Self {
        frame.map_or(Tile::EMPTY, Tile::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_sample_the_expected_corners() {
        let tl = vec2(0.0, 0.0);
        let tr = vec2(1.0, 0.0);
        let br = vec2(1.0, 1.0);
        let bl = vec2(0.0, 1.0);
        let cases = [
            (TileFlip::NONE, [tl, tr, br, bl]),
            // A clockwise quarter turn moves the picture's bottom-left to
            // the top-left of the quad.
            (TileFlip::ROTATE_90, [bl, tl, tr, br]),
            (TileFlip::ROTATE_180, [br, bl, tl, tr]),
            (TileFlip::ROTATE_270, [tr, br, bl, tl]),
            (
                TileFlip {
                    horizontal: true,
                    ..TileFlip::NONE
                },
                [tr, tl, bl, br],
            ),
        ];
        for (flip, want) in cases {
            assert_eq!(flip.sample_corners(), want, "{flip:?}");
        }
    }
}
