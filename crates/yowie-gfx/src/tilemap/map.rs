//! The tile grid.

use yowie_core::{Color, IVec2, Rect, UVec2, Vec2};

use super::anim::{TileAnimation, TileAnimations};
use super::layout::GridLayout;
use super::tile::Tile;
use crate::SpriteSheet;

/// One layer of tiles on a grid, drawn from a single sprite sheet.
///
/// A layer is one texture, so drawing it is one draw call however many
/// tiles are visible; only cells the camera can see are visited. To draw
/// several layers, make one `Tilemap` for each and give them different
/// [`Graphics::set_layer`](crate::Graphics::set_layer) values.
///
/// ```ignore
/// let mut map = Tilemap::new(GridLayout::square(vec2(16.0, 16.0)), uvec2(64, 64), sheet);
/// map.set(3, 4, Tile::new(7).flipped(TileFlip::ROTATE_90));
/// map.animate(10, TileAnimation::uniform([10, 11, 12, 11], 0.2));
/// map.advance(dt);
/// map.draw(gfx);
/// ```
#[derive(Clone, Debug)]
pub struct Tilemap {
    pub(super) layout: GridLayout,
    pub(super) size: UVec2,
    pub(super) tiles: Vec<Tile>,
    pub(super) sheet: SpriteSheet,
    /// The size a tile's picture is drawn at, anchored to the bottom
    /// middle of its cell. Defaults to the sheet's frame size.
    pub tile_size: Vec2,
    /// World position added to every cell.
    pub offset: Vec2,
    /// Multiplies every tile's colour.
    pub tint: Color,
    pub(super) anims: TileAnimations,
}

impl Tilemap {
    /// An empty map of `size` cells.
    pub fn new(layout: GridLayout, size: UVec2, sheet: SpriteSheet) -> Self {
        Self {
            layout,
            size,
            tiles: vec![Tile::EMPTY; size.x as usize * size.y as usize],
            tile_size: sheet.frame_size().as_vec2(),
            sheet,
            offset: Vec2::ZERO,
            tint: Color::WHITE,
            anims: TileAnimations::default(),
        }
    }

    /// The grid geometry.
    pub fn layout(&self) -> &GridLayout {
        &self.layout
    }

    /// The size in cells.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// The sprite sheet the tiles come from.
    pub fn sheet(&self) -> &SpriteSheet {
        &self.sheet
    }

    /// All cells, row by row.
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.size.x && (y as u32) < self.size.y)
            .then(|| y as usize * self.size.x as usize + x as usize)
    }

    /// The tile at a cell, or an empty tile outside the map.
    pub fn get(&self, x: i32, y: i32) -> Tile {
        self.index(x, y).map_or(Tile::EMPTY, |i| self.tiles[i])
    }

    /// Puts a tile in a cell; cells outside the map are ignored.
    pub fn set(&mut self, x: i32, y: i32, tile: Tile) {
        if let Some(i) = self.index(x, y) {
            self.tiles[i] = tile;
        }
    }

    /// Puts sheet frame `frame` in a cell, or clears it for `None`.
    pub fn set_frame(&mut self, x: i32, y: i32, frame: Option<u32>) {
        self.set(x, y, frame.into());
    }

    /// Fills every cell with `tile`.
    pub fn fill(&mut self, tile: Tile) {
        self.tiles.fill(tile);
    }

    /// Empties every cell.
    pub fn clear(&mut self) {
        self.fill(Tile::EMPTY);
    }

    /// Sets every cell from `f`, called with each cell's coordinates.
    pub fn fill_with(&mut self, mut f: impl FnMut(i32, i32) -> Tile) {
        let w = self.size.x as usize;
        for (i, t) in self.tiles.iter_mut().enumerate() {
            *t = f((i % w) as i32, (i / w) as i32);
        }
    }

    /// Makes every cell showing `frame` play `animation` instead. An
    /// animation with no frames is ignored.
    pub fn animate(&mut self, frame: u32, animation: TileAnimation) {
        self.anims.insert(frame, animation);
    }

    /// Moves the animations forward by `dt` seconds.
    pub fn advance(&mut self, dt: f32) {
        self.anims.advance(dt);
    }

    /// The cell under a world position, or `None` outside the map.
    pub fn cell_at(&self, world: Vec2) -> Option<IVec2> {
        let c = self.layout.cell_at(world - self.offset);
        self.index(c.x, c.y).map(|_| c)
    }

    /// The world position of the middle of a cell.
    pub fn cell_center(&self, cell: IVec2) -> Vec2 {
        self.layout.cell_center(cell) + self.offset
    }

    /// The world rectangle the cells cover.
    pub fn bounds(&self) -> Rect {
        self.layout.map_bounds(self.size).translate(self.offset)
    }
}
