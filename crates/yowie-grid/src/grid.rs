use yowie_core::{IVec2, ivec2};

/// The four orthogonal steps: right, down, left, up.
pub const DIRS4: [IVec2; 4] = [ivec2(1, 0), ivec2(0, 1), ivec2(-1, 0), ivec2(0, -1)];

/// The eight steps: the four orthogonal ones, then the four diagonals.
pub const DIRS8: [IVec2; 8] = [
    ivec2(1, 0),
    ivec2(0, 1),
    ivec2(-1, 0),
    ivec2(0, -1),
    ivec2(1, 1),
    ivec2(-1, 1),
    ivec2(-1, -1),
    ivec2(1, -1),
];

/// Which cells count as adjacent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Neighbourhood {
    /// Up, down, left and right.
    Four,
    /// The four above plus the diagonals.
    Eight,
}

impl Neighbourhood {
    /// The steps to the adjacent cells.
    pub fn dirs(self) -> &'static [IVec2] {
        match self {
            Neighbourhood::Four => &DIRS4,
            Neighbourhood::Eight => &DIRS8,
        }
    }
}

/// A rectangular grid of cells stored row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid<T> {
    width: usize,
    height: usize,
    cells: Vec<T>,
}

impl<T: Clone> Grid<T> {
    /// Creates a grid with every cell set to `value`.
    pub fn filled(width: usize, height: usize, value: T) -> Self {
        Self {
            width,
            height,
            cells: vec![value; width * height],
        }
    }

    /// Sets every cell to `value`.
    pub fn fill(&mut self, value: T) {
        self.cells.fill(value);
    }

    /// Replaces the connected region of cells equal to the one at `start`
    /// with `value` and returns how many cells changed. Does nothing when
    /// `start` is outside the grid or already equals `value`.
    pub fn paint(&mut self, start: IVec2, value: T, hood: Neighbourhood) -> usize
    where
        T: PartialEq,
    {
        let Some(target) = self.get(start).cloned() else {
            return 0;
        };
        if target == value {
            return 0;
        }
        let region = crate::flood_fill(self.width, self.height, start, hood, |p| {
            self.get(p) == Some(&target)
        });
        for &p in &region {
            self.set(p, value.clone());
        }
        region.len()
    }
}

impl<T: Default + Clone> Grid<T> {
    /// Creates a grid of default values.
    pub fn new(width: usize, height: usize) -> Self {
        Self::filled(width, height, T::default())
    }
}

impl<T> Grid<T> {
    /// Creates a grid by calling `f` for each cell, row by row.
    pub fn from_fn(width: usize, height: usize, mut f: impl FnMut(IVec2) -> T) -> Self {
        let mut cells = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                cells.push(f(ivec2(x as i32, y as i32)));
            }
        }
        Self {
            width,
            height,
            cells,
        }
    }

    /// Width in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Returns true when `p` is inside the grid.
    pub fn contains(&self, p: IVec2) -> bool {
        self.index(p).is_some()
    }

    /// The position of the cell at `index` in [`Grid::cells`].
    pub fn position(&self, index: usize) -> IVec2 {
        let w = self.width.max(1);
        ivec2((index % w) as i32, (index / w) as i32)
    }

    /// The index of `p` in [`Grid::cells`], or `None` outside the grid.
    pub fn index(&self, p: IVec2) -> Option<usize> {
        if p.x < 0 || p.y < 0 || p.x as usize >= self.width || p.y as usize >= self.height {
            return None;
        }
        Some(p.y as usize * self.width + p.x as usize)
    }

    /// Returns the cell at `p`, or `None` outside the grid.
    pub fn get(&self, p: IVec2) -> Option<&T> {
        self.index(p).and_then(|i| self.cells.get(i))
    }

    /// Returns the cell at `p` mutably, or `None` outside the grid.
    pub fn get_mut(&mut self, p: IVec2) -> Option<&mut T> {
        let i = self.index(p)?;
        self.cells.get_mut(i)
    }

    /// Stores `value` at `p`. Returns false, and stores nothing, outside
    /// the grid.
    pub fn set(&mut self, p: IVec2, value: T) -> bool {
        match self.get_mut(p) {
            Some(cell) => {
                *cell = value;
                true
            }
            None => false,
        }
    }

    /// All cells, row by row.
    pub fn cells(&self) -> &[T] {
        &self.cells
    }

    /// All cells mutably, row by row.
    pub fn cells_mut(&mut self) -> &mut [T] {
        &mut self.cells
    }

    /// Iterates over every cell with its position, row by row.
    pub fn iter(&self) -> impl Iterator<Item = (IVec2, &T)> {
        self.cells
            .iter()
            .enumerate()
            .map(|(i, v)| (self.position(i), v))
    }

    /// The in-bounds positions adjacent to `p`.
    pub fn neighbours(&self, p: IVec2, hood: Neighbourhood) -> impl Iterator<Item = IVec2> + '_ {
        hood.dirs()
            .iter()
            .map(move |&d| p + d)
            .filter(|&n| self.contains(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_checked_access() {
        let mut g = Grid::<u8>::new(3, 2);
        assert!(g.set(ivec2(2, 1), 9));
        assert!(!g.set(ivec2(3, 0), 1));
        assert!(!g.set(ivec2(-1, 0), 1));
        assert_eq!(g.get(ivec2(2, 1)), Some(&9));
        assert_eq!(g.get(ivec2(0, 2)), None);
        assert_eq!(g.cells().len(), 6);
        assert_eq!(g.index(ivec2(2, 1)), Some(5));
        assert_eq!(g.position(5), ivec2(2, 1));
        assert!(g.contains(ivec2(0, 0)) && !g.contains(ivec2(0, -1)));
        if let Some(c) = g.get_mut(ivec2(0, 0)) {
            *c = 4;
        }
        assert_eq!(g.iter().next(), Some((ivec2(0, 0), &4)));
        g.fill(1);
        assert!(g.cells().iter().all(|&c| c == 1));
    }

    #[test]
    fn neighbours_stay_in_bounds() {
        let g = Grid::<u8>::new(3, 3);
        let cases = [
            (ivec2(1, 1), Neighbourhood::Four, 4),
            (ivec2(1, 1), Neighbourhood::Eight, 8),
            (ivec2(0, 0), Neighbourhood::Four, 2),
            (ivec2(0, 0), Neighbourhood::Eight, 3),
            (ivec2(2, 1), Neighbourhood::Eight, 5),
            (ivec2(5, 5), Neighbourhood::Four, 0),
        ];
        for (p, hood, want) in cases {
            assert_eq!(g.neighbours(p, hood).count(), want, "{p} {hood:?}");
        }
    }

    #[test]
    fn empty_grid_is_safe() {
        let g = Grid::<u8>::new(0, 0);
        assert!(g.get(ivec2(0, 0)).is_none());
        assert_eq!(g.iter().count(), 0);
        assert_eq!(g.position(0), ivec2(0, 0));
    }

    #[test]
    fn from_fn_and_paint() {
        let mut g = Grid::from_fn(4, 3, |p| if p.x == 2 { 1 } else { 0 });
        assert_eq!(g.get(ivec2(2, 2)), Some(&1));
        // Painting the left region does not cross the wall at x = 2.
        assert_eq!(g.paint(ivec2(0, 0), 5, Neighbourhood::Four), 6);
        assert_eq!(g.get(ivec2(3, 0)), Some(&0));
        assert_eq!(g.paint(ivec2(0, 0), 5, Neighbourhood::Four), 0);
        assert_eq!(g.paint(ivec2(9, 9), 5, Neighbourhood::Four), 0);
    }
}
