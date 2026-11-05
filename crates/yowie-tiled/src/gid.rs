//! Global tile ids and their flip flags.

/// A global tile id as stored in a layer, with Tiled's flip flags in the
/// four high bits.
///
/// Zero is an empty cell. To find a tile's tileset and local id, pass the
/// `Gid` to [`Map::tileset_for`](crate::Map::tileset_for).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Gid(pub u32);

/// The flip and rotation flags of a [`Gid`].
///
/// On orthogonal and isometric maps the three standard flags compose as
/// Tiled applies them: the diagonal flip (swap x and y) first, then the
/// horizontal and vertical flips. A 90-degree clockwise rotation is
/// `diagonal + horizontal`. On hexagonal maps `rotate_120` rotates the tile
/// 120 degrees clockwise in addition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileFlags {
    /// Mirror left to right.
    pub horizontal: bool,
    /// Mirror top to bottom.
    pub vertical: bool,
    /// Mirror across the main diagonal (swap x and y).
    pub diagonal: bool,
    /// Rotate 120 degrees clockwise; hexagonal maps only.
    pub rotate_120: bool,
}

impl Gid {
    /// Horizontal flip bit.
    pub const FLIP_HORIZONTAL: u32 = 0x8000_0000;
    /// Vertical flip bit.
    pub const FLIP_VERTICAL: u32 = 0x4000_0000;
    /// Diagonal flip bit.
    pub const FLIP_DIAGONAL: u32 = 0x2000_0000;
    /// Hexagonal 120-degree rotation bit.
    pub const ROTATE_HEX_120: u32 = 0x1000_0000;
    const ALL_FLAGS: u32 = 0xF000_0000;

    /// Reports whether the cell is empty.
    pub fn is_empty(self) -> bool {
        self.tile_id() == 0
    }

    /// The global id with every flag removed.
    pub fn tile_id(self) -> u32 {
        self.0 & !Self::ALL_FLAGS
    }

    /// The flags set on this id.
    pub fn flags(self) -> TileFlags {
        TileFlags {
            horizontal: self.0 & Self::FLIP_HORIZONTAL != 0,
            vertical: self.0 & Self::FLIP_VERTICAL != 0,
            diagonal: self.0 & Self::FLIP_DIAGONAL != 0,
            rotate_120: self.0 & Self::ROTATE_HEX_120 != 0,
        }
    }

    /// Splits the id into the id without flags and its flags.
    pub fn split(self) -> (u32, TileFlags) {
        (self.tile_id(), self.flags())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_table() {
        let cases = [
            (0u32, 0u32, TileFlags::default()),
            (5, 5, TileFlags::default()),
            (
                0x8000_0005,
                5,
                TileFlags {
                    horizontal: true,
                    ..Default::default()
                },
            ),
            (
                0x4000_0005,
                5,
                TileFlags {
                    vertical: true,
                    ..Default::default()
                },
            ),
            (
                0x2000_0005,
                5,
                TileFlags {
                    diagonal: true,
                    ..Default::default()
                },
            ),
            (
                0x1000_0005,
                5,
                TileFlags {
                    rotate_120: true,
                    ..Default::default()
                },
            ),
            (
                0xE000_0007,
                7,
                TileFlags {
                    horizontal: true,
                    vertical: true,
                    diagonal: true,
                    rotate_120: false,
                },
            ),
        ];
        for (raw, id, flags) in cases {
            assert_eq!(Gid(raw).split(), (id, flags), "{raw:#x}");
        }
        assert!(Gid(0xF000_0000).is_empty());
        assert!(!Gid(1).is_empty());
    }
}
