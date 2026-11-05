//! Sprite sheets: a texture cut into a grid of equal frames.

use yowie_anim::Flipbook;
use yowie_core::{Rect, UVec2, Vec2};

use crate::{Graphics, Sprite, Texture};

/// The layout of a sprite sheet without its texture: frames of equal size
/// numbered row by row from the top-left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetGrid {
    /// Size of one frame in texels.
    pub frame_size: UVec2,
    /// Frames per row.
    pub columns: u32,
    /// Number of rows.
    pub rows: u32,
    /// Texels between the texture edge and the first frame.
    pub margin: u32,
    /// Texels between neighbouring frames.
    pub spacing: u32,
}

impl SheetGrid {
    /// The grid that fits `frame_size` frames in a texture of
    /// `texture_size`, with no margin and no spacing.
    pub fn fit(texture_size: UVec2, frame_size: UVec2) -> Self {
        Self {
            frame_size,
            columns: (texture_size.x / frame_size.x.max(1)).max(1),
            rows: (texture_size.y / frame_size.y.max(1)).max(1),
            margin: 0,
            spacing: 0,
        }
    }

    /// The grid that fits `frame_size` frames in `texture_size` with the
    /// given margin and spacing.
    pub fn fit_spaced(texture_size: UVec2, frame_size: UVec2, margin: u32, spacing: u32) -> Self {
        let fit = |total: u32, frame: u32| {
            let usable = total.saturating_sub(2 * margin);
            ((usable + spacing) / (frame + spacing).max(1)).max(1)
        };
        Self {
            frame_size,
            columns: fit(texture_size.x, frame_size.x),
            rows: fit(texture_size.y, frame_size.y),
            margin,
            spacing,
        }
    }

    /// The number of frames.
    pub fn count(&self) -> u32 {
        self.columns * self.rows
    }

    /// The rectangle of frame `index` in texels, or `None` past the last
    /// frame.
    pub fn frame_rect(&self, index: u32) -> Option<Rect> {
        if self.columns == 0 || index >= self.count() {
            return None;
        }
        let (col, row) = (index % self.columns, index / self.columns);
        let step = self.frame_size + UVec2::splat(self.spacing);
        Some(Rect::new(
            (self.margin + col * step.x) as f32,
            (self.margin + row * step.y) as f32,
            self.frame_size.x as f32,
            self.frame_size.y as f32,
        ))
    }
}

/// A texture cut into a [`SheetGrid`] of frames.
///
/// ```ignore
/// let sheet = SpriteSheet::new(texture, uvec2(16, 16));
/// sheet.draw_frame(gfx, 3, &Sprite::at(pos).centered());
/// ```
#[derive(Clone, Debug)]
pub struct SpriteSheet {
    texture: Texture,
    grid: SheetGrid,
}

impl SpriteSheet {
    /// Cuts `texture` into frames of `frame_size` texels.
    pub fn new(texture: Texture, frame_size: UVec2) -> Self {
        let grid = SheetGrid::fit(texture.size(), frame_size);
        Self { texture, grid }
    }

    /// Cuts `texture` into frames with a margin around the grid and
    /// spacing between frames.
    pub fn with_spacing(texture: Texture, frame_size: UVec2, margin: u32, spacing: u32) -> Self {
        let grid = SheetGrid::fit_spaced(texture.size(), frame_size, margin, spacing);
        Self { texture, grid }
    }

    /// Uses an explicit grid.
    pub fn from_grid(texture: Texture, grid: SheetGrid) -> Self {
        Self { texture, grid }
    }

    /// The texture holding the frames.
    pub fn texture(&self) -> &Texture {
        &self.texture
    }

    /// The frame layout.
    pub fn grid(&self) -> &SheetGrid {
        &self.grid
    }

    /// The number of frames.
    pub fn count(&self) -> u32 {
        self.grid.count()
    }

    /// The size of one frame in texels.
    pub fn frame_size(&self) -> UVec2 {
        self.grid.frame_size
    }

    /// The rectangle of frame `index` in texels.
    pub fn frame_rect(&self, index: u32) -> Option<Rect> {
        self.grid.frame_rect(index)
    }

    /// The texture coordinates of frame `index` as `(min, max)`, 0 to 1.
    pub fn frame_uv(&self, index: u32) -> Option<(Vec2, Vec2)> {
        let r = self.frame_rect(index)?;
        let ts = self.texture.size().as_vec2();
        Some((r.min() / ts, r.max() / ts))
    }

    /// Queues frame `index` placed by `sprite`. The sprite's source
    /// rectangle is replaced by the frame, and its size is the frame's
    /// size when `sprite.size` is `None`. Does nothing for an index past
    /// the last frame.
    pub fn draw_frame(&self, gfx: &mut Graphics, index: u32, sprite: &Sprite) {
        if let Some(src) = self.frame_rect(index) {
            let sprite = Sprite {
                source: Some(src),
                ..*sprite
            };
            gfx.draw_sprite(&self.texture, &sprite);
        }
    }
}

/// A flipbook that plays `frames` of a sheet at `fps`, with each named
/// tag covering a range of positions in `frames`. Use it with
/// [`yowie_anim::FlipbookPlayer`], whose `frame()` is the sheet index to
/// pass to [`SpriteSheet::draw_frame`].
pub fn sheet_flipbook(
    frames: impl IntoIterator<Item = usize>,
    fps: f32,
    tags: &[(&str, usize, usize)],
) -> Flipbook {
    let mut book = Flipbook::from_fps(frames, fps);
    for (name, from, to) in tags {
        book = book.with_tag(*name, *from, *to, yowie_anim::TagDirection::Forward);
    }
    book
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::uvec2;

    #[test]
    fn frame_rects_follow_margin_and_spacing() {
        let g = SheetGrid::fit_spaced(uvec2(38, 20), uvec2(16, 8), 1, 2);
        assert_eq!((g.columns, g.rows), (2, 2));
        let cases = [
            (0, Some(Rect::new(1.0, 1.0, 16.0, 8.0))),
            (1, Some(Rect::new(19.0, 1.0, 16.0, 8.0))),
            (2, Some(Rect::new(1.0, 11.0, 16.0, 8.0))),
            (3, Some(Rect::new(19.0, 11.0, 16.0, 8.0))),
            (4, None),
        ];
        for (i, want) in cases {
            assert_eq!(g.frame_rect(i), want, "frame {i}");
        }
    }

    #[test]
    fn plain_grid_counts() {
        let g = SheetGrid::fit(uvec2(64, 48), uvec2(16, 16));
        assert_eq!((g.columns, g.rows, g.count()), (4, 3, 12));
    }
}
