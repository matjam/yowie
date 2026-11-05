//! Paints terrain that picks its own tiles. Grass is a 47-tile blob set
//! composed from a six-tile template by `expand_blob`, with a flower
//! variant mixed into the filled tiles; walls are a 16-tile edge set;
//! water is a two-colour corner Wang set whose shorelines curve around the
//! land. Each reads the same terrain grid through its own `Mapper`. Paint
//! with the left mouse button, erase with the right; 1 selects the grass
//! brush, 2 the wall brush and 3 the water brush. Escape quits.
//!
//! The strip along the bottom is the same idea on a hexagonal layout: a
//! 64-tile edge set on hexagons in staggered rows, where each tile shows a
//! rim on the sides no neighbour continues.

#[path = "common/art.rs"]
mod art;

use art::Canvas;
use yowie::gfx::autotile::{Layout, Mapper, Rules, WangTile, WangType, expand_blob};
use yowie::gfx::image::RgbaImage;
use yowie::gfx::{GridLayout, ScaleMode, SpriteSheet, Stagger, Texture, TextureOptions, Tilemap};
use yowie::input::{Key, MouseButton};
use yowie::rng::Rng;
use yowie::{Color, Config, Context, Game, GameResult, Rect, Vec2, ivec2, uvec2, vec2};

const TILE: u32 = 16;
const MAP_W: i32 = 40;
const MAP_H: i32 = 22;
const HEX_W: i32 = 21;
const HEX_H: i32 = 6;
const HEX_ROW: f32 = 12.0;
const HEX_TOP: f32 = (MAP_H as u32 * TILE) as f32;
const HEX_STRIP: f32 = HEX_H as f32 * HEX_ROW + 4.0;
const VIEW_H: u32 = MAP_H as u32 * TILE + HEX_STRIP as u32;

const EMPTY: i32 = 0;
const GRASS: i32 = 1;
const WALL: i32 = 2;
const WATER: i32 = 3;
const LAND_COLOR: i32 = 1;
const WATER_COLOR: i32 = 2;

fn sheet(gfx: &yowie::gfx::Graphics, img: &RgbaImage) -> GameResult<SpriteSheet> {
    let tex = Texture::from_image(gfx, img, TextureOptions::default())?;
    Ok(SpriteSheet::new(tex, uvec2(TILE, TILE)))
}

struct Autotile {
    terrain: Vec<i32>,
    grass_map: Tilemap,
    wall_map: Tilemap,
    water_map: Tilemap,
    grass: Mapper,
    wall: Mapper,
    water: Mapper,
    hex_map: Tilemap,
    brush: i32,
    font: yowie::gfx::Font,
}

impl Autotile {
    fn at(&self, x: i32, y: i32) -> i32 {
        self.terrain[(y * MAP_W + x) as usize]
    }

    fn refresh(&mut self) {
        let t = self.terrain.clone();
        let at = |x: i32, y: i32| t[(y * MAP_W + x) as usize];
        let color = |x: i32, y: i32| {
            if at(x, y) == WATER {
                WATER_COLOR
            } else {
                LAND_COLOR
            }
        };
        let (gm, wm, am) = (&mut self.grass_map, &mut self.wall_map, &mut self.water_map);
        self.grass
            .apply(MAP_W, MAP_H, at, |x, y, f| gm.set(x, y, f.into()));
        self.wall
            .apply(MAP_W, MAP_H, at, |x, y, f| wm.set(x, y, f.into()));
        self.water
            .apply(MAP_W, MAP_H, color, |x, y, f| am.set(x, y, f.into()));
    }

    /// Redoes only the neighbourhood of one changed cell.
    fn refresh_cell(&mut self, x: i32, y: i32) {
        let t = self.terrain.clone();
        let at = |x: i32, y: i32| t[(y * MAP_W + x) as usize];
        let color = |x: i32, y: i32| {
            if at(x, y) == WATER {
                WATER_COLOR
            } else {
                LAND_COLOR
            }
        };
        let (gm, wm, am) = (&mut self.grass_map, &mut self.wall_map, &mut self.water_map);
        self.grass
            .cell(x, y, MAP_W, MAP_H, at, |x, y, f| gm.set(x, y, f.into()));
        self.wall
            .cell(x, y, MAP_W, MAP_H, at, |x, y, f| wm.set(x, y, f.into()));
        self.water
            .cell(x, y, MAP_W, MAP_H, color, |x, y, f| am.set(x, y, f.into()));
    }
}

impl Game for Autotile {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let layout = GridLayout::square(vec2(TILE as f32, TILE as f32));
        let size = uvec2(MAP_W as u32, MAP_H as u32);

        // Grass: 47 blob tiles from a drawn template, plus a flower tile
        // in the spare 48th slot as a variant of the filled case.
        let mut blob = expand_blob(&make_template(), TILE).unwrap_or_default();
        add_flowers(&mut blob);
        let frames = std::array::from_fn(|i| Some(i as u32));
        let grass = Mapper::new(Rules::blob47(GRASS, frames).variant(255, 47, 0.4));
        let grass_map = Tilemap::new(layout, size, sheet(&ctx.gfx, &blob)?);

        let wall_frames = std::array::from_fn(|i| Some(i as u32));
        let wall = Mapper::new(Rules::edge16(WALL, wall_frames));
        let wall_map = Tilemap::new(layout, size, sheet(&ctx.gfx, &make_walls())?);

        // Water: one tile per combination of water corners, which is the
        // complete two-colour corner Wang set.
        let water = Mapper::new(Rules::wang(WangType::Corners, water_tiles()));
        let water_map = Tilemap::new(layout, size, sheet(&ctx.gfx, &make_water())?);

        // Seed the map: grass blobs from a few random walks, ponds and a
        // wall run.
        let mut terrain = vec![EMPTY; (MAP_W * MAP_H) as usize];
        let mut rng = Rng::new(7);
        let mut walk = |id: i32, steps: usize, terrain: &mut Vec<i32>| {
            let (mut x, mut y) = (rng.range(0, MAP_W - 1), rng.range(0, MAP_H - 1));
            for _ in 0..steps {
                if (0..MAP_W).contains(&x) && (0..MAP_H).contains(&y) {
                    terrain[(y * MAP_W + x) as usize] = id;
                }
                x += rng.range(-1, 1);
                y += rng.range(-1, 1);
            }
        };
        for _ in 0..6 {
            walk(GRASS, 90, &mut terrain);
        }
        for _ in 0..3 {
            walk(WATER, 60, &mut terrain);
        }
        for x in 6..26 {
            terrain[(8 * MAP_W + x) as usize] = WALL;
        }
        for y in 8..16 {
            terrain[(y * MAP_W + 26) as usize] = WALL;
        }

        // The hexagonal strip: pointy-top hexagons in staggered rows with
        // the odd rows shifted right, which is Tiled's stagger axis Y with
        // stagger index odd. The terrain is walked over the layout's own
        // neighbours, so nothing here knows the hexagon offsets.
        let hex_layout = GridLayout::hex_pointy(vec2(TILE as f32, TILE as f32), Stagger::Odd);
        let auto_layout = hex_layout.autotile_layout();
        debug_assert_eq!(auto_layout, Layout::HexRowsOdd);
        let mut hex_land = vec![0; (HEX_W * HEX_H) as usize];
        let mut rng = Rng::new(11);
        for (sx, sy) in [(3, 2), (9, 3), (15, 2), (19, 4)] {
            let mut c = ivec2(sx, sy);
            for _ in 0..40 {
                if (0..HEX_W).contains(&c.x) && (0..HEX_H).contains(&c.y) {
                    hex_land[(c.y * HEX_W + c.x) as usize] = 1;
                }
                let dirs = auto_layout.dirs();
                c = auto_layout.neighbour(c, dirs[rng.below(dirs.len() as u64) as usize]);
            }
        }
        let hex_frames = std::array::from_fn(|i| Some(i as u32));
        let hex_mapper = Mapper::new(Rules::edge64(1, hex_frames)).layout(auto_layout);
        let mut hex_map = Tilemap::new(hex_layout, uvec2(HEX_W as u32, HEX_H as u32), {
            let img = make_hexes();
            let tex = Texture::from_image(&ctx.gfx, &img, TextureOptions::default())?;
            SpriteSheet::new(tex, uvec2(TILE, TILE))
        });
        hex_map.offset = vec2(0.0, HEX_TOP);
        hex_mapper.apply(
            HEX_W,
            HEX_H,
            |x, y| hex_land[(y * HEX_W + x) as usize],
            |x, y, f| hex_map.set(x, y, f.into()),
        );

        let mut game = Autotile {
            terrain,
            grass_map,
            wall_map,
            water_map,
            grass,
            wall,
            water,
            hex_map,
            brush: GRASS,
            font: ctx.gfx.default_font(),
        };
        game.refresh();
        Ok(game)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        let input = &ctx.input;
        for (key, brush) in [
            (Key::Digit1, GRASS),
            (Key::Digit2, WALL),
            (Key::Digit3, WATER),
        ] {
            if input.key_pressed(key) {
                self.brush = brush;
            }
        }
        if let Some(cell) = self
            .grass_map
            .cell_at(input.mouse())
            .filter(|_| input.mouse_inside())
        {
            let want = if input.mouse_down(MouseButton::Left) {
                Some(self.brush)
            } else if input.mouse_down(MouseButton::Right) {
                Some(EMPTY)
            } else {
                None
            };
            if let Some(want) = want
                && self.at(cell.x, cell.y) != want
            {
                self.terrain[(cell.y * MAP_W + cell.x) as usize] = want;
                // One changed cell only needs its neighbourhood redone.
                self.refresh_cell(cell.x, cell.y);
            }
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mouse = ctx.input.mouse();
        let inside = ctx.input.mouse_inside();
        let g = &mut ctx.gfx;
        let map = Rect::new(
            0.0,
            0.0,
            (MAP_W as u32 * TILE) as f32,
            (MAP_H as u32 * TILE) as f32,
        );
        g.fill_rect(map, Color::rgb8(38, 30, 26));
        g.set_layer(0);
        self.water_map.draw(g);
        g.set_layer(1);
        self.grass_map.draw(g);
        g.set_layer(2);
        self.wall_map.draw(g);
        g.set_layer(3);
        if let Some(c) = self.grass_map.cell_at(mouse).filter(|_| inside) {
            let o = self.grass_map.cell_center(c) - Vec2::splat(TILE as f32 / 2.0);
            g.fill_rect(
                Rect::new(o.x, o.y, TILE as f32, TILE as f32),
                Color::rgba8(255, 255, 255, 60),
            );
        }
        g.fill_rect(
            Rect::new(0.0, HEX_TOP, map.w, HEX_STRIP),
            Color::rgb8(20, 24, 34),
        );
        self.hex_map.draw(g);
        let name = ["", "grass", "wall", "water"][self.brush as usize];
        let hint = format!("brush: {name} (1 grass, 2 wall, 3 water; left paints, right erases)");
        g.draw_text(
            &self.font,
            &hint,
            vec2(6.0, 4.0),
            &yowie::gfx::TextStyle::new(11.0).color(Color::rgb8(240, 236, 220)),
        );
        Ok(())
    }
}

/// The sixteen corner Wang tiles: frame `m` has water at the corners whose
/// bits are set (north-west 1, north-east 2, south-west 4, south-east 8)
/// and land elsewhere. Frame 0, all land, is a transparent tile so dry
/// cells draw nothing.
fn water_tiles() -> Vec<WangTile> {
    use yowie::gfx::autotile::Dir;
    (0..16u32)
        .map(|m| {
            let mut colors = [0; 8];
            for (i, d) in [Dir::NW, Dir::NE, Dir::SW, Dir::SE].into_iter().enumerate() {
                colors[d.index()] = if m & (1 << i) != 0 {
                    WATER_COLOR
                } else {
                    LAND_COLOR
                };
            }
            WangTile::new(m, colors)
        })
        .collect()
}

/// Draws the sixteen water tiles. Each pixel interpolates the four
/// corners' wetness and is water past the halfway mark, so a lone wet
/// corner rounds off and two wet corners meet in a straight shore; a band
/// just inside the shore is foam.
fn make_water() -> RgbaImage {
    let mut c = Canvas::new(16 * TILE, TILE);
    let (deep, wave, foam) = (
        [46, 104, 176, 255],
        [60, 124, 196, 255],
        [176, 214, 232, 255],
    );
    for m in 0..16i32 {
        let wet_at = |i: i32| if m & (1 << i) != 0 { 1.0f32 } else { 0.0 };
        c.paint(m * TILE as i32, 0, TILE as i32, |px, py| {
            let (u, v) = (
                (px as f32 + 0.5) / TILE as f32,
                (py as f32 + 0.5) / TILE as f32,
            );
            let wet = wet_at(0) * (1.0 - u) * (1.0 - v)
                + wet_at(1) * u * (1.0 - v)
                + wet_at(2) * (1.0 - u) * v
                + wet_at(3) * u * v;
            if wet < 0.5 {
                return None;
            }
            Some(if wet < 0.62 {
                foam
            } else if (px / 4 + py / 4) % 2 == 0 {
                wave
            } else {
                deep
            })
        });
    }
    c.img
}

/// Draws the 16 wall tiles: a stone block with arms toward each connected
/// side, indexed by the edge16 mask. The shape is filled first and
/// outlined afterwards, so a straight run reads as one wall.
fn make_walls() -> RgbaImage {
    let mut c = Canvas::new(16 * TILE, TILE);
    let (stone, shade) = ([120, 116, 128, 255], [86, 82, 96, 255]);
    let (a, b) = (5usize, 11usize);
    let t = TILE as usize;
    for mask in 0..16usize {
        let mut solid = vec![vec![false; t]; t];
        let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize| {
            for row in solid.iter_mut().take(y1).skip(y0) {
                for v in row.iter_mut().take(x1).skip(x0) {
                    *v = true;
                }
            }
        };
        fill(a, a, b, b);
        if mask & 1 != 0 {
            fill(a, 0, b, a);
        }
        if mask & 2 != 0 {
            fill(b, a, t, b);
        }
        if mask & 4 != 0 {
            fill(a, b, b, t);
        }
        if mask & 8 != 0 {
            fill(0, a, a, b);
        }
        for py in 0..t {
            for px in 0..t {
                if !solid[py][px] {
                    continue;
                }
                // Outline pixels have an empty neighbour inside the tile;
                // at the tile border the arm continues next door.
                let mut col = stone;
                for (dx, dy) in [(0i32, -1i32), (1, 0), (0, 1), (-1, 0)] {
                    let (nx, ny) = (px as i32 + dx, py as i32 + dy);
                    if nx >= 0
                        && ny >= 0
                        && nx < t as i32
                        && ny < t as i32
                        && !solid[ny as usize][nx as usize]
                    {
                        col = shade;
                    }
                }
                c.put((mask * t + px) as i32, py as i32, col);
            }
        }
    }
    c.img
}

/// Draws the six-tile grass template `expand_blob` expects: the
/// inside-corner tile, a preview, and the four corners of a filled
/// two-by-two block. Edges get a dark rim and rounded silhouettes.
fn make_template() -> RgbaImage {
    let mut c = Canvas::new(2 * TILE, 3 * TILE);
    let (light, dark, rim) = ([96, 168, 72, 255], [52, 112, 48, 255], [34, 74, 40, 255]);
    let t = TILE as i32;
    // Paints one template tile: grass with a subtle checker, a rim on the
    // exposed sides, and rounded corners where two rims meet.
    let mut fill = |tx: i32, ty: i32, top: bool, right: bool, bottom: bool, left: bool| {
        let r = 5;
        for py in 0..t {
            for px in 0..t {
                let mut col = if (px / 2 + py / 2) % 2 == 0 {
                    dark
                } else {
                    light
                };
                let edge = (top && py < 3)
                    || (bottom && py >= t - 3)
                    || (left && px < 3)
                    || (right && px >= t - 3);
                if edge {
                    col = rim;
                }
                let mut corner = None;
                if top && left && px < r && py < r {
                    corner = Some((r, r));
                }
                if top && right && px >= t - r && py < r {
                    corner = Some((t - 1 - r, r));
                }
                if bottom && left && px < r && py >= t - r {
                    corner = Some((r, t - 1 - r));
                }
                if bottom && right && px >= t - r && py >= t - r {
                    corner = Some((t - 1 - r, t - 1 - r));
                }
                if let Some((cx, cy)) = corner {
                    let d = (px - cx) * (px - cx) + (py - cy) * (py - cy);
                    if d > r * r {
                        continue;
                    }
                    if d > (r - 3) * (r - 3) {
                        col = rim;
                    }
                }
                c.put(tx * t + px, ty * t + py, col);
            }
        }
    };
    fill(1, 0, true, true, true, true);
    fill(0, 1, true, false, false, true);
    fill(1, 1, true, true, false, false);
    fill(0, 2, false, false, true, true);
    fill(1, 2, false, true, true, false);
    // The inner tile: interior grass with a rim block in each corner, so
    // the 3-pixel rims arriving from both neighbours turn the corner at
    // the same width.
    fill(0, 0, false, false, false, false);
    for (cx, cy) in [(0, 0), (t - 3, 0), (0, t - 3), (t - 3, t - 3)] {
        c.rect(cx, cy, 3, 3, rim);
    }
    c.img
}

/// Copies the filled tile (frame 46) into the spare 48th slot and scatters
/// flowers on it, for the variant of the full neighbourhood.
fn add_flowers(img: &mut RgbaImage) {
    let t = TILE;
    let (sx, sy) = (46 % 8 * t, 46 / 8 * t);
    let (dx, dy) = (47 % 8 * t, 47 / 8 * t);
    for y in 0..t {
        for x in 0..t {
            let p = *img.get_pixel(sx + x, sy + y);
            img.put_pixel(dx + x, dy + y, p);
        }
    }
    let (pink, gold) = ([235, 140, 190, 255], [245, 210, 90, 255]);
    for (i, (px, py)) in [(4, 5), (11, 3), (8, 10), (13, 12), (3, 12)]
        .into_iter()
        .enumerate()
    {
        let col = yowie::gfx::image::Rgba(if i % 2 == 1 { gold } else { pink });
        for (ox, oy) in [(0, 0), (1, 0), (0, 1)] {
            img.put_pixel(dx + px + ox, dy + py + oy, col);
        }
    }
}

/// Draws the 64 hexagon tiles: a pointy-top hexagon filled with a checker,
/// with a light rim along each side the mask says has no neighbour of the
/// same terrain. Bit `i` is the `i`-th direction the rows layout uses,
/// clockwise from north-east, which is the order the sides are walked.
fn make_hexes() -> RgbaImage {
    let mut c = Canvas::new(8 * TILE, 8 * TILE);
    let (body, shade, rim) = (
        [96, 120, 168, 255],
        [74, 96, 140, 255],
        [206, 222, 244, 255],
    );
    let t = TILE as f32;
    let (half, side) = (t / 2.0, t / 2.0);
    // Vertices clockwise from the top; the left and right sides are
    // vertical. Side `i` runs from vertex `i` to vertex `i + 1`, so side 0
    // is the north-east edge.
    let verts = [
        (half, 0.0),
        (t, (t - side) / 2.0),
        (t, (t + side) / 2.0),
        (half, t),
        (0.0, (t + side) / 2.0),
        (0.0, (t - side) / 2.0),
    ];
    for mask in 0..64i32 {
        let (ox, oy) = (mask % 8 * TILE as i32, mask / 8 * TILE as i32);
        c.paint(ox, oy, TILE as i32, |px, py| {
            let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
            let mut nearest = 0;
            let mut near_d = f32::INFINITY;
            for i in 0..6 {
                let (a, b) = (verts[i], verts[(i + 1) % 6]);
                let (ex, ey) = (b.0 - a.0, b.1 - a.1);
                // Clockwise on a y-down screen: outside an edge is on its
                // left.
                let d = ((fx - a.0) * ey - (fy - a.1) * ex) / ex.hypot(ey);
                if d > 0.0 {
                    return None;
                }
                if -d < near_d {
                    near_d = -d;
                    nearest = i;
                }
            }
            // Edge `i` of the outline faces direction `i` of the rows
            // layout, which starts at north-east.
            let mut col = if (px / 3 + py / 3) % 2 == 0 {
                shade
            } else {
                body
            };
            if near_d < 2.2 && mask & (1 << nearest) == 0 {
                col = rim;
            }
            Some(col)
        });
    }
    c.img
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(14, 16, 24),
        ..Config::new("Yowie autotile")
            .with_size(MAP_W as u32 * TILE * 2, VIEW_H * 2)
            .with_view(MAP_W as u32 * TILE, VIEW_H, ScaleMode::Integer)
    };
    yowie::run::<Autotile>(config)
}
