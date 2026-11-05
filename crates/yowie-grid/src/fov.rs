use yowie_core::{IVec2, ivec2};

/// A slope `n / d` with `d > 0`, so comparisons stay in integers.
#[derive(Clone, Copy)]
struct Slope {
    n: i32,
    d: i32,
}

struct Caster<O, V> {
    origin: IVec2,
    radius: i32,
    side: i32,
    seen: Vec<bool>,
    opaque: O,
    visit: V,
}

/// Quadrant transforms: north, east, south, west.
fn to_world(quadrant: u8, origin: IVec2, col: i32, depth: i32) -> IVec2 {
    match quadrant {
        0 => ivec2(origin.x + col, origin.y - depth),
        1 => ivec2(origin.x + depth, origin.y + col),
        2 => ivec2(origin.x + col, origin.y + depth),
        _ => ivec2(origin.x - depth, origin.y + col),
    }
}

/// `floor(depth * s + 1/2)`.
fn round_ties_up(depth: i32, s: Slope) -> i32 {
    (2 * depth * s.n + s.d).div_euclid(2 * s.d)
}

/// `ceil(depth * s - 1/2)`.
fn round_ties_down(depth: i32, s: Slope) -> i32 {
    -(s.d - 2 * depth * s.n).div_euclid(2 * s.d)
}

impl<O: FnMut(IVec2) -> bool, V: FnMut(IVec2)> Caster<O, V> {
    fn reveal(&mut self, p: IVec2) {
        let off = p - self.origin + IVec2::splat(self.radius);
        let i = (off.y * self.side + off.x) as usize;
        if let Some(seen) = self.seen.get_mut(i)
            && !*seen
        {
            *seen = true;
            (self.visit)(p);
        }
    }

    fn scan(&mut self, quadrant: u8, depth: i32, mut start: Slope, end: Slope) {
        let mut prev_wall: Option<bool> = None;
        let (min_col, max_col) = (round_ties_up(depth, start), round_ties_down(depth, end));
        for col in min_col..=max_col {
            let p = to_world(quadrant, self.origin, col, depth);
            let wall = (self.opaque)(p);
            // A floor cell is only seen when its centre lies inside the
            // slopes; this is what makes visibility symmetric.
            let centred = col * start.d >= depth * start.n && col * end.d <= depth * end.n;
            if (wall || centred) && col * col + depth * depth <= self.radius * self.radius {
                self.reveal(p);
            }
            let edge = Slope {
                n: 2 * col - 1,
                d: 2 * depth,
            };
            if prev_wall == Some(true) && !wall {
                start = edge;
            }
            if prev_wall == Some(false) && wall && depth < self.radius {
                self.scan(quadrant, depth + 1, start, edge);
            }
            prev_wall = Some(wall);
        }
        if prev_wall == Some(false) && depth < self.radius {
            self.scan(quadrant, depth + 1, start, end);
        }
    }
}

/// Calls `visit` once for each cell visible from `origin` within `radius`
/// cells, using symmetric shadowcasting.
///
/// `opaque(p)` says whether the cell at `p` blocks sight; an opaque cell
/// can itself be seen. It is called for cells outside any grid too, so
/// return `true` there. Visibility between two floor cells is symmetric:
/// if A sees B then B sees A. The origin is always visited. Cells are
/// within a circle of the given radius; a radius below 0 counts as 0.
pub fn fov(
    origin: IVec2,
    radius: i32,
    opaque: impl FnMut(IVec2) -> bool,
    visit: impl FnMut(IVec2),
) {
    let radius = radius.max(0);
    let side = 2 * radius + 1;
    let mut caster = Caster {
        origin,
        radius,
        side,
        seen: vec![false; (side * side) as usize],
        opaque,
        visit,
    };
    caster.reveal(origin);
    let full = Slope { n: 1, d: 1 };
    let neg = Slope { n: -1, d: 1 };
    for quadrant in 0..4 {
        caster.scan(quadrant, 1, neg, full);
    }
}

/// Like [`fov`], and returns the visible cells, origin first.
pub fn fov_cells(origin: IVec2, radius: i32, opaque: impl FnMut(IVec2) -> bool) -> Vec<IVec2> {
    let mut cells = Vec::new();
    fov(origin, radius, opaque, |p| cells.push(p));
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn map_opaque(rows: &'static [&'static str]) -> impl Fn(IVec2) -> bool {
        move |p| {
            rows.get(p.y as usize)
                .filter(|_| p.y >= 0 && p.x >= 0)
                .and_then(|r| r.as_bytes().get(p.x as usize))
                .is_none_or(|&c| c == b'#')
        }
    }

    #[test]
    fn open_field_is_a_disc() {
        let cells = fov_cells(ivec2(0, 0), 4, |_| false);
        let set: HashSet<_> = cells.iter().copied().collect();
        assert_eq!(set.len(), cells.len(), "cells repeat");
        for y in -6..=6 {
            for x in -6..=6 {
                assert_eq!(set.contains(&ivec2(x, y)), x * x + y * y <= 16, "({x},{y})");
            }
        }
        assert_eq!(cells[0], ivec2(0, 0));
    }

    #[test]
    fn walls_cast_shadows() {
        static ROWS: [&str; 5] = [
            "###########",
            "#.........#",
            "#....#....#",
            "#.........#",
            "###########",
        ];
        let seen: HashSet<_> = fov_cells(ivec2(2, 2), 20, map_opaque(&ROWS))
            .into_iter()
            .collect();
        // The wall itself is lit; the cell straight behind it is not.
        assert!(seen.contains(&ivec2(5, 2)));
        assert!(!seen.contains(&ivec2(6, 2)));
        assert!(!seen.contains(&ivec2(7, 2)));
        assert!(seen.contains(&ivec2(4, 2)));
        assert!(seen.contains(&ivec2(4, 3))); // Nothing outside the room is reported except its walls.
        assert!(
            seen.iter()
                .all(|p| (0..11).contains(&p.x) && (0..5).contains(&p.y))
        );
    }

    #[test]
    fn radius_zero_sees_only_the_origin() {
        assert_eq!(fov_cells(ivec2(3, 3), 0, |_| false), [ivec2(3, 3)]);
        assert_eq!(fov_cells(ivec2(3, 3), -4, |_| false), [ivec2(3, 3)]);
    }

    #[test]
    fn floor_visibility_is_symmetric() {
        let mut seed = 99u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        for _ in 0..20 {
            let (w, h) = (14, 14);
            let wall: Vec<bool> = (0..w * h).map(|_| next() % 4 == 0).collect();
            let opaque = |p: IVec2| {
                p.x < 0
                    || p.y < 0
                    || p.x >= w as i32
                    || p.y >= h as i32
                    || wall[p.y as usize * w + p.x as usize]
            };
            let floors: Vec<IVec2> = (0..w * h)
                .filter(|&i| !wall[i])
                .map(|i| ivec2((i % w) as i32, (i / w) as i32))
                .collect();
            let sees: Vec<HashSet<IVec2>> = floors
                .iter()
                .map(|&f| fov_cells(f, 30, opaque).into_iter().collect())
                .collect();
            for (i, &a) in floors.iter().enumerate() {
                for (j, &b) in floors.iter().enumerate() {
                    assert_eq!(sees[i].contains(&b), sees[j].contains(&a), "{a} {b}");
                }
            }
        }
    }
}
