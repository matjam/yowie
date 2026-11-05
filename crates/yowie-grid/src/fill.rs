use yowie_core::IVec2;

use crate::Neighbourhood;

/// Returns the cells connected to `start` through cells for which
/// `passable` is true, in breadth-first order, `start` first. The region
/// is limited to a `w` by `h` grid.
///
/// Returns an empty list when `start` is outside the grid or not passable.
pub fn flood_fill(
    w: usize,
    h: usize,
    start: IVec2,
    hood: Neighbourhood,
    mut passable: impl FnMut(IVec2) -> bool,
) -> Vec<IVec2> {
    let inside = |p: IVec2| p.x >= 0 && p.y >= 0 && (p.x as usize) < w && (p.y as usize) < h;
    if !inside(start) || !passable(start) {
        return Vec::new();
    }
    let index = |p: IVec2| p.y as usize * w + p.x as usize;
    let mut seen = vec![false; w * h];
    seen[index(start)] = true;
    let mut out = vec![start];
    let mut head = 0;
    while let Some(&p) = out.get(head) {
        head += 1;
        for &d in hood.dirs() {
            let n = p + d;
            if inside(n) && !seen[index(n)] {
                seen[index(n)] = true;
                if passable(n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::ivec2;

    const MAP: [&str; 4] = ["..#..", "..#..", "###..", "....."];

    fn open(p: IVec2) -> bool {
        MAP[p.y as usize].as_bytes()[p.x as usize] == b'.'
    }

    #[test]
    fn regions_respect_walls_and_neighbourhood() {
        let cases = [
            (ivec2(0, 0), Neighbourhood::Four, 4),
            (ivec2(0, 0), Neighbourhood::Eight, 4),
            (ivec2(4, 0), Neighbourhood::Four, 11),
            (ivec2(3, 2), Neighbourhood::Eight, 11),
            (ivec2(2, 0), Neighbourhood::Four, 0),
            (ivec2(9, 9), Neighbourhood::Four, 0),
        ];
        for (start, hood, want) in cases {
            let got = flood_fill(5, 4, start, hood, open);
            assert_eq!(got.len(), want, "{start} {hood:?}");
            if want > 0 {
                assert_eq!(got[0], start);
            }
        }
    }

    #[test]
    fn eight_neighbourhood_crosses_diagonal_links() {
        let diagonal_only = |p: IVec2| p.x == p.y;
        let four = flood_fill(3, 3, ivec2(0, 0), Neighbourhood::Four, diagonal_only);
        let eight = flood_fill(3, 3, ivec2(0, 0), Neighbourhood::Eight, diagonal_only);
        assert_eq!((four.len(), eight.len()), (1, 3));
    }

    #[test]
    fn no_cell_repeats() {
        let got = flood_fill(5, 4, ivec2(4, 3), Neighbourhood::Eight, open);
        let mut sorted: Vec<_> = got.iter().map(|p| (p.x, p.y)).collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len());
    }
}
