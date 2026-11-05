use yowie_core::{IVec2, ivec2};

use crate::grid::DIRS4;

/// The cost that forbids a step.
pub const BLOCKED: f32 = f32::INFINITY;

/// Whether and when a path may step diagonally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diagonal {
    /// Orthogonal steps only.
    Never,
    /// Diagonal steps are always allowed, even between two blocked cells.
    Always,
    /// A diagonal step needs both orthogonal cells it passes between to be
    /// enterable, so paths never cut a corner or squeeze between walls.
    IfBothClear,
    /// A diagonal step needs at least one of those cells to be enterable,
    /// so paths cut a corner but not through a solid diagonal wall.
    IfEitherClear,
}

const DIAGONALS: [IVec2; 4] = [ivec2(1, 1), ivec2(-1, 1), ivec2(-1, -1), ivec2(1, -1)];

fn usable(c: f32) -> bool {
    (0.0..BLOCKED).contains(&c)
}

/// Calls `f(next, cost)` for each step from `p` that stays inside a
/// `w` by `h` grid and that `cost(from, to)` allows under `rule`.
pub(crate) fn for_each_step(
    p: IVec2,
    w: usize,
    h: usize,
    rule: Diagonal,
    cost: &mut impl FnMut(IVec2, IVec2) -> f32,
    mut f: impl FnMut(IVec2, f32),
) {
    let inside = |q: IVec2| q.x >= 0 && q.y >= 0 && (q.x as usize) < w && (q.y as usize) < h;
    for d in DIRS4 {
        let n = p + d;
        if inside(n) {
            let c = cost(p, n);
            if usable(c) {
                f(n, c);
            }
        }
    }
    if rule == Diagonal::Never {
        return;
    }
    for d in DIAGONALS {
        let n = p + d;
        if !inside(n) {
            continue;
        }
        if rule != Diagonal::Always {
            let a = p + ivec2(d.x, 0);
            let b = p + ivec2(0, d.y);
            let clear_a = inside(a) && usable(cost(p, a));
            let clear_b = inside(b) && usable(cost(p, b));
            let ok = if rule == Diagonal::IfBothClear {
                clear_a && clear_b
            } else {
                clear_a || clear_b
            };
            if !ok {
                continue;
            }
        }
        let c = cost(p, n);
        if usable(c) {
            f(n, c);
        }
    }
}
