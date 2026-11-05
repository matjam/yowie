//! Directions and the neighbour rules of each grid shape.

use yowie_core::{IVec2, ivec2};

/// The eight directions, clockwise from north in Tiled's order. A
/// direction's index numbers the bits of a blob mask and the slots of
/// [`WangTile::colors`](super::WangTile::colors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    /// Up.
    N = 0,
    /// Up and right.
    NE = 1,
    /// Right.
    E = 2,
    /// Down and right.
    SE = 3,
    /// Down.
    S = 4,
    /// Down and left.
    SW = 5,
    /// Left.
    W = 6,
    /// Up and left.
    NW = 7,
}

impl Dir {
    /// All eight directions, clockwise from north.
    pub const ALL: [Dir; 8] = [
        Dir::N,
        Dir::NE,
        Dir::E,
        Dir::SE,
        Dir::S,
        Dir::SW,
        Dir::W,
        Dir::NW,
    ];

    /// The direction's number, 0 for north to 7 for north-west.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The shape of the grid a [`Mapper`](super::Mapper) walks: which
/// directions a cell has neighbours in, and which cell each lies in.
///
/// A hexagonal layout has six neighbours and no diagonals. The rows
/// layouts hold pointy-top hexagons in staggered rows (Tiled's stagger
/// axis Y): every cell has an east and a west neighbour. The columns
/// layouts hold flat-top hexagons in staggered columns (stagger axis X):
/// every cell has a north and a south neighbour. Odd and even name the
/// shifted rows or columns, as Tiled's stagger index does. Shifted rows
/// move right; shifted columns move down.
///
/// [`IsoDiamond`](Layout::IsoDiamond) is a square grid drawn as diamonds,
/// and its direction names are screen directions: the cell north of
/// `(x, y)` is `(x - 1, y - 1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Layout {
    /// A square grid: four edges and four diagonals.
    #[default]
    Square,
    /// Hexagons in staggered rows, the odd rows shifted right.
    HexRowsOdd,
    /// Hexagons in staggered rows, the even rows shifted right.
    HexRowsEven,
    /// Hexagons in staggered columns, the odd columns shifted down.
    HexColsOdd,
    /// Hexagons in staggered columns, the even columns shifted down.
    HexColsEven,
    /// Hexagons in axial coordinates: `x` runs east and `y` south-east, so
    /// every cell has the same offsets.
    HexAxial,
    /// A square grid drawn as isometric diamonds, with screen directions.
    IsoDiamond,
}

type Offsets = [(i32, i32); 8];

const SQUARE: Offsets = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

// Entries for directions a layout does not use stay (0, 0).
const ROWS_UNSHIFTED: Offsets = [
    (0, 0),
    (0, -1),
    (1, 0),
    (0, 1),
    (0, 0),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];
const ROWS_SHIFTED: Offsets = [
    (0, 0),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
];
const COLS_UNSHIFTED: Offsets = [
    (0, -1),
    (1, -1),
    (0, 0),
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, 0),
    (-1, -1),
];
const COLS_SHIFTED: Offsets = [
    (0, -1),
    (1, 0),
    (0, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (0, 0),
    (-1, 0),
];
const AXIAL: Offsets = [
    (0, 0),
    (1, -1),
    (1, 0),
    (0, 1),
    (0, 0),
    (-1, 1),
    (-1, 0),
    (0, -1),
];
const ISO: Offsets = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
];

const SQUARE_DIRS: [Dir; 8] = Dir::ALL;
const ROW_DIRS: [Dir; 6] = [Dir::NE, Dir::E, Dir::SE, Dir::SW, Dir::W, Dir::NW];
const COL_DIRS: [Dir; 6] = [Dir::N, Dir::NE, Dir::SE, Dir::S, Dir::SW, Dir::NW];

impl Layout {
    /// The cell one step from `cell` in direction `dir`. A direction the
    /// layout has no neighbour in, such as north on a rows layout, returns
    /// the cell itself.
    pub fn neighbour(self, cell: IVec2, dir: Dir) -> IVec2 {
        let shifted = |index: i32, odd: bool| (index.rem_euclid(2) == 1) == odd;
        let table = match self {
            Layout::Square => &SQUARE,
            Layout::HexRowsOdd | Layout::HexRowsEven => {
                if shifted(cell.y, self == Layout::HexRowsOdd) {
                    &ROWS_SHIFTED
                } else {
                    &ROWS_UNSHIFTED
                }
            }
            Layout::HexColsOdd | Layout::HexColsEven => {
                if shifted(cell.x, self == Layout::HexColsOdd) {
                    &COLS_SHIFTED
                } else {
                    &COLS_UNSHIFTED
                }
            }
            Layout::HexAxial => &AXIAL,
            Layout::IsoDiamond => &ISO,
        };
        let (dx, dy) = table[dir.index()];
        cell + ivec2(dx, dy)
    }

    /// The directions the layout has neighbours in, clockwise: eight for
    /// square and isometric layouts, six for hexagonal ones.
    pub fn dirs(self) -> &'static [Dir] {
        match self {
            Layout::HexRowsOdd | Layout::HexRowsEven | Layout::HexAxial => &ROW_DIRS,
            Layout::HexColsOdd | Layout::HexColsEven => &COL_DIRS,
            Layout::Square | Layout::IsoDiamond => &SQUARE_DIRS,
        }
    }

    /// Whether the layout is hexagonal: six neighbours and no diagonals.
    pub fn is_hex(self) -> bool {
        matches!(
            self,
            Layout::HexRowsOdd
                | Layout::HexRowsEven
                | Layout::HexColsOdd
                | Layout::HexColsEven
                | Layout::HexAxial
        )
    }
}
