use std::collections::BinaryHeap;

use yowie_core::{IVec2, ivec2};

use crate::Diagonal;
use crate::heap::Node;
use crate::step::for_each_step;

const NONE: u32 = u32::MAX;

/// A path found by A*.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// Cells from the start to the goal, both included.
    pub points: Vec<IVec2>,
    /// The total cost of the steps, not counting the start cell.
    pub cost: f32,
}

/// An A* searcher that keeps its working memory between searches. Reuse
/// one for many searches on grids of the same size to avoid allocating.
#[derive(Debug, Default)]
pub struct Pathfinder {
    w: usize,
    h: usize,
    min_cost: f32,
    stamp: u32,
    seen: Vec<u32>,
    shut: Vec<u32>,
    g: Vec<f32>,
    came: Vec<u32>,
    open: BinaryHeap<Node>,
}

impl Pathfinder {
    /// Creates a searcher for a `w` by `h` grid.
    pub fn new(w: usize, h: usize) -> Self {
        let mut p = Self::default();
        p.resize(w, h);
        p
    }

    /// Sets the lowest cost any step can have. With a positive value the
    /// search is guided toward the goal; it must not exceed the true
    /// lowest step cost or paths may be longer than necessary. The default
    /// of 0 makes the search a Dijkstra search, always optimal.
    pub fn with_min_cost(mut self, min_cost: f32) -> Self {
        self.min_cost = if min_cost.is_finite() && min_cost > 0.0 {
            min_cost
        } else {
            0.0
        };
        self
    }

    /// Changes the grid size the searcher works on.
    pub fn resize(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        let n = w * h;
        if self.seen.len() < n {
            self.seen = vec![0; n];
            self.shut = vec![0; n];
            self.g = vec![0.0; n];
            self.came = vec![NONE; n];
            self.stamp = 0;
        }
    }

    fn begin(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.seen.fill(0);
            self.shut.fill(0);
            self.stamp = 1;
        }
        self.open.clear();
    }

    fn index(&self, p: IVec2) -> Option<usize> {
        if p.x < 0 || p.y < 0 || p.x as usize >= self.w || p.y as usize >= self.h {
            return None;
        }
        Some(p.y as usize * self.w + p.x as usize)
    }

    fn point(&self, i: usize) -> IVec2 {
        ivec2((i % self.w) as i32, (i / self.w) as i32)
    }

    fn heuristic(&self, from: IVec2, goal: IVec2, rule: Diagonal) -> f32 {
        if self.min_cost == 0.0 {
            return 0.0;
        }
        let d = (from - goal).abs();
        let steps = if rule == Diagonal::Never {
            d.x + d.y
        } else {
            d.x.max(d.y)
        };
        steps as f32 * self.min_cost
    }

    /// Finds the least-cost path from `start` to `goal`.
    ///
    /// `cost(from, to)` is the price of stepping from `from` onto the
    /// adjacent cell `to`; return [`BLOCKED`](crate::BLOCKED) to forbid the step. Returns
    /// `None` when either end is outside the grid or no path exists.
    pub fn find(
        &mut self,
        start: IVec2,
        goal: IVec2,
        rule: Diagonal,
        mut cost: impl FnMut(IVec2, IVec2) -> f32,
    ) -> Option<Path> {
        let (si, gi) = (self.index(start)?, self.index(goal)?);
        if si == gi {
            return Some(Path {
                points: vec![start],
                cost: 0.0,
            });
        }
        self.begin();
        let stamp = self.stamp;
        self.g[si] = 0.0;
        self.came[si] = NONE;
        self.seen[si] = stamp;
        self.open.push(Node {
            f: self.heuristic(start, goal, rule),
            i: si as u32,
        });
        while let Some(cur) = self.open.pop() {
            let ci = cur.i as usize;
            if self.shut[ci] == stamp {
                continue;
            }
            if ci == gi {
                return Some(self.trace(ci));
            }
            self.shut[ci] = stamp;
            let cp = self.point(ci);
            let gc = self.g[ci];
            let mut next = Vec::new();
            for_each_step(cp, self.w, self.h, rule, &mut cost, |n, c| {
                next.push((n, c))
            });
            for (n, c) in next {
                let Some(ni) = self.index(n) else { continue };
                if self.shut[ni] == stamp {
                    continue;
                }
                let ng = gc + c;
                if self.seen[ni] != stamp || ng < self.g[ni] {
                    self.seen[ni] = stamp;
                    self.g[ni] = ng;
                    self.came[ni] = ci as u32;
                    let f = ng + self.heuristic(n, goal, rule);
                    self.open.push(Node { f, i: ni as u32 });
                }
            }
        }
        None
    }

    fn trace(&self, goal: usize) -> Path {
        let mut points = Vec::new();
        let mut i = goal as u32;
        while i != NONE {
            points.push(self.point(i as usize));
            i = self.came[i as usize];
        }
        points.reverse();
        Path {
            points,
            cost: self.g[goal],
        }
    }
}

/// Finds the least-cost path on a `w` by `h` grid with a one-off
/// [`Pathfinder`]. See [`Pathfinder::find`]. Use a [`Pathfinder`] directly
/// to search repeatedly without allocating.
pub fn astar(
    w: usize,
    h: usize,
    start: IVec2,
    goal: IVec2,
    rule: Diagonal,
    cost: impl FnMut(IVec2, IVec2) -> f32,
) -> Option<Path> {
    Pathfinder::new(w, h).find(start, goal, rule, cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLOCKED, dijkstra};

    fn walls(rows: &[&str]) -> (usize, usize, Vec<Vec<bool>>) {
        let grid: Vec<Vec<bool>> = rows
            .iter()
            .map(|r| r.chars().map(|c| c == '#').collect())
            .collect();
        (grid[0].len(), grid.len(), grid)
    }

    fn cost_of(grid: &[Vec<bool>]) -> impl Fn(IVec2, IVec2) -> f32 + '_ {
        move |_, to| {
            if grid[to.y as usize][to.x as usize] {
                BLOCKED
            } else {
                1.0
            }
        }
    }

    #[test]
    fn routes_around_walls() {
        let (w, h, g) = walls(&["....#....", "....#....", "....#....", "........."]);
        let p = astar(w, h, ivec2(0, 0), ivec2(8, 0), Diagonal::Never, cost_of(&g)).unwrap();
        assert_eq!(p.cost, 14.0);
        assert_eq!(p.points.len(), 15);
        assert_eq!(p.points.first(), Some(&ivec2(0, 0)));
        assert_eq!(p.points.last(), Some(&ivec2(8, 0)));
        let d = astar(
            w,
            h,
            ivec2(0, 0),
            ivec2(8, 0),
            Diagonal::Always,
            cost_of(&g),
        )
        .unwrap();
        assert_eq!(d.cost, 8.0);
    }

    #[test]
    fn unreachable_and_out_of_bounds() {
        let (w, h, g) = walls(&["..#..", "..#..", "..#.."]);
        let blocked = astar(
            w,
            h,
            ivec2(0, 0),
            ivec2(4, 0),
            Diagonal::Always,
            cost_of(&g),
        );
        assert!(blocked.is_none());
        assert!(astar(w, h, ivec2(0, 0), ivec2(9, 9), Diagonal::Never, cost_of(&g)).is_none());
        assert!(
            astar(
                w,
                h,
                ivec2(-1, 0),
                ivec2(1, 1),
                Diagonal::Never,
                cost_of(&g)
            )
            .is_none()
        );
        let same = astar(w, h, ivec2(1, 1), ivec2(1, 1), Diagonal::Never, cost_of(&g)).unwrap();
        assert_eq!((same.points, same.cost), (vec![ivec2(1, 1)], 0.0));
    }

    #[test]
    fn diagonal_rules_control_corner_cutting() {
        // The only diagonal route passes between two walls touching at a corner.
        let (w, h, g) = walls(&[".#", "#."]);
        let cases = [
            (Diagonal::Never, false),
            (Diagonal::Always, true),
            (Diagonal::IfBothClear, false),
            (Diagonal::IfEitherClear, false),
        ];
        for (rule, reachable) in cases {
            let p = astar(w, h, ivec2(0, 0), ivec2(1, 1), rule, cost_of(&g));
            assert_eq!(p.is_some(), reachable, "{rule:?}");
        }
        // One open side: cutting a corner needs IfEitherClear or Always.
        let (w, h, g) = walls(&["..", "#."]);
        let cases = [
            (Diagonal::IfBothClear, 2.0),
            (Diagonal::IfEitherClear, 1.0),
            (Diagonal::Always, 1.0),
        ];
        for (rule, want) in cases {
            let p = astar(w, h, ivec2(0, 0), ivec2(1, 1), rule, cost_of(&g)).unwrap();
            assert_eq!(p.cost, want, "{rule:?}");
        }
    }

    #[test]
    fn custom_costs_prefer_cheap_cells() {
        // A swamp row costs 10; going around costs less.
        let cost = |_: IVec2, to: IVec2| if to.y == 1 && to.x < 4 { 10.0 } else { 1.0 };
        let p = astar(5, 3, ivec2(0, 0), ivec2(0, 2), Diagonal::Never, cost).unwrap();
        // Around via x = 4 costs 10; straight down costs 11.
        assert_eq!(p.cost, 10.0);
        assert!(p.points.iter().any(|q| q.x == 4));
    }

    #[test]
    fn guided_search_matches_dijkstra_on_random_maps() {
        let mut seed = 12345u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        for _ in 0..30 {
            let (w, h) = (12, 9);
            let blocked: Vec<bool> = (0..w * h).map(|_| next() % 4 == 0).collect();
            let cost = |_: IVec2, to: IVec2| {
                if blocked[to.y as usize * w + to.x as usize] {
                    BLOCKED
                } else {
                    1.0
                }
            };
            for rule in [Diagonal::Never, Diagonal::IfBothClear, Diagonal::Always] {
                let start = ivec2(0, 0);
                let map = dijkstra(w, h, &[start], rule, cost);
                let mut finder = Pathfinder::new(w, h).with_min_cost(1.0);
                for gy in 0..h as i32 {
                    for gx in 0..w as i32 {
                        let goal = ivec2(gx, gy);
                        let got = finder.find(start, goal, rule, cost);
                        let want = *map.get(goal).unwrap_or(&BLOCKED);
                        match got {
                            Some(p) => assert_eq!(p.cost, want, "{rule:?} {goal}"),
                            None => assert!(want.is_infinite(), "{rule:?} {goal}"),
                        }
                    }
                }
            }
        }
    }
}
