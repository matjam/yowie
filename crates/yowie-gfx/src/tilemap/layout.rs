//! Grid geometry: where cells lie in the plane, and which cell holds a
//! point.

use yowie_core::{IVec2, Rect, UVec2, Vec2, ivec2, vec2};

use crate::autotile;

/// Which rows or columns of a hexagonal grid are shifted by half a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stagger {
    /// Odd rows (or columns) are shifted: indices 1, 3, 5 and so on.
    Odd,
    /// Even rows (or columns) are shifted: indices 0, 2, 4 and so on.
    Even,
}

/// The shape of a grid's cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GridKind {
    /// Rectangular cells in rows and columns.
    Square,
    /// Square cells seen as diamonds: `x` runs down and to the right, `y`
    /// runs down and to the left.
    Isometric,
    /// Pointy-top hexagons in staggered rows; shifted rows move right.
    HexPointy(Stagger),
    /// Flat-top hexagons in staggered columns; shifted columns move down.
    HexFlat(Stagger),
}

/// A grid of cells with its geometry, in world units with +Y down.
///
/// Every kind converts cell coordinates to world positions
/// ([`cell_center`](Self::cell_center)) and back
/// ([`cell_at`](Self::cell_at)). Cell `(0, 0)` has its bounding box's
/// top-left at the origin, except on an isometric grid, whose `x` axis
/// runs down and to the right and whose `y` axis down and to the left, so
/// cells with a large `y` lie at negative world `x`; the top corner of
/// cell `(0, 0)` is at `(0, 0)`.
///
/// `cell` is the bounding box of one cell: its width and height for
/// squares, diamonds and hexagons. For hexagons `hex_side` is the length
/// of the side parallel to the stagger axis' perpendicular: the vertical
/// side of a pointy hexagon or the horizontal side of a flat one; zero
/// means half the cell's height (pointy) or width (flat), which makes a
/// regular hexagon when the cell has the matching proportions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    /// The cell shape.
    pub kind: GridKind,
    /// The bounding box of one cell.
    pub cell: Vec2,
    /// The hexagon side length; see the type documentation.
    pub hex_side: f32,
}

impl GridLayout {
    /// A square grid of `cell`-sized cells.
    pub fn square(cell: Vec2) -> Self {
        Self::new(GridKind::Square, cell)
    }

    /// An isometric grid whose diamonds fit in `cell`.
    pub fn isometric(cell: Vec2) -> Self {
        Self::new(GridKind::Isometric, cell)
    }

    /// Pointy-top hexagons in staggered rows.
    pub fn hex_pointy(cell: Vec2, stagger: Stagger) -> Self {
        Self::new(GridKind::HexPointy(stagger), cell)
    }

    /// Flat-top hexagons in staggered columns.
    pub fn hex_flat(cell: Vec2, stagger: Stagger) -> Self {
        Self::new(GridKind::HexFlat(stagger), cell)
    }

    /// A grid of `kind` with default hexagon sides.
    pub fn new(kind: GridKind, cell: Vec2) -> Self {
        Self {
            kind,
            cell,
            hex_side: 0.0,
        }
    }

    /// The same grid with another hexagon side length.
    pub fn with_hex_side(self, hex_side: f32) -> Self {
        Self { hex_side, ..self }
    }

    fn side(&self) -> f32 {
        if self.hex_side > 0.0 {
            return self.hex_side;
        }
        match self.kind {
            GridKind::HexFlat(_) => self.cell.x * 0.5,
            _ => self.cell.y * 0.5,
        }
    }

    /// Distance between row centres of a pointy grid or column centres of
    /// a flat one.
    fn step(&self) -> f32 {
        match self.kind {
            GridKind::HexFlat(_) => (self.cell.x + self.side()) * 0.5,
            _ => (self.cell.y + self.side()) * 0.5,
        }
    }

    fn shifted(stagger: Stagger, index: i32) -> bool {
        let odd = index.rem_euclid(2) == 1;
        odd == (stagger == Stagger::Odd)
    }

    /// The world position of the middle of a cell.
    pub fn cell_center(&self, cell: IVec2) -> Vec2 {
        let (x, y) = (cell.x as f32, cell.y as f32);
        let c = self.cell;
        match self.kind {
            GridKind::Square => vec2((x + 0.5) * c.x, (y + 0.5) * c.y),
            GridKind::Isometric => vec2((x - y) * c.x * 0.5, (x + y + 1.0) * c.y * 0.5),
            GridKind::HexPointy(s) => {
                let shift = if Self::shifted(s, cell.y) { 0.5 } else { 0.0 };
                vec2((x + 0.5 + shift) * c.x, c.y * 0.5 + y * self.step())
            }
            GridKind::HexFlat(s) => {
                let shift = if Self::shifted(s, cell.x) { 0.5 } else { 0.0 };
                vec2(c.x * 0.5 + x * self.step(), (y + 0.5 + shift) * c.y)
            }
        }
    }

    /// The top-left of a cell's bounding box.
    pub fn cell_origin(&self, cell: IVec2) -> Vec2 {
        self.cell_center(cell) - self.cell * 0.5
    }

    /// The cell holding a world position. On the border between two cells
    /// either may be returned. The result can lie outside any map.
    pub fn cell_at(&self, p: Vec2) -> IVec2 {
        let c = self.cell;
        match self.kind {
            GridKind::Square => ivec2((p.x / c.x).floor() as i32, (p.y / c.y).floor() as i32),
            GridKind::Isometric => {
                let u = p.x / (c.x * 0.5);
                let v = p.y / (c.y * 0.5) - 1.0;
                ivec2(
                    ((u + v) * 0.5).round() as i32,
                    ((v - u) * 0.5).round() as i32,
                )
            }
            GridKind::HexPointy(_) => {
                let row = ((p.y - c.y * 0.5) / self.step()).round() as i32;
                let col = (p.x / c.x).floor() as i32;
                self.nearest(p, ivec2(col, row))
            }
            GridKind::HexFlat(_) => {
                let col = ((p.x - c.x * 0.5) / self.step()).round() as i32;
                let row = (p.y / c.y).floor() as i32;
                self.nearest(p, ivec2(col, row))
            }
        }
    }

    /// The candidate near `guess` whose centre is closest to `p`, with
    /// distances measured so that a lattice of staggered cells is regular.
    fn nearest(&self, p: Vec2, guess: IVec2) -> IVec2 {
        let scale = match self.kind {
            GridKind::HexFlat(_) => vec2((self.cell.y * 0.866_025_4) / self.step(), 1.0),
            _ => vec2(1.0, (self.cell.x * 0.866_025_4) / self.step()),
        };
        let mut best = guess;
        let mut best_d = f32::MAX;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let cell = guess + ivec2(dx, dy);
                let d = ((self.cell_center(cell) - p) * scale).length_squared();
                if d < best_d {
                    best_d = d;
                    best = cell;
                }
            }
        }
        best
    }

    /// The corners of a cell's outline, clockwise from the top or left:
    /// four for squares and diamonds, six for hexagons.
    pub fn cell_corners(&self, cell: IVec2) -> Vec<Vec2> {
        let m = self.cell_center(cell);
        let (hw, hh) = (self.cell.x * 0.5, self.cell.y * 0.5);
        match self.kind {
            GridKind::Square => vec![
                m + vec2(-hw, -hh),
                m + vec2(hw, -hh),
                m + vec2(hw, hh),
                m + vec2(-hw, hh),
            ],
            GridKind::Isometric => vec![
                m + vec2(0.0, -hh),
                m + vec2(hw, 0.0),
                m + vec2(0.0, hh),
                m + vec2(-hw, 0.0),
            ],
            GridKind::HexPointy(_) => {
                let s = self.side() * 0.5;
                vec![
                    m + vec2(0.0, -hh),
                    m + vec2(hw, -s),
                    m + vec2(hw, s),
                    m + vec2(0.0, hh),
                    m + vec2(-hw, s),
                    m + vec2(-hw, -s),
                ]
            }
            GridKind::HexFlat(_) => {
                let s = self.side() * 0.5;
                vec![
                    m + vec2(-hw, 0.0),
                    m + vec2(-s, -hh),
                    m + vec2(s, -hh),
                    m + vec2(hw, 0.0),
                    m + vec2(s, hh),
                    m + vec2(-s, hh),
                ]
            }
        }
    }

    /// The world rectangle covering a map of `size` cells.
    pub fn map_bounds(&self, size: UVec2) -> Rect {
        let corners = [
            ivec2(0, 0),
            ivec2(size.x as i32 - 1, 0),
            ivec2(0, size.y as i32 - 1),
            ivec2(size.x as i32 - 1, size.y as i32 - 1),
            ivec2(size.x as i32 - 1, 1.min(size.y as i32 - 1)),
            ivec2(1.min(size.x as i32 - 1), size.y as i32 - 1),
        ];
        let mut rect: Option<Rect> = None;
        for cell in corners {
            let r = Rect::from_center(self.cell_center(cell), self.cell);
            rect = Some(rect.map_or(r, |acc| acc.union(&r)));
        }
        rect.unwrap_or(Rect::ZERO)
    }

    /// The inclusive range of cells, clamped to a map of `size`, that can
    /// touch `area` (world units) when each cell's picture reaches `reach`
    /// units past its bounding box on every side. `None` when the range
    /// is empty.
    pub fn cells_in(&self, area: Rect, reach: Vec2, size: UVec2) -> Option<(IVec2, IVec2)> {
        let a = Rect::new(
            area.x - reach.x,
            area.y - reach.y,
            area.w + 2.0 * reach.x,
            area.h + 2.0 * reach.y,
        );
        let c = self.cell;
        let (lo, hi) = match self.kind {
            GridKind::Square => (
                ivec2((a.x / c.x).floor() as i32, (a.y / c.y).floor() as i32),
                ivec2(
                    ((a.x + a.w) / c.x).floor() as i32,
                    ((a.y + a.h) / c.y).floor() as i32,
                ),
            ),
            GridKind::Isometric => {
                let corners = [a.min(), vec2(a.x + a.w, a.y), a.max(), vec2(a.x, a.y + a.h)];
                let cells = corners.map(|p| {
                    let u = p.x / (c.x * 0.5);
                    let v = p.y / (c.y * 0.5) - 1.0;
                    vec2((u + v) * 0.5, (v - u) * 0.5)
                });
                let lo = cells.iter().fold(Vec2::MAX, |m, p| m.min(*p));
                let hi = cells.iter().fold(Vec2::MIN, |m, p| m.max(*p));
                (
                    ivec2(lo.x.floor() as i32 - 1, lo.y.floor() as i32 - 1),
                    ivec2(hi.x.ceil() as i32 + 1, hi.y.ceil() as i32 + 1),
                )
            }
            GridKind::HexPointy(_) => {
                let row = |y: f32| ((y - c.y * 0.5) / self.step()).floor() as i32;
                (
                    ivec2((a.x / c.x).floor() as i32 - 1, row(a.y) - 1),
                    ivec2(((a.x + a.w) / c.x).floor() as i32 + 1, row(a.y + a.h) + 1),
                )
            }
            GridKind::HexFlat(_) => {
                let col = |x: f32| ((x - c.x * 0.5) / self.step()).floor() as i32;
                (
                    ivec2(col(a.x) - 1, (a.y / c.y).floor() as i32 - 1),
                    ivec2(col(a.x + a.w) + 1, ((a.y + a.h) / c.y).floor() as i32 + 1),
                )
            }
        };
        let lo = lo.max(IVec2::ZERO);
        let hi = hi.min(ivec2(size.x as i32 - 1, size.y as i32 - 1));
        (lo.x <= hi.x && lo.y <= hi.y).then_some((lo, hi))
    }

    /// The matching neighbour rules for [`autotile::Mapper`].
    pub fn autotile_layout(&self) -> autotile::Layout {
        match self.kind {
            GridKind::Square => autotile::Layout::Square,
            GridKind::Isometric => autotile::Layout::IsoDiamond,
            GridKind::HexPointy(Stagger::Odd) => autotile::Layout::HexRowsOdd,
            GridKind::HexPointy(Stagger::Even) => autotile::Layout::HexRowsEven,
            GridKind::HexFlat(Stagger::Odd) => autotile::Layout::HexColsOdd,
            GridKind::HexFlat(Stagger::Even) => autotile::Layout::HexColsEven,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layouts() -> Vec<GridLayout> {
        vec![
            GridLayout::square(vec2(16.0, 16.0)),
            GridLayout::square(vec2(32.0, 20.0)),
            GridLayout::isometric(vec2(64.0, 32.0)),
            GridLayout::hex_pointy(vec2(52.0, 60.0), Stagger::Odd),
            GridLayout::hex_pointy(vec2(52.0, 60.0), Stagger::Even),
            GridLayout::hex_flat(vec2(60.0, 52.0), Stagger::Odd),
            GridLayout::hex_flat(vec2(60.0, 52.0), Stagger::Even),
            GridLayout::hex_pointy(vec2(40.0, 48.0), Stagger::Odd).with_hex_side(20.0),
        ]
    }

    #[test]
    fn cell_to_world_to_cell_round_trips() {
        for l in layouts() {
            for y in -6..9 {
                for x in -6..9 {
                    let cell = ivec2(x, y);
                    assert_eq!(l.cell_at(l.cell_center(cell)), cell, "{l:?}");
                }
            }
        }
    }

    #[test]
    fn points_near_the_centre_stay_in_the_cell() {
        for l in layouts() {
            // An inscribed radius every kind of cell contains.
            let r = match l.kind {
                GridKind::Isometric => l.cell.y * 0.4,
                GridKind::Square => l.cell.min_element() * 0.45,
                GridKind::HexPointy(_) => l.cell.x * 0.45,
                GridKind::HexFlat(_) => l.cell.y * 0.45,
            };
            for y in -3..5 {
                for x in -3..5 {
                    let cell = ivec2(x, y);
                    let c = l.cell_center(cell);
                    for k in 0..8 {
                        let dir = Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4);
                        assert_eq!(l.cell_at(c + dir * r), cell, "{l:?} {cell} dir {k}");
                    }
                }
            }
        }
    }

    #[test]
    fn staggered_rows_and_columns_shift_as_documented() {
        let odd = GridLayout::hex_pointy(vec2(52.0, 60.0), Stagger::Odd);
        let even = GridLayout::hex_pointy(vec2(52.0, 60.0), Stagger::Even);
        assert_eq!(odd.cell_center(ivec2(0, 0)).x, 26.0);
        assert_eq!(odd.cell_center(ivec2(0, 1)).x, 52.0);
        assert_eq!(even.cell_center(ivec2(0, 0)).x, 52.0);
        assert_eq!(even.cell_center(ivec2(0, 1)).x, 26.0);
        // Rows are 3/4 of the height apart for a regular side.
        assert_eq!(
            odd.cell_center(ivec2(0, 1)).y - odd.cell_center(ivec2(0, 0)).y,
            45.0
        );
        let flat = GridLayout::hex_flat(vec2(60.0, 52.0), Stagger::Odd);
        assert_eq!(flat.cell_center(ivec2(1, 0)).y, 52.0);
        assert_eq!(flat.cell_center(ivec2(0, 0)).y, 26.0);
    }

    #[test]
    fn cells_in_covers_every_cell_that_reaches_the_area() {
        let size = uvec2_(30, 30);
        let areas = [
            Rect::new(100.0, 80.0, 200.0, 150.0),
            Rect::new(-50.0, -50.0, 120.0, 90.0),
            Rect::new(400.0, 300.0, 64.0, 64.0),
        ];
        for l in layouts() {
            for area in areas {
                let reach = vec2(8.0, 12.0);
                let range = l.cells_in(area, reach, size);
                for y in 0..size.y as i32 {
                    for x in 0..size.x as i32 {
                        let cell = ivec2(x, y);
                        let r = Rect::from_center(l.cell_center(cell), l.cell + reach * 2.0);
                        if !r.intersects(&area) {
                            continue;
                        }
                        let (lo, hi) = range.unwrap_or((ivec2(1, 1), ivec2(0, 0)));
                        assert!(
                            cell.x >= lo.x && cell.x <= hi.x && cell.y >= lo.y && cell.y <= hi.y,
                            "{l:?}: cell {cell} reaches {area:?} but range is {range:?}"
                        );
                    }
                }
            }
        }
    }

    fn uvec2_(x: u32, y: u32) -> UVec2 {
        UVec2::new(x, y)
    }

    #[test]
    fn corners_are_closed_outlines() {
        for l in layouts() {
            let c = l.cell_corners(ivec2(2, 3));
            let want = if matches!(l.kind, GridKind::HexPointy(_) | GridKind::HexFlat(_)) {
                6
            } else {
                4
            };
            assert_eq!(c.len(), want);
            let centre = l.cell_center(ivec2(2, 3));
            for p in c {
                assert!((p - centre).length() <= l.cell.length() * 0.5 + 1e-3);
            }
        }
    }
}
