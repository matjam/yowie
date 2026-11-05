//! Applying rules to a terrain grid.

use yowie_core::{IVec2, UVec2, ivec2};

use super::blob::blob_index;
use super::layout::{Dir, Layout};
use super::rules::{Kind, Rules, Weighted};
use super::wang::{WangIndex, closest, positions};

/// Applies one set of [`Rules`] to a terrain grid on a [`Layout`].
///
/// The game keeps a small integer per cell. The mapper turns that into
/// tile frames: for a whole map with [`apply`](Self::apply), for one
/// changed cell with [`cell`](Self::cell), or for a changed rectangle with
/// [`region`](Self::region). Each call reports frames through a callback;
/// `None` means the cell has no tile.
///
/// By default the map border continues each cell's own terrain, so edges
/// connect off the map; [`outside`](Self::outside) sets a fixed terrain
/// instead. Tile variants are chosen by a hash of the seed and the cell
/// position, so reapplying gives the same picture.
///
/// ```
/// use yowie_gfx::autotile::{Layout, Mapper, Rules};
///
/// // Walls: frame index equals the 4-bit neighbour mask.
/// let rules = Rules::edge16(1, std::array::from_fn(|i| Some(i as u32)));
/// let mapper = Mapper::new(rules).layout(Layout::Square);
/// let map = [1, 1, 0, 0];
/// let mut frames = Vec::new();
/// mapper.apply(2, 2, |x, y| map[(y * 2 + x) as usize], |_, _, f| frames.push(f));
/// // The top-left wall joins east (2) and, past the border, north (1) and
/// // west (8); south is open ground.
/// assert_eq!(frames[0], Some(1 | 2 | 8));
/// ```
#[derive(Clone, Debug)]
pub struct Mapper {
    rules: Rules,
    layout: Layout,
    seed: u64,
    outside: Option<i32>,
    index: WangIndex,
}

impl Mapper {
    /// A mapper for `rules` on a square grid with seed 0.
    pub fn new(rules: Rules) -> Self {
        let mut m = Self {
            rules,
            layout: Layout::Square,
            seed: 0,
            outside: None,
            index: WangIndex::default(),
        };
        m.rebuild_index();
        m
    }

    /// Sets the grid shape.
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self.rebuild_index();
        self
    }

    /// Sets the seed that varies which variant each cell picks.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Treats everything beyond the map edge as `terrain`.
    pub fn outside(mut self, terrain: i32) -> Self {
        self.outside = Some(terrain);
        self
    }

    /// The grid shape.
    pub fn grid_layout(&self) -> Layout {
        self.layout
    }

    /// The rules.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    fn rebuild_index(&mut self) {
        if self.rules.kind == Kind::Wang {
            let pos = positions(self.rules.wang_type, self.layout);
            self.index = WangIndex::build(&self.rules.wang, pos);
        }
    }

    /// Computes a frame for every cell of a `w` by `h` map and hands each
    /// to `set`. `terrain` returns the id at a cell and is only called
    /// inside the map. Corner16 rules emit one extra row and column: `x`
    /// and `y` run to `w` and `h` inclusive.
    pub fn apply(
        &self,
        w: i32,
        h: i32,
        terrain: impl Fn(i32, i32) -> i32,
        mut set: impl FnMut(i32, i32, Option<u32>),
    ) {
        let dual = self.rules.kind == Kind::Corner16;
        let (ew, eh) = if dual { (w + 1, h + 1) } else { (w, h) };
        for y in 0..eh {
            for x in 0..ew {
                set(x, y, self.compute(x, y, w, h, &terrain));
            }
        }
    }

    /// Computes the frames of a map stored as `terrain`, row by row, and
    /// returns the size of the frame grid with its frames. The grid is one
    /// cell wider and taller for Corner16 rules.
    pub fn apply_grid(&self, size: UVec2, terrain: &[i32]) -> (UVec2, Vec<Option<u32>>) {
        let (w, h) = (size.x as i32, size.y as i32);
        let out = if self.rules.kind == Kind::Corner16 {
            UVec2::new(size.x + 1, size.y + 1)
        } else {
            size
        };
        let mut frames = vec![None; out.x as usize * out.y as usize];
        self.apply(
            w,
            h,
            |x, y| terrain.get((y * w + x) as usize).copied().unwrap_or(0),
            |x, y, f| frames[(y * out.x as i32 + x) as usize] = f,
        );
        (out, frames)
    }

    /// Recomputes the cells a change at `(x, y)` can affect: the cell and
    /// the neighbours the layout gives it, or the four surrounding corners
    /// for Corner16 rules. Call it after editing one cell instead of
    /// reapplying the map. For a block of edits use
    /// [`region`](Self::region).
    pub fn cell(
        &self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        terrain: impl Fn(i32, i32) -> i32,
        mut set: impl FnMut(i32, i32, Option<u32>),
    ) {
        if self.rules.kind == Kind::Corner16 {
            for cy in y..=y + 1 {
                for cx in x..=x + 1 {
                    if cx >= 0 && cy >= 0 && cx <= w && cy <= h {
                        set(cx, cy, self.compute(cx, cy, w, h, &terrain));
                    }
                }
            }
            return;
        }
        let inside = |c: IVec2| c.x >= 0 && c.y >= 0 && c.x < w && c.y < h;
        let here = ivec2(x, y);
        if inside(here) {
            set(x, y, self.compute(x, y, w, h, &terrain));
        }
        for &d in self.layout.dirs() {
            let n = self.layout.neighbour(here, d);
            if inside(n) {
                set(n.x, n.y, self.compute(n.x, n.y, w, h, &terrain));
            }
        }
    }

    /// Recomputes the cells a change anywhere in a rectangle can affect and
    /// hands each to `set` once, in row-major order. The rectangle is `rw`
    /// by `rh` cells with its top-left at `(x, y)` on a `w` by `h` map; it
    /// may reach past the map.
    #[allow(clippy::too_many_arguments)]
    pub fn region(
        &self,
        x: i32,
        y: i32,
        rw: i32,
        rh: i32,
        w: i32,
        h: i32,
        terrain: impl Fn(i32, i32) -> i32,
        mut set: impl FnMut(i32, i32, Option<u32>),
    ) {
        if rw <= 0 || rh <= 0 {
            return;
        }
        if self.rules.kind == Kind::Corner16 {
            for cy in y.max(0)..=(y + rh).min(h) {
                for cx in x.max(0)..=(x + rw).min(w) {
                    set(cx, cy, self.compute(cx, cy, w, h, &terrain));
                }
            }
            return;
        }
        // Every layout's neighbours lie within one cell each way.
        let (bx0, by0) = ((x - 1).max(0), (y - 1).max(0));
        let (bx1, by1) = ((x + rw + 1).min(w), (y + rh + 1).min(h));
        if bx0 >= bx1 || by0 >= by1 {
            return;
        }
        let bw = bx1 - bx0;
        let mut mark = vec![false; (bw * (by1 - by0)) as usize];
        let mut put = |c: IVec2| {
            if c.x >= bx0 && c.y >= by0 && c.x < bx1 && c.y < by1 {
                mark[((c.y - by0) * bw + c.x - bx0) as usize] = true;
            }
        };
        for ey in y.max(by0 - 1)..(y + rh).min(by1 + 1) {
            for ex in x.max(bx0 - 1)..(x + rw).min(bx1 + 1) {
                let e = ivec2(ex, ey);
                put(e);
                for &d in self.layout.dirs() {
                    put(self.layout.neighbour(e, d));
                }
            }
        }
        for cy in by0..by1 {
            for cx in bx0..bx1 {
                if mark[((cy - by0) * bw + cx - bx0) as usize] {
                    set(cx, cy, self.compute(cx, cy, w, h, &terrain));
                }
            }
        }
    }

    /// The terrain at a cell with the border policy applied; `own` is the
    /// terrain being matched.
    fn at(&self, c: IVec2, w: i32, h: i32, own: i32, terrain: &impl Fn(i32, i32) -> i32) -> i32 {
        if c.x < 0 || c.y < 0 || c.x >= w || c.y >= h {
            self.outside.unwrap_or(own)
        } else {
            terrain(c.x, c.y)
        }
    }

    fn look(
        &self,
        c: IVec2,
        dir: Dir,
        w: i32,
        h: i32,
        own: i32,
        terrain: &impl Fn(i32, i32) -> i32,
    ) -> i32 {
        self.at(self.layout.neighbour(c, dir), w, h, own, terrain)
    }

    fn compute(
        &self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        terrain: &impl Fn(i32, i32) -> i32,
    ) -> Option<u32> {
        let r = &self.rules;
        let c = ivec2(x, y);
        match r.kind {
            Kind::Corner16 => {
                let t = r.terrain;
                let mut mask = 0usize;
                // The corner at (x, y) touches cells (x-1, y-1) to (x, y);
                // the cell up-left of the corner fills the tile's
                // north-west quadrant.
                for (i, o) in [(-1, -1), (0, -1), (-1, 0), (0, 0)].into_iter().enumerate() {
                    let v = self.at(ivec2(x + o.0, y + o.1), w, h, t, terrain);
                    if r.connected(v, t) {
                        mask |= 1 << i;
                    }
                }
                if mask == 0 {
                    return None;
                }
                self.pick(x, y, r.slots.get(mask)?)
            }
            Kind::Wang => self.wang_frame(c, w, h, terrain),
            kind => {
                let t = terrain(x, y);
                if t != r.terrain {
                    return None;
                }
                let mut mask = 0usize;
                match kind {
                    Kind::Edge16 => {
                        for (i, d) in [Dir::N, Dir::E, Dir::S, Dir::W].into_iter().enumerate() {
                            if r.connected(self.look(c, d, w, h, t, terrain), t) {
                                mask |= 1 << i;
                            }
                        }
                    }
                    Kind::Edge64 => {
                        for (i, &d) in self.layout.dirs().iter().enumerate() {
                            if r.connected(self.look(c, d, w, h, t, terrain), t) {
                                mask |= 1 << i;
                            }
                        }
                    }
                    _ => {
                        let mut raw = 0u8;
                        for d in Dir::ALL {
                            if r.connected(self.look(c, d, w, h, t, terrain), t) {
                                raw |= 1 << d.index();
                            }
                        }
                        mask = blob_index(raw);
                    }
                }
                // Edge64 rules under a square layout have eight neighbours,
                // and so masks the frames do not cover.
                self.pick(x, y, r.slots.get(mask)?)
            }
        }
    }

    fn wang_frame(
        &self,
        c: IVec2,
        w: i32,
        h: i32,
        terrain: &impl Fn(i32, i32) -> i32,
    ) -> Option<u32> {
        let t = terrain(c.x, c.y);
        let pos = positions(self.rules.wang_type, self.layout);
        let hex = self.layout.is_hex();
        let mut want = [0i32; 8];
        let mut empty = t == 0;
        for &p in pos {
            let i = p.index();
            want[i] = if i % 2 == 1 && !hex {
                // A diagonal position is a corner, shared with the two
                // cells beside it and the one across it.
                let corner = [(i + 7) % 8, i, (i + 1) % 8]
                    .into_iter()
                    .map(|d| self.look(c, Dir::ALL[d], w, h, t, terrain))
                    .max()
                    .unwrap_or(0);
                corner.max(t)
            } else {
                t.max(self.look(c, p, w, h, t, terrain))
            };
            if want[i] != 0 {
                empty = false;
            }
        }
        if empty {
            return None;
        }
        if let Some(exact) = self.index.exact(&want) {
            return self.pick(c.x, c.y, exact);
        }
        let best = closest(&self.rules.wang, pos, &want);
        self.pick(c.x, c.y, &best)
    }

    /// Chooses among a slot's variants by a hash of the cell position.
    fn pick(&self, x: i32, y: i32, slot: &[Weighted]) -> Option<u32> {
        match slot {
            [] => None,
            [only] => Some(only.frame),
            _ => {
                let total: f64 = slot.iter().map(|v| v.weight).sum();
                let mut r = (hash(self.seed, x, y) >> 11) as f64 / (1u64 << 53) as f64 * total;
                for v in slot {
                    if r < v.weight {
                        return Some(v.frame);
                    }
                    r -= v.weight;
                }
                slot.last().map(|v| v.frame)
            }
        }
    }
}

/// Mixes the seed and a cell position, splitmix64 style.
fn hash(seed: u64, x: i32, y: i32) -> u64 {
    let mut z = seed
        ^ (i64::from(x) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (i64::from(y) as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z ^= z >> 30;
    z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
