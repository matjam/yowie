//! Text: fonts, shaping, wrapping and alignment through cosmic-text, glyphs
//! rasterised by swash into atlases and drawn through the sprite batch.
//!
//! The engine bundles the Go fonts (`Go-Regular` and `Go-Mono`, BSD
//! licensed; see `fonts/README`) and loads no system fonts, so start-up is
//! quick and screenshots are the same on every machine. Load other fonts
//! with [`Font::from_bytes`].

mod atlas;
mod layout;

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::fontdb;
use cosmic_text::{CacheKey, FontSystem, SwashCache, SwashContent};
use yowie_core::{Color, Vec2, vec2};

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::pixels;
use crate::shared::Shared;
use atlas::{AtlasKind, GlyphAtlas};
use layout::LayoutCache;

const GO_REGULAR: &[u8] = include_bytes!("../../fonts/Go-Regular.ttf");
const GO_MONO: &[u8] = include_bytes!("../../fonts/Go-Mono.ttf");

/// A font face loaded into a renderer. Cloning is cheap; a font is only
/// valid with the [`Graphics`] that loaded it.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub(crate) face: fontdb::ID,
    pub(crate) family: Arc<str>,
    pub(crate) weight: fontdb::Weight,
    pub(crate) style: fontdb::Style,
    pub(crate) stretch: fontdb::Stretch,
}

impl Font {
    /// Loads a TrueType or OpenType font from its file's bytes. For a
    /// collection, the first face is used.
    pub fn from_bytes(gfx: &mut Graphics, bytes: Vec<u8>) -> Result<Font> {
        gfx.text.load(bytes)
    }

    /// The font's family name.
    pub fn family(&self) -> &str {
        &self.family
    }
}

/// How lines line up within the wrap width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Lines start at the left edge.
    #[default]
    Left,
    /// Lines are centred.
    Center,
    /// Lines end at the right edge.
    Right,
    /// Lines other than the last stretch to fill the width.
    Justified,
}

/// How text is set: size, line spacing, colour, wrap width and alignment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size in view units. Text with a size that is not positive and
    /// finite is not drawn.
    pub size: f32,
    /// Line height as a multiple of the size.
    pub line_height: f32,
    /// Text colour, sRGB with straight alpha.
    pub color: Color,
    /// Wrap lines to this width in view units; `None` breaks lines only at
    /// newlines.
    pub width: Option<f32>,
    /// Line alignment within `width` (or within the widest line).
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,
            line_height: 1.25,
            color: Color::WHITE,
            width: None,
            align: TextAlign::Left,
        }
    }
}

impl TextStyle {
    /// White, left-aligned, unwrapped text of `size` view units.
    pub fn new(size: f32) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// Sets the colour.
    pub fn color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    /// Sets the wrap width in view units.
    pub fn width(self, width: f32) -> Self {
        Self {
            width: Some(width),
            ..self
        }
    }

    /// Sets the alignment.
    pub fn align(self, align: TextAlign) -> Self {
        Self { align, ..self }
    }

    /// Sets the line height as a multiple of the size.
    pub fn line_height(self, line_height: f32) -> Self {
        Self {
            line_height,
            ..self
        }
    }

    fn drawable(&self) -> bool {
        self.size.is_finite() && self.size > 0.0 && self.line_height.is_finite()
    }
}

/// Where one rasterised glyph sits in an atlas.
#[derive(Clone, Copy, Debug)]
struct GlyphSlot {
    kind: AtlasKind,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

/// The font database, rasteriser, atlases and caches of one renderer.
pub(crate) struct TextSystem {
    fonts: FontSystem,
    swash: SwashCache,
    mask: GlyphAtlas,
    color: Option<GlyphAtlas>,
    glyphs: HashMap<CacheKey, Option<GlyphSlot>>,
    layouts: LayoutCache,
    regular: Font,
    mono: Font,
    full: bool,
}

impl TextSystem {
    pub(crate) fn new(shared: &Shared) -> Result<Self> {
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), fontdb::Database::new());
        let regular = load_font(&mut fonts, GO_REGULAR.to_vec())?;
        let mono = load_font(&mut fonts, GO_MONO.to_vec())?;
        Ok(Self {
            fonts,
            swash: SwashCache::new(),
            mask: GlyphAtlas::new(shared, AtlasKind::Mask)?,
            color: None,
            glyphs: HashMap::new(),
            layouts: LayoutCache::default(),
            regular,
            mono,
            full: false,
        })
    }

    fn load(&mut self, bytes: Vec<u8>) -> Result<Font> {
        load_font(&mut self.fonts, bytes)
    }

    /// Rasterises a glyph into its atlas the first time it is asked for.
    fn glyph(&mut self, shared: &Shared, key: CacheKey) -> Result<Option<GlyphSlot>> {
        if let Some(slot) = self.glyphs.get(&key) {
            return Ok(*slot);
        }
        let Some(image) = self.swash.get_image_uncached(&mut self.fonts, key) else {
            self.glyphs.insert(key, None);
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
                self.glyphs.insert(key, None);
                return Ok(None);
            }
        };
        let atlas = match kind {
            AtlasKind::Mask => &mut self.mask,
            AtlasKind::Color => match &mut self.color {
                Some(a) => a,
                none => none.insert(GlyphAtlas::new(shared, AtlasKind::Color)?),
            },
        };
        let Some((x, y)) = atlas.insert(shared, w, h, &data)? else {
            if !self.full {
                log::warn!("yowie-gfx: glyph atlas full; clearing it after this frame");
            }
            self.full = true;
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
        self.glyphs.insert(key, Some(slot));
        Ok(Some(slot))
    }

    /// Ends a frame: ages the layout cache and, if an atlas filled up,
    /// clears both atlases so the next frame packs only what it draws.
    pub(crate) fn end_frame(&mut self) {
        self.layouts.end_frame();
        if std::mem::take(&mut self.full) {
            self.mask.reset();
            if let Some(c) = &mut self.color {
                c.reset();
            }
            self.glyphs.clear();
        }
    }
}

/// Adds a font file's first face to the database.
fn load_font(fonts: &mut FontSystem, bytes: Vec<u8>) -> Result<Font> {
    let ids = fonts
        .db_mut()
        .load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
    let id = *ids.first().ok_or(GfxError::NoFontFace)?;
    let face = fonts.db().face(id).ok_or(GfxError::NoFontFace)?;
    let family = face
        .families
        .first()
        .map(|(name, _)| name.clone())
        .ok_or(GfxError::NoFontFace)?;
    Ok(Font {
        face: id,
        family: family.into(),
        weight: face.weight,
        style: face.style,
        stretch: face.stretch,
    })
}

impl Graphics {
    /// The bundled proportional font, Go Regular.
    pub fn default_font(&self) -> Font {
        self.text.regular.clone()
    }

    /// The bundled monospaced font, Go Mono.
    pub fn default_mono_font(&self) -> Font {
        self.text.mono.clone()
    }

    /// The size of `text` set in `font` and `style`, in view units: the
    /// widest line (or the wrap width, for aligned text) and the height of
    /// all lines.
    pub fn measure_text(&mut self, font: &Font, text: &str, style: &TextStyle) -> Vec2 {
        if !style.drawable() {
            return Vec2::ZERO;
        }
        let t = &mut self.text;
        t.layouts.get(&mut t.fonts, font, text, style).size
    }

    /// Queues `text` with its layout box's top-left corner at `position`.
    /// Glyphs rasterise at the output's pixel density, so text is sharp on
    /// high-density displays and inside fixed views.
    pub fn draw_text(&mut self, font: &Font, text: &str, position: Vec2, style: &TextStyle) {
        if !style.drawable() || text.is_empty() {
            return;
        }
        let layout = {
            let t = &mut self.text;
            t.layouts.get(&mut t.fonts, font, text, style)
        };
        let scale = self.pixels_per_unit().max(1e-3);
        // Without a camera, start on a whole pixel so glyphs keep the
        // sharpness they were rasterised with.
        let origin = if self.cursor.camera.is_none() {
            (position * scale).round()
        } else {
            position * scale
        };
        let mask_color = self.vertex_color(style.color);
        let color_tint = self.vertex_color(Color::WHITE.with_alpha(style.color.a));
        for placed in &layout.glyphs {
            let phys = placed.glyph.physical((0.0, placed.line_y * scale), scale);
            let slot = match self.text.glyph(&self.shared, phys.cache_key) {
                Ok(Some(s)) => s,
                Ok(None) => continue,
                Err(e) => {
                    log::error!("yowie-gfx: glyph upload failed: {e}");
                    continue;
                }
            };
            if slot.w == 0 || slot.h == 0 {
                continue;
            }
            let (texture, atlas_size, color, mode) = match slot.kind {
                AtlasKind::Mask => (
                    self.text.mask.texture.clone(),
                    self.text.mask.size(),
                    mask_color,
                    1,
                ),
                AtlasKind::Color => match &self.text.color {
                    Some(a) => (a.texture.clone(), a.size(), color_tint, 0),
                    None => continue,
                },
            };
            let x0 = origin.x + (phys.x + slot.left) as f32;
            let y0 = origin.y + (phys.y - slot.top) as f32;
            let (w, h) = (slot.w as f32, slot.h as f32);
            let corners = [
                vec2(x0, y0),
                vec2(x0 + w, y0),
                vec2(x0 + w, y0 + h),
                vec2(x0, y0 + h),
            ]
            .map(|p| p / scale);
            let a = atlas_size.as_vec2();
            let (u0, v0) = (slot.x as f32 / a.x, slot.y as f32 / a.y);
            let (u1, v1) = (
                (slot.x + slot.w) as f32 / a.x,
                (slot.y + slot.h) as f32 / a.y,
            );
            let uvs = [vec2(u0, v0), vec2(u1, v0), vec2(u1, v1), vec2(u0, v1)];
            self.push_quad(&texture, corners, uvs, color, mode);
        }
    }
}
