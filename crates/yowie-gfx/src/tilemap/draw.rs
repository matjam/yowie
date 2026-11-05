//! Drawing a tilemap: visible cells become one triangle list.

use yowie_core::{IVec2, Vec2, ivec2, vec2};

use super::layout::{GridKind, GridLayout};
use super::map::Tilemap;
use super::tile::{Tile, TileFlip};
use crate::{Graphics, Vertex};

/// How far, in texels, a tile's texture coordinates are pulled inside its
/// frame so sampling at a seam never reads the neighbouring frame.
const UV_INSET_TEXELS: f32 = 0.02;

/// The four corners of a tile's picture, clockwise from the top-left of
/// the picture, as world positions with their texture coordinates.
///
/// `frame_uv` is the frame's `(min, max)` texture coordinates. The quad is
/// `tile_size` big with its bottom edge on the cell's bottom edge, centred
/// across the cell.
pub(crate) fn tile_quad(
    layout: &GridLayout,
    centre: Vec2,
    tile_size: Vec2,
    flip: TileFlip,
    frame_uv: (Vec2, Vec2),
) -> [(Vec2, Vec2); 4] {
    let bottom = centre.y + layout.cell.y * 0.5;
    let min = vec2(centre.x - tile_size.x * 0.5, bottom - tile_size.y);
    let mut pos = [
        min,
        min + vec2(tile_size.x, 0.0),
        min + tile_size,
        min + vec2(0.0, tile_size.y),
    ];
    if flip.rotate_120 {
        let mid = min + tile_size * 0.5;
        let r = Vec2::from_angle(std::f32::consts::TAU / 3.0);
        pos = pos.map(|p| mid + r.rotate(p - mid));
    }
    let (lo, hi) = frame_uv;
    let samples = flip.sample_corners();
    std::array::from_fn(|i| (pos[i], lo + samples[i] * (hi - lo)))
}

impl Tilemap {
    /// Queues the visible tiles. Only cells that can reach the camera's
    /// visible rectangle are visited, and all of them go out as one
    /// triangle list on the sheet's texture. The current layer, sort key,
    /// blend mode and shader of `gfx` apply.
    pub fn draw(&self, gfx: &mut Graphics) {
        self.draw_shifted(gfx, Vec2::ZERO);
    }

    /// Like [`draw`](Self::draw), with the whole layer moved by `shift`
    /// world units; parallax layers use it.
    pub fn draw_shifted(&self, gfx: &mut Graphics, shift: Vec2) {
        let origin = self.offset + shift;
        let area = gfx.visible_area().translate(-origin);
        let cell = self.layout.cell;
        let reach = (self.tile_size - cell).max(Vec2::ZERO);
        // A turned hexagonal tile stays inside its cell's bounding box.
        let Some((lo, hi)) = self.layout.cells_in(area, reach, self.size) else {
            return;
        };
        let texture = self.sheet.texture();
        let ts = texture.size().as_vec2();
        let inset = Vec2::splat(UV_INSET_TEXELS) / ts;
        let count = ((hi.x - lo.x + 1) * (hi.y - lo.y + 1)) as usize;
        let mut vertices = Vec::with_capacity(count * 4);
        let mut indices = Vec::with_capacity(count * 6);
        let mut visit = |c: IVec2| {
            let tile = self.tiles[c.y as usize * self.size.x as usize + c.x as usize];
            if let Some(quad) = self.quad_for(c, tile, origin, inset) {
                let base = vertices.len() as u32;
                for (p, uv) in quad {
                    vertices.push(Vertex::new(p, uv, self.tint));
                }
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        };
        if self.layout.kind == GridKind::Isometric {
            // Back to front: cells further down the screen cover the ones
            // behind them.
            for s in (lo.x + lo.y)..=(hi.x + hi.y) {
                for x in lo.x.max(s - hi.y)..=hi.x.min(s - lo.y) {
                    visit(ivec2(x, s - x));
                }
            }
        } else {
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    visit(ivec2(x, y));
                }
            }
        }
        if !indices.is_empty() {
            gfx.draw_triangles(Some(texture), &vertices, &indices);
        }
    }

    fn quad_for(
        &self,
        cell: IVec2,
        tile: Tile,
        origin: Vec2,
        inset: Vec2,
    ) -> Option<[(Vec2, Vec2); 4]> {
        if tile.is_empty() {
            return None;
        }
        let frame = self.anims.current(tile.frame);
        let (lo, hi) = self.sheet.frame_uv(frame)?;
        let centre = self.layout.cell_center(cell) + origin;
        Some(tile_quad(
            &self.layout,
            centre,
            self.tile_size,
            tile.flip,
            (lo + inset, hi - inset),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_tests::{assert_px, headless, render};
    use crate::tilemap::{Stagger, TileFlip};
    use crate::{Camera2D, SpriteSheet, Texture, TextureOptions};
    use yowie_core::{Color, uvec2};

    #[test]
    fn quads_anchor_to_the_cell_bottom_and_apply_flips() {
        let l = GridLayout::square(vec2(16.0, 16.0));
        let c = l.cell_center(ivec2(2, 1));
        let uv = (vec2(0.0, 0.0), vec2(0.5, 0.5));
        let q = tile_quad(&l, c, vec2(16.0, 32.0), TileFlip::NONE, uv);
        assert_eq!(q[0].0, vec2(32.0, 0.0));
        assert_eq!(q[2].0, vec2(48.0, 32.0));
        let flipped = tile_quad(
            &l,
            c,
            vec2(16.0, 32.0),
            TileFlip {
                horizontal: true,
                ..TileFlip::NONE
            },
            uv,
        );
        assert_eq!(flipped[0].1, vec2(0.5, 0.0));
        assert_eq!(flipped[1].1, vec2(0.0, 0.0));
    }

    #[test]
    fn rotated_hex_tiles_turn_about_their_middle() {
        let l = GridLayout::hex_pointy(vec2(52.0, 60.0), Stagger::Odd);
        let c = l.cell_center(ivec2(0, 0));
        let flip = TileFlip {
            rotate_120: true,
            ..TileFlip::NONE
        };
        let q = tile_quad(&l, c, vec2(52.0, 60.0), flip, (Vec2::ZERO, Vec2::ONE));
        let mid = c;
        for (p, _) in q {
            let d = (p - mid).length();
            assert!((d - vec2(26.0, 30.0).length()).abs() < 1e-3);
        }
    }

    fn checker(gfx: &Graphics) -> SpriteSheet {
        // Four 4x4 frames: red, green, blue, yellow.
        let mut px = Vec::new();
        for row in 0..4 {
            for frame in 0..4 {
                let color = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]][frame];
                let _ = row;
                for _ in 0..4 {
                    px.extend_from_slice(&[color[0], color[1], color[2], 255]);
                }
            }
        }
        let Ok(tex) = Texture::from_pixels(gfx, 16, 4, &px, TextureOptions::default()) else {
            panic!("texture");
        };
        SpriteSheet::new(tex, uvec2(4, 4))
    }

    #[test]
    fn tilemap_snapshot_draws_tiles_with_culling() {
        let Some(mut gfx) = headless(uvec2(64, 32)) else {
            return;
        };
        let sheet = checker(&gfx);
        let mut map = Tilemap::new(GridLayout::square(vec2(4.0, 4.0)), uvec2(100, 100), sheet);
        map.fill_with(|x, y| Tile::new(((x + y) % 4) as u32));
        map.animate(3, super::super::TileAnimation::uniform([0], 1.0));
        gfx.begin_frame();
        // Look at the map's cells 40..56 by 20..28.
        let cam = Camera2D::new(vec2(40.0 * 4.0 + 32.0, 20.0 * 4.0 + 16.0));
        gfx.with_camera(&cam, |g| map.draw(g));
        let stats_before = gfx.stats();
        let _ = stats_before;
        let img = render(&mut gfx);
        // The map has 10000 cells but a 64x32 view shows 16 by 8.
        assert!(gfx.stats().vertices <= 4 * (18 * 10), "{:?}", gfx.stats());
        assert_eq!(gfx.stats().draw_calls, 1);
        // Pixel (2, 2) is cell (40, 20): (40+20)%4 = 0, red.
        assert_px(&img, 2, 2, [255, 0, 0, 255], 2);
        // Pixel (6, 2) is cell (41, 20): frame 1, green.
        assert_px(&img, 6, 2, [0, 255, 0, 255], 2);
        // Cell (43, 20) is frame 3, animated into frame 0: red.
        assert_px(&img, 14, 2, [255, 0, 0, 255], 2);
        let _ = Color::WHITE;
    }
}
