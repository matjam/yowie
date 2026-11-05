use std::collections::BinaryHeap;

use yowie_core::IVec2;

use crate::heap::Node;
use crate::step::for_each_step;
use crate::{BLOCKED, Diagonal, Grid, Neighbourhood};

/// Builds a Dijkstra map: for every cell of a `w` by `h` grid, the least
/// total cost of reaching it from the nearest of `sources`. Cells that no
/// source can reach hold [`BLOCKED`]. Sources outside the grid are
/// ignored.
///
/// Use one source for a distance field, or many (all the exits, all the
/// enemies) to make every cell know the way to the nearest. `cost` is as
/// for [`crate::Pathfinder::find`]; the map measures the cost of walking
/// from a source to the cell, so for walking toward a source follow
/// [`downhill`].
pub fn dijkstra(
    w: usize,
    h: usize,
    sources: &[IVec2],
    rule: Diagonal,
    cost: impl FnMut(IVec2, IVec2) -> f32,
) -> Grid<f32> {
    let mut dist = Grid::filled(w, h, BLOCKED);
    let mut seeds = Vec::new();
    for &s in sources {
        if dist.set(s, 0.0) {
            seeds.push((s, 0.0));
        }
    }
    relax(&mut dist, seeds, rule, cost);
    dist
}

/// Builds a flee map from a Dijkstra map. Walking downhill on the result
/// leads away from the sources by a route that avoids dead ends, because
/// the map is re-relaxed after scaling: a fleeing creature prefers a far
/// open space over a close corner.
///
/// `strength` scales the distances before they are negated, as in the
/// classic roguelike technique; 1.2 is the usual value. Cells that were
/// unreachable stay [`BLOCKED`]. `cost` must be the cost function the
/// distance map was built with.
pub fn flee_map(
    dist: &Grid<f32>,
    rule: Diagonal,
    cost: impl FnMut(IVec2, IVec2) -> f32,
    strength: f32,
) -> Grid<f32> {
    let mut flee = dist.clone();
    let mut seeds = Vec::new();
    for (p, v) in dist.iter() {
        if v.is_finite() {
            let scaled = -v * strength;
            flee.set(p, scaled);
            seeds.push((p, scaled));
        }
    }
    relax(&mut flee, seeds, rule, cost);
    flee
}

/// Returns the neighbour of `p` with the lowest value in `map`, if one is
/// lower than `p`'s own. Follow it repeatedly to walk to a source of a
/// Dijkstra map, or away along a flee map.
pub fn downhill(map: &Grid<f32>, p: IVec2, hood: Neighbourhood) -> Option<IVec2> {
    let mut best = *map.get(p)?;
    let mut found = None;
    for n in map.neighbours(p, hood) {
        if let Some(&v) = map.get(n)
            && v < best
        {
            best = v;
            found = Some(n);
        }
    }
    found
}

/// Lowers each cell to the least `seed + path cost` over all seeds.
fn relax(
    dist: &mut Grid<f32>,
    seeds: Vec<(IVec2, f32)>,
    rule: Diagonal,
    mut cost: impl FnMut(IVec2, IVec2) -> f32,
) {
    let (w, h) = (dist.width(), dist.height());
    let mut open = BinaryHeap::new();
    for (p, f) in seeds {
        if let Some(i) = dist.index(p) {
            open.push(Node { f, i: i as u32 });
        }
    }
    let mut next = Vec::new();
    while let Some(cur) = open.pop() {
        let ci = cur.i as usize;
        if cur.f > dist.cells()[ci] {
            continue;
        }
        let cp = dist.position(ci);
        next.clear();
        for_each_step(cp, w, h, rule, &mut cost, |n, c| next.push((n, c)));
        for &(n, c) in &next {
            let Some(ni) = dist.index(n) else { continue };
            let nd = cur.f + c;
            if nd < dist.cells()[ni] {
                dist.cells_mut()[ni] = nd;
                open.push(Node {
                    f: nd,
                    i: ni as u32,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::ivec2;

    fn open(_: IVec2, _: IVec2) -> f32 {
        1.0
    }

    #[test]
    fn single_source_distances() {
        let d = dijkstra(5, 3, &[ivec2(0, 0)], Diagonal::Never, open);
        assert_eq!(d.get(ivec2(4, 2)), Some(&6.0));
        let d8 = dijkstra(5, 3, &[ivec2(0, 0)], Diagonal::Always, open);
        assert_eq!(d8.get(ivec2(4, 2)), Some(&4.0));
    }

    #[test]
    fn multiple_sources_take_the_nearest() {
        let d = dijkstra(9, 1, &[ivec2(0, 0), ivec2(8, 0)], Diagonal::Never, open);
        let row: Vec<f32> = d.cells().to_vec();
        assert_eq!(row, [0.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 0.0]);
        // A source outside the grid is ignored.
        let none = dijkstra(3, 1, &[ivec2(9, 9)], Diagonal::Never, open);
        assert!(none.cells().iter().all(|v| v.is_infinite()));
    }

    #[test]
    fn walls_leave_unreachable_cells_blocked() {
        let cost = |_: IVec2, to: IVec2| if to.x == 2 { BLOCKED } else { 1.0 };
        let d = dijkstra(5, 2, &[ivec2(0, 0)], Diagonal::Always, cost);
        assert_eq!(d.get(ivec2(1, 1)), Some(&1.0));
        assert!(d.get(ivec2(3, 0)).is_some_and(|v| v.is_infinite()));
    }

    #[test]
    fn downhill_walks_to_the_source() {
        let d = dijkstra(6, 6, &[ivec2(5, 5)], Diagonal::Never, open);
        let mut p = ivec2(0, 0);
        let mut steps = 0;
        while let Some(n) = downhill(&d, p, Neighbourhood::Four) {
            p = n;
            steps += 1;
        }
        assert_eq!((p, steps), (ivec2(5, 5), 10));
        assert_eq!(downhill(&d, ivec2(9, 9), Neighbourhood::Four), None);
    }

    #[test]
    fn flee_map_runs_to_the_far_end() {
        // A corridor with the threat at x = 3; fleeing from x = 4 should
        // run right to x = 9, and fleeing from x = 2 left to x = 0.
        let d = dijkstra(10, 1, &[ivec2(3, 0)], Diagonal::Never, open);
        let flee = flee_map(&d, Diagonal::Never, open, 1.2);
        for (start, end) in [(4, 9), (2, 0)] {
            let mut p = ivec2(start, 0);
            while let Some(n) = downhill(&flee, p, Neighbourhood::Four) {
                p = n;
            }
            assert_eq!(p, ivec2(end, 0), "from {start}");
        }
    }

    #[test]
    fn flee_map_prefers_open_space_over_dead_ends() {
        // From the threat at the junction (2,1): left is a dead end of
        // length 1, right is a long corridor.
        let cost = |_: IVec2, to: IVec2| if to.y != 1 { BLOCKED } else { 1.0 };
        let d = dijkstra(12, 3, &[ivec2(2, 1)], Diagonal::Never, cost);
        let flee = flee_map(&d, Diagonal::Never, cost, 1.2);
        let mut p = ivec2(3, 1);
        while let Some(n) = downhill(&flee, p, Neighbourhood::Four) {
            p = n;
        }
        assert_eq!(p, ivec2(11, 1));
    }
}
