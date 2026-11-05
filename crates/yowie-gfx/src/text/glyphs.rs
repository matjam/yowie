//! Glyph rasterisation into the atlases: coverage masks, colour bitmaps
//! and signed distance tiles.

use std::collections::HashMap;

use cosmic_text::{CacheKey, CacheKeyFlags, SwashContent, fontdb};

use super::TextSystem;
use super::atlas::{AtlasKind, GlyphAtlas};
use super::sdf::{self, SOURCE_EM};
use crate::error::Result;
use crate::pixels;
use crate::shared::Shared;

/// Where one rasterised glyph sits in an atlas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphSlot {
    pub(crate) kind: AtlasKind,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) left: i32,
    pub(crate) top: i32,
}

/// Where one distance tile sits in the SDF atlas, with its placement in
/// tile texels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SdfSlot {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) left: f32,
    pub(crate) top: f32,
}

/// Glyphs already in the atlases. `None` records a glyph with no image.
#[derive(Debug, Default)]
pub(crate) struct GlyphCache {
    raster: HashMap<CacheKey, Option<GlyphSlot>>,
    sdf: HashMap<(fontdb::ID, u16, u16), Option<SdfSlot>>,
}

impl GlyphCache {
    pub(crate) fn clear(&mut self) {
        self.raster.clear();
        self.sdf.clear();
    }
}

impl TextSystem {
    /// Rasterises a glyph into its atlas the first time it is asked for.
    pub(crate) fn glyph(&mut self, shared: &Shared, key: CacheKey) -> Result<Option<GlyphSlot>> {
        if let Some(slot) = self.glyphs.raster.get(&key) {
            return Ok(*slot);
        }
        let Some(image) = self.swash.get_image_uncached(&mut self.fonts, key) else {
            self.glyphs.raster.insert(key, None);
            return Ok(None);
        };
        let (w, h) = (image.placement.width, image.placement.height);
        let (kind, data) = match image.content {
            SwashContent::Mask => (AtlasKind::Mask, image.data),
            SwashContent::Color => {
                let mut data = image.data;
                pixels::premultiply_srgb(&mut data);
                (AtlasKind::Color, data)
            }
            SwashContent::SubpixelMask => {
                // Subpixel masks are never requested; drop them rather than
                // draw them wrongly.
                self.glyphs.raster.insert(key, None);
                return Ok(None);
            }
        };
        let atlas = match kind {
            AtlasKind::Color => match &mut self.color {
                Some(a) => a,
                none => none.insert(GlyphAtlas::new(shared, AtlasKind::Color)?),
            },
            _ => &mut self.mask,
        };
        let Some((x, y)) = atlas.insert(shared, w, h, &data)? else {
            self.note_full();
            return Ok(None);
        };
        let slot = GlyphSlot {
            kind,
            x,
            y,
            w,
            h,
            left: image.placement.left,
            top: image.placement.top,
        };
        self.glyphs.raster.insert(key, Some(slot));
        Ok(Some(slot))
    }

    /// Builds a glyph's distance tile the first time it is asked for. A
    /// glyph with no outline (a bitmap emoji, a space) has none, and the
    /// caller falls back to the ordinary rasteriser.
    pub(crate) fn sdf_glyph(
        &mut self,
        shared: &Shared,
        font: fontdb::ID,
        glyph: u16,
        weight: fontdb::Weight,
    ) -> Result<Option<SdfSlot>> {
        let id = (font, glyph, weight.0);
        if let Some(slot) = self.glyphs.sdf.get(&id) {
            return Ok(*slot);
        }
        let (key, _, _) = CacheKey::new(
            font,
            glyph,
            SOURCE_EM,
            (0.0, 0.0),
            weight,
            CacheKeyFlags::DISABLE_HINTING,
        );
        let tile = self
            .swash
            .get_image_uncached(&mut self.fonts, key)
            .filter(|i| i.content == SwashContent::Mask)
            .and_then(|i| {
                sdf::tile(
                    &i.data,
                    i.placement.width,
                    i.placement.height,
                    i.placement.left,
                    i.placement.top,
                )
            });
        let Some(tile) = tile else {
            self.glyphs.sdf.insert(id, None);
            return Ok(None);
        };
        let atlas = match &mut self.sdf {
            Some(a) => a,
            none => none.insert(GlyphAtlas::new(shared, AtlasKind::Sdf)?),
        };
        let Some((x, y)) = atlas.insert(shared, tile.width, tile.height, &tile.data)? else {
            self.note_full();
            return Ok(None);
        };
        let slot = SdfSlot {
            x,
            y,
            w: tile.width,
            h: tile.height,
            left: tile.left,
            top: tile.top,
        };
        self.glyphs.sdf.insert(id, Some(slot));
        Ok(Some(slot))
    }

    fn note_full(&mut self) {
        if !self.full {
            log::warn!("yowie-gfx: glyph atlas full; clearing it after this frame");
        }
        self.full = true;
    }
}
