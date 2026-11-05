use std::collections::HashMap;

use yowie_core::{IVec2, ivec2};

use super::*;

fn identity16() -> [Option<u32>; 16] {
    std::array::from_fn(|i| Some(i as u32))
}

fn collect(
    m: &Mapper,
    w: i32,
    h: i32,
    terrain: impl Fn(i32, i32) -> i32,
) -> HashMap<(i32, i32), Option<u32>> {
    let mut out = HashMap::new();
    m.apply(w, h, terrain, |x, y, f| {
        out.insert((x, y), f);
    });
    out
}

/// A plus shape in a 3x3 map.
fn plus(x: i32, y: i32) -> i32 {
    i32::from(x == 1 || y == 1)
}

#[test]
fn edge16_masks() {
    let m = Mapper::new(Rules::edge16(1, identity16())).outside(0);
    let got = collect(&m, 3, 3, plus);
    assert_eq!(got[&(1, 1)], Some(1 | 2 | 4 | 8));
    for (cell, want) in [((1, 0), 4), ((1, 2), 1), ((0, 1), 2), ((2, 1), 8)] {
        assert_eq!(got[&cell], Some(want), "{cell:?}");
    }
    for cell in [(0, 0), (2, 0), (0, 2), (2, 2)] {
        assert_eq!(got[&cell], None);
    }
    // Without a fixed outside the border continues the terrain.
    let m = Mapper::new(Rules::edge16(1, identity16()));
    assert!(collect(&m, 2, 2, |_, _| 1).values().all(|f| *f == Some(15)));
}

#[test]
fn corner16_emits_one_extra_row_and_column() {
    let m = Mapper::new(Rules::corner16(1, identity16())).outside(0);
    let got = collect(&m, 2, 2, |x, y| i32::from(x == 0 && y == 0));
    assert_eq!(got.len(), 9);
    for (cell, want) in [
        ((0, 0), Some(8)),
        ((1, 0), Some(4)),
        ((0, 1), Some(2)),
        ((1, 1), Some(1)),
        ((2, 0), None),
        ((2, 2), None),
    ] {
        assert_eq!(got[&cell], want, "{cell:?}");
    }
    let (size, frames) = m.apply_grid(yowie_core::uvec2(2, 2), &[1, 0, 0, 0]);
    assert_eq!((size.x, size.y, frames.len()), (3, 3, 9));
}

#[test]
fn blob47_frames() {
    let frames = std::array::from_fn(|i| Some(i as u32));
    let m = Mapper::new(Rules::blob47(1, frames)).outside(0);
    let got = collect(&m, 3, 3, |_, _| 1);
    assert_eq!(got[&(1, 1)], Some(46));
    let corner = (1 << Dir::E.index()) | (1 << Dir::S.index()) | (1 << Dir::SE.index());
    assert_eq!(got[&(0, 0)], Some(blob_index(corner) as u32));
}

#[test]
fn cell_and_region_match_apply() {
    let frames = std::array::from_fn(|i| Some(i as u32));
    let m = Mapper::new(Rules::blob47(1, frames));
    let (w, h) = (8, 8);
    let mut terrain = vec![0i32; (w * h) as usize];
    for i in 0..terrain.len() {
        terrain[i] = i32::from((i * 7 + i / 3) % 3 != 0);
    }
    let read = |t: &Vec<i32>| {
        let t = t.clone();
        move |x: i32, y: i32| t[(y * w + x) as usize]
    };
    let mut frames_now = collect(&m, w, h, read(&terrain));
    // Edit one cell and update only what the change reaches.
    terrain[(3 * w + 3) as usize] ^= 1;
    m.cell(3, 3, w, h, read(&terrain), |x, y, f| {
        frames_now.insert((x, y), f);
    });
    assert_eq!(frames_now, collect(&m, w, h, read(&terrain)));
    // Edit a block, then update the rectangle once per affected cell.
    for y in 4..6 {
        for x in 1..4 {
            terrain[(y * w + x) as usize] ^= 1;
        }
    }
    let mut visits = 0;
    m.region(1, 4, 3, 2, w, h, read(&terrain), |x, y, f| {
        visits += 1;
        frames_now.insert((x, y), f);
    });
    assert_eq!(frames_now, collect(&m, w, h, read(&terrain)));
    assert_eq!(visits, 5 * 4, "each affected cell is computed once");
}

#[test]
fn weighted_variants_are_stable_seeded_and_proportional() {
    let rules = Rules::edge16(1, identity16()).variant(15, 100, 3.0);
    let m = Mapper::new(rules);
    let a = collect(&m, 64, 64, |_, _| 1);
    let b = collect(&m, 64, 64, |_, _| 1);
    assert_eq!(a, b, "reapplying is stable");
    let n100 = a.values().filter(|f| **f == Some(100)).count();
    let share = n100 as f64 / a.len() as f64;
    assert!(
        (0.70..0.80).contains(&share),
        "weight 3 against 1 gave {share}"
    );
    let seeded = collect(&Mapper::new(m.rules().clone()).seed(1), 64, 64, |_, _| 1);
    assert_ne!(a, seeded, "a new seed changes the choice");
}

fn wang_corner_set() -> Vec<WangTile> {
    (0..16u32)
        .map(|m| {
            let mut colors = [0; 8];
            for (i, d) in [Dir::NW, Dir::NE, Dir::SW, Dir::SE].into_iter().enumerate() {
                colors[d.index()] = if m & (1 << i) != 0 { 2 } else { 1 };
            }
            WangTile::new(m, colors)
        })
        .collect()
}

#[test]
fn wang_corners_let_the_higher_colour_win() {
    let m = Mapper::new(Rules::wang(WangType::Corners, wang_corner_set())).outside(1);
    let got = collect(&m, 3, 3, |x, y| if (x, y) == (1, 1) { 2 } else { 1 });
    assert_eq!(got[&(1, 1)], Some(15));
    assert_eq!(got[&(0, 0)], Some(8), "its south-east corner touches the 2");
    assert_eq!(got[&(2, 1)], Some(1 | 4), "north-west and south-west touch");
    let empty = Mapper::new(Rules::wang(WangType::Corners, wang_corner_set())).outside(0);
    assert_eq!(collect(&empty, 1, 1, |_, _| 0)[&(0, 0)], None);
}

#[test]
fn wang_falls_back_to_the_closest_tile() {
    // Only the all-1 and all-2 tiles exist.
    let tiles = vec![WangTile::new(0, [1; 8]), WangTile::new(1, [2; 8])];
    let m = Mapper::new(Rules::wang(WangType::Full, tiles)).outside(1);
    let got = collect(&m, 3, 3, |x, y| if (x, y) == (1, 1) { 2 } else { 1 });
    assert_eq!(got[&(1, 1)], Some(1));
    // A neighbour of the 2 is a mix; two corner and one edge position
    // touch the 2, the rest are 1, so tile 0 matches more positions.
    assert_eq!(got[&(2, 1)], Some(0));
}

#[test]
fn layouts_neighbours_are_mutual() {
    let layouts = [
        Layout::Square,
        Layout::HexRowsOdd,
        Layout::HexRowsEven,
        Layout::HexColsOdd,
        Layout::HexColsEven,
        Layout::HexAxial,
        Layout::IsoDiamond,
    ];
    for l in layouts {
        for y in -2..3 {
            for x in -2..3 {
                let c = ivec2(x, y);
                for &d in l.dirs() {
                    let n = l.neighbour(c, d);
                    assert_ne!(n, c, "{l:?} {d:?}");
                    let back = l.dirs().iter().any(|&e| l.neighbour(n, e) == c);
                    assert!(back, "{l:?}: {c} -> {n} has no way back");
                }
            }
        }
    }
}

#[test]
fn edge64_on_a_hex_ring() {
    let frames = std::array::from_fn(|i| Some(i as u32));
    let layout = Layout::HexRowsOdd;
    let m = Mapper::new(Rules::edge64(1, frames))
        .layout(layout)
        .outside(0);
    let mut ring = vec![ivec2(2, 2)];
    ring.extend(
        layout
            .dirs()
            .iter()
            .map(|&d| layout.neighbour(ivec2(2, 2), d)),
    );
    let in_ring = |x: i32, y: i32| i32::from(ring.contains(&IVec2::new(x, y)));
    let got = collect(&m, 5, 5, in_ring);
    assert_eq!(got[&(2, 2)], Some(63));
    // A lone hexagon connects to nothing; one eastern neighbour sets the
    // east bit alone.
    let lone = collect(&m, 5, 5, |x, y| i32::from((x, y) == (2, 2)));
    assert_eq!(lone[&(2, 2)], Some(0));
    let east = layout.dirs().iter().position(|&d| d == Dir::E).unwrap_or(0);
    let pair = collect(&m, 5, 5, |x, y| i32::from(y == 2 && (x == 2 || x == 3)));
    assert_eq!(pair[&(2, 2)], Some(1 << east));
    // Edge64 under a square layout has masks its frames do not cover.
    let square = Mapper::new(Rules::edge64(1, std::array::from_fn(|i| Some(i as u32)))).outside(0);
    assert_eq!(collect(&square, 5, 5, in_ring)[&(2, 2)], None);
}

#[test]
fn iso_diamond_north_is_up_and_left() {
    assert_eq!(
        Layout::IsoDiamond.neighbour(ivec2(3, 3), Dir::N),
        ivec2(2, 2)
    );
    assert_eq!(
        Layout::IsoDiamond.neighbour(ivec2(3, 3), Dir::E),
        ivec2(4, 2)
    );
    // An iso edge16 map joins the same cells a square one does after the
    // quarter turn: the screen-north neighbour is bit 1.
    let m = Mapper::new(Rules::edge16(1, identity16()))
        .layout(Layout::IsoDiamond)
        .outside(0);
    let got = collect(&m, 3, 3, |x, y| {
        i32::from((x, y) == (1, 1) || (x, y) == (0, 0))
    });
    assert_eq!(got[&(1, 1)], Some(1), "(0,0) lies screen-north of (1,1)");
    assert_eq!(got[&(0, 0)], Some(4), "(1,1) lies screen-south of (0,0)");
}
