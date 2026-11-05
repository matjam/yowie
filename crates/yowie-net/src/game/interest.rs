//! Grid-based interest management.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use yowie_core::Vec2;

/// A spatial hash of entities, for finding those near a viewer without
/// visiting all of them.
#[derive(Debug, Clone)]
pub struct InterestGrid<ID> {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<ID>>,
    at: HashMap<ID, ((i32, i32), Vec2)>,
}

impl<ID: Copy + Eq + Hash> InterestGrid<ID> {
    /// Creates a grid with square cells `cell_size` units wide. A size near
    /// the typical interest radius works well.
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell: cell_size.max(f32::EPSILON),
            cells: HashMap::new(),
            at: HashMap::new(),
        }
    }

    fn key(&self, p: Vec2) -> (i32, i32) {
        (
            (p.x / self.cell).floor() as i32,
            (p.y / self.cell).floor() as i32,
        )
    }

    /// Inserts an entity or moves it to `pos`.
    pub fn set(&mut self, id: ID, pos: Vec2) {
        let key = self.key(pos);
        if let Some((old, p)) = self.at.get_mut(&id) {
            *p = pos;
            if *old == key {
                return;
            }
            let old = std::mem::replace(old, key);
            self.remove_from_cell(old, id);
        } else {
            self.at.insert(id, (key, pos));
        }
        self.cells.entry(key).or_default().push(id);
    }

    fn remove_from_cell(&mut self, key: (i32, i32), id: ID) {
        if let Some(v) = self.cells.get_mut(&key) {
            v.retain(|e| *e != id);
            if v.is_empty() {
                self.cells.remove(&key);
            }
        }
    }

    /// Removes an entity.
    pub fn remove(&mut self, id: ID) {
        if let Some((key, _)) = self.at.remove(&id) {
            self.remove_from_cell(key, id);
        }
    }

    /// Number of entities in the grid.
    pub fn len(&self) -> usize {
        self.at.len()
    }

    /// True when the grid holds no entities.
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }

    /// Calls `f(id, position)` for every entity within `radius` of `center`.
    pub fn query(&self, center: Vec2, radius: f32, mut f: impl FnMut(ID, Vec2)) {
        let lo = self.key(center - Vec2::splat(radius));
        let hi = self.key(center + Vec2::splat(radius));
        let r2 = radius * radius;
        for cx in lo.0..=hi.0 {
            for cy in lo.1..=hi.1 {
                for id in self.cells.get(&(cx, cy)).into_iter().flatten() {
                    if let Some((_, p)) = self.at.get(id)
                        && p.distance_squared(center) <= r2
                    {
                        f(*id, *p);
                    }
                }
            }
        }
    }
}

/// Tracks which entities one viewer should hear about: those within
/// `radius`, leaving only beyond `radius + margin` so an entity at the edge
/// does not flicker in and out.
///
/// Each update, call [`Interest::begin`], [`Interest::visit`] for every
/// candidate (or use [`Interest::update`] with an [`InterestGrid`]), then
/// [`Interest::end`] for the entities that entered and left.
#[derive(Debug, Clone)]
pub struct Interest<ID> {
    radius: f32,
    margin: f32,
    viewer: Vec2,
    current: HashSet<ID>,
    seen: HashSet<ID>,
}

impl<ID: Copy + Eq + Hash> Interest<ID> {
    /// Creates a viewer set. A `margin` of zero or less means a tenth of
    /// `radius`.
    pub fn new(radius: f32, margin: f32) -> Self {
        Self {
            radius,
            margin: if margin > 0.0 { margin } else { radius / 10.0 },
            viewer: Vec2::ZERO,
            current: HashSet::new(),
            seen: HashSet::new(),
        }
    }

    /// Starts an update with the viewer at `viewer`.
    pub fn begin(&mut self, viewer: Vec2) {
        self.viewer = viewer;
        self.seen.clear();
    }

    /// Considers an entity; returns whether it is in the set this update.
    pub fn visit(&mut self, id: ID, pos: Vec2) -> bool {
        let r = if self.current.contains(&id) {
            self.radius + self.margin
        } else {
            self.radius
        };
        if pos.distance_squared(self.viewer) > r * r {
            return false;
        }
        self.seen.insert(id);
        true
    }

    /// Finishes the update. Returns the entities that entered and left the
    /// set. An entity not visited since `begin` has left.
    pub fn end(&mut self) -> (Vec<ID>, Vec<ID>) {
        let entered = self.seen.difference(&self.current).copied().collect();
        let left = self.current.difference(&self.seen).copied().collect();
        std::mem::swap(&mut self.current, &mut self.seen);
        self.seen.clear();
        (entered, left)
    }

    /// Runs `begin`, a grid query and `end` in one call.
    pub fn update(&mut self, viewer: Vec2, grid: &InterestGrid<ID>) -> (Vec<ID>, Vec<ID>) {
        self.begin(viewer);
        let mut found = Vec::new();
        grid.query(viewer, self.radius + self.margin, |id, p| {
            found.push((id, p))
        });
        for (id, p) in found {
            self.visit(id, p);
        }
        self.end()
    }

    /// Whether `id` was in the set at the last update.
    pub fn contains(&self, id: ID) -> bool {
        self.current.contains(&id)
    }

    /// Number of entities in the set.
    pub fn len(&self) -> usize {
        self.current.len()
    }

    /// True when the set is empty.
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::vec2;

    #[test]
    fn enter_leave_with_hysteresis() {
        let mut grid = InterestGrid::new(10.0);
        grid.set(1u32, vec2(5.0, 0.0));
        grid.set(2, vec2(30.0, 0.0));
        grid.set(3, vec2(-6.0, 6.0));
        let mut v = Interest::new(10.0, 2.0);
        let (mut entered, left) = v.update(Vec2::ZERO, &grid);
        entered.sort_unstable();
        assert_eq!((entered, left), (vec![1, 3], vec![]));
        // Entity 1 drifts to 11: inside radius + margin, so it stays.
        grid.set(1, vec2(11.0, 0.0));
        let (entered, left) = v.update(Vec2::ZERO, &grid);
        assert!(entered.is_empty() && left.is_empty());
        assert!(v.contains(1));
        // At 13 it leaves; a new entity arriving at 8 enters.
        grid.set(1, vec2(13.0, 0.0));
        grid.set(2, vec2(0.0, 8.0));
        let (entered, left) = v.update(Vec2::ZERO, &grid);
        assert_eq!((entered, left), (vec![2], vec![1]));
        grid.remove(2);
        assert_eq!(grid.len(), 2);
        let (_, left) = v.update(Vec2::ZERO, &grid);
        assert_eq!(left, vec![2]);
    }
}
