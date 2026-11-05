//! Glyph atlases: rasterised glyphs packed into shelves of one texture.

use yowie_core::{UVec2, uvec2};

use crate::error::Result;
use crate::shared::Shared;
use crate::texture::{Filter, Purpose, Texture, TextureOptions};

/// Space left between glyphs so linear filtering never reads a neighbour.
const PAD: u32 = 1;
/// The size a new atlas starts at.
const START_SIZE: u32 = 512;
/// The largest an atlas grows to.
const MAX_SIZE: u32 = 4096;

/// Which atlas a glyph lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AtlasKind {
    /// One byte of coverage per texel, tinted by the text colour.
    Mask,
    /// Signed distance tiles, one byte per texel, for scalable text.
    Sdf,
    /// Premultiplied sRGB colour, for emoji and other colour glyphs.
    Color,
}

#[derive(Clone, Copy, Debug)]
struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

/// Packs rectangles into horizontal shelves of a square area.
#[derive(Debug, Default)]
struct ShelfPacker {
    shelves: Vec<Shelf>,
    next_y: u32,
    size: u32,
}

impl ShelfPacker {
    fn new(size: u32) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (pw, ph) = (w + PAD, h + PAD);
        if pw > self.size || ph > self.size {
            return None;
        }
        let size = self.size;
        // The shortest shelf that fits wastes the least height; a shelf more
        // than twice as tall as the glyph is left for taller glyphs.
        let best = self
            .shelves
            .iter_mut()
            .filter(|s| s.height >= ph && s.height <= ph * 2 && s.x + pw <= size)
            .min_by_key(|s| s.height);
        if let Some(shelf) = best {
            let at = (shelf.x, shelf.y);
            shelf.x += pw;
            return Some(at);
        }
        if self.next_y + ph > self.size {
            return None;
        }
        let y = self.next_y;
        self.next_y += ph;
        self.shelves.push(Shelf {
            y,
            height: ph,
            x: pw,
        });
        Some((0, y))
    }
}

/// One atlas texture and the shelves packed into it.
#[derive(Debug)]
pub(crate) struct GlyphAtlas {
    kind: AtlasKind,
    pub(crate) texture: Texture,
    packer: ShelfPacker,
}

impl GlyphAtlas {
    fn make_texture(shared: &Shared, kind: AtlasKind, size: u32) -> Result<Texture> {
        let format = match kind {
            AtlasKind::Mask | AtlasKind::Sdf => wgpu::TextureFormat::R8Unorm,
            AtlasKind::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
        };
        Texture::create(
            shared,
            uvec2(size, size),
            format,
            TextureOptions {
                filter: Filter::Linear,
                premultiplied: true,
                ..TextureOptions::default()
            },
            Purpose::Sampled,
        )
    }

    pub(crate) fn new(shared: &Shared, kind: AtlasKind) -> Result<Self> {
        Ok(Self {
            kind,
            texture: Self::make_texture(shared, kind, START_SIZE)?,
            packer: ShelfPacker::new(START_SIZE),
        })
    }

    /// The atlas size in texels.
    pub(crate) fn size(&self) -> UVec2 {
        UVec2::splat(self.packer.size)
    }

    /// Doubles the atlas, copying what it holds into the top-left of the
    /// new texture. Draws already queued keep the old texture, and packed
    /// glyphs keep their texel positions.
    fn grow(&mut self, shared: &Shared) -> Result<bool> {
        let old = self.packer.size;
        if old >= MAX_SIZE {
            return Ok(false);
        }
        let texture = Self::make_texture(shared, self.kind, old * 2)?;
        let mut encoder = shared
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("yowie atlas grow"),
            });
        encoder.copy_texture_to_texture(
            self.texture.inner.texture.as_image_copy(),
            texture.inner.texture.as_image_copy(),
            wgpu::Extent3d {
                width: old,
                height: old,
                depth_or_array_layers: 1,
            },
        );
        shared.queue().submit([encoder.finish()]);
        self.texture = texture;
        self.packer.size = old * 2;
        Ok(true)
    }

    /// Packs a `w` by `h` glyph and uploads its texels, growing the atlas
    /// when it is full. Returns the glyph's top-left texel, or `None` when
    /// the atlas is at its largest and full.
    pub(crate) fn insert(
        &mut self,
        shared: &Shared,
        w: u32,
        h: u32,
        data: &[u8],
    ) -> Result<Option<(u32, u32)>> {
        let at = loop {
            if let Some(at) = self.packer.allocate(w, h) {
                break at;
            }
            if !self.grow(shared)? {
                return Ok(None);
            }
        };
        let bpp = match self.kind {
            AtlasKind::Mask | AtlasKind::Sdf => 1,
            AtlasKind::Color => 4,
        };
        if w > 0 && h > 0 {
            self.texture
                .write_level0(shared, at.0, at.1, w, h, data, bpp);
        }
        Ok(Some(at))
    }

    /// Forgets every packed glyph; the texture's contents are overwritten
    /// as glyphs are packed again.
    pub(crate) fn reset(&mut self) {
        self.packer = ShelfPacker::new(self.packer.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_pack_without_overlap() {
        let mut packer = ShelfPacker::new(64);
        let mut placed: Vec<(u32, u32, u32, u32)> = Vec::new();
        let sizes = [
            (10, 12),
            (10, 12),
            (30, 5),
            (20, 12),
            (40, 40),
            (60, 3),
            (70, 1),
        ];
        for (w, h) in sizes {
            let Some((x, y)) = packer.allocate(w, h) else {
                continue;
            };
            for &(px, py, pw, ph) in &placed {
                let apart = x + w + PAD <= px
                    || px + pw + PAD <= x
                    || y + h + PAD <= py
                    || py + ph + PAD <= y;
                assert!(apart, "{x},{y} {w}x{h} overlaps {px},{py} {pw}x{ph}");
            }
            assert!(x + w <= 64 && y + h <= 64);
            placed.push((x, y, w, h));
        }
        assert_eq!(placed.len(), 6, "only the 70-wide glyph cannot fit");
    }
}
