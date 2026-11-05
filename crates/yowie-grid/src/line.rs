use yowie_core::{IVec2, ivec2};

/// Returns the cells on the Bresenham line from `a` to `b`, both ends
/// included. The line has one cell per step along the longer axis.
pub fn line(a: IVec2, b: IVec2) -> Vec<IVec2> {
    let dx = (b.x - a.x).abs();
    let dy = -(b.y - a.y).abs();
    let sx = if a.x < b.x { 1 } else { -1 };
    let sy = if a.y < b.y { 1 } else { -1 };
    let mut err = dx + dy;
    let mut p = a;
    let mut out = vec![p];
    while p != b {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            p.x += sx;
        }
        if e2 <= dx {
            err += dx;
            p.y += sy;
        }
        out.push(p);
    }
    out
}

/// Returns every cell the straight segment from the centre of `a` to the
/// centre of `b` touches, both ends included, in order from `a`.
///
/// Unlike [`line`], a segment that crosses a corner between four cells
/// includes both cells beside the corner as well as the diagonal one, so
/// it is the right test for "can a projectile or a sight line pass".
pub fn supercover_line(a: IVec2, b: IVec2) -> Vec<IVec2> {
    let d = b - a;
    let (nx, ny) = (d.x.abs(), d.y.abs());
    let sign = ivec2(d.x.signum(), d.y.signum());
    let mut p = a;
    let mut out = vec![p];
    let (mut ix, mut iy) = (0, 0);
    while ix < nx || iy < ny {
        // Compare how far along the line the next x and y cell edges lie.
        let decision =
            (1 + 2 * i64::from(ix)) * i64::from(ny) - (1 + 2 * i64::from(iy)) * i64::from(nx);
        match decision.cmp(&0) {
            std::cmp::Ordering::Equal => {
                out.push(ivec2(p.x + sign.x, p.y));
                out.push(ivec2(p.x, p.y + sign.y));
                p += sign;
                ix += 1;
                iy += 1;
            }
            std::cmp::Ordering::Less => {
                p.x += sign.x;
                ix += 1;
            }
            std::cmp::Ordering::Greater => {
                p.y += sign.y;
                iy += 1;
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(v: &[(i32, i32)]) -> Vec<IVec2> {
        v.iter().map(|&(x, y)| ivec2(x, y)).collect()
    }

    #[test]
    fn bresenham_known_lines() {
        let cases = [
            ((0, 0), (0, 0), vec![(0, 0)]),
            ((0, 0), (3, 0), vec![(0, 0), (1, 0), (2, 0), (3, 0)]),
            ((0, 0), (0, -2), vec![(0, 0), (0, -1), (0, -2)]),
            ((0, 0), (3, 3), vec![(0, 0), (1, 1), (2, 2), (3, 3)]),
            ((0, 0), (4, 2), vec![(0, 0), (1, 1), (2, 1), (3, 2), (4, 2)]),
        ];
        for (a, b, want) in cases {
            assert_eq!(
                line(ivec2(a.0, a.1), ivec2(b.0, b.1)),
                pts(&want),
                "{a:?}->{b:?}"
            );
        }
    }

    #[test]
    fn bresenham_has_one_cell_per_step_and_connects() {
        let ends = [(7, -3), (-5, 9), (2, 2), (-6, -1), (0, 11)];
        for (x, y) in ends {
            let b = ivec2(x, y);
            let l = line(ivec2(0, 0), b);
            assert_eq!(l.len() as i32, x.abs().max(y.abs()) + 1);
            assert_eq!(l.last(), Some(&b));
            for w in l.windows(2) {
                assert_eq!((w[1] - w[0]).abs().max_element(), 1);
            }
        }
    }

    #[test]
    fn supercover_includes_every_touched_cell() {
        let cases = [
            ((0, 0), (0, 0), vec![(0, 0)]),
            ((0, 0), (3, 0), vec![(0, 0), (1, 0), (2, 0), (3, 0)]),
            // A shallow line touches cells above and below the Bresenham ones.
            (
                (0, 0),
                (4, 1),
                vec![(0, 0), (1, 0), (2, 0), (2, 1), (3, 1), (4, 1)],
            ),
            // Through the corner at (0.5, 0.5): both side cells are included.
            ((0, 0), (1, 1), vec![(0, 0), (1, 0), (0, 1), (1, 1)]),
            (
                (2, 2),
                (0, 0),
                vec![(2, 2), (1, 2), (2, 1), (1, 1), (0, 1), (1, 0), (0, 0)],
            ),
        ];
        for (a, b, want) in cases {
            let got = supercover_line(ivec2(a.0, a.1), ivec2(b.0, b.1));
            assert_eq!(got, pts(&want), "{a:?}->{b:?}");
        }
    }

    #[test]
    fn supercover_contains_bresenham() {
        let ends = [(9, 4), (-7, 3), (5, -8), (6, 6)];
        for (x, y) in ends {
            let b = ivec2(x, y);
            let s = supercover_line(ivec2(0, 0), b);
            for p in line(ivec2(0, 0), b) {
                assert!(s.contains(&p), "{p} missing for {b}");
            }
        }
    }
}
