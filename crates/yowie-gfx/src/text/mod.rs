//! Text: fonts, shaping, wrapping, alignment, rich markup, hyphenation,
//! bidirectional and vertical layout through cosmic-text; glyphs
//! rasterised by swash (including colour glyphs) or drawn from signed
//! distance fields, through the sprite batch.
//!
//! The engine bundles the Go fonts (regular, bold, italic and mono; BSD
//! licensed, see `fonts/README`) and loads no system fonts unless a game
//! asks, so start-up is quick and screenshots are the same on every
//! machine. Load other fonts with [`Font::from_bytes`]; chain fallbacks
//! with [`Font::with_fallback`].

mod atlas;
mod cache;
mod draw;
mod glyphs;
mod hit;
mod hyphen;
mod layout;
mod markup;
mod sdf;
mod vertical;

use std::sync::Arc;

use cosmic_text::{FontSystem, SwashCache, fontdb};
use yowie_core::{Color, Vec2};

use crate::Graphics;
use crate::error::{GfxError, Result};
use crate::shared::Shared;
use atlas::{AtlasKind, GlyphAtlas};
use cache::LayoutCache;
use layout::{LayoutData, ShapeSpec};

pub use hit::{Caret, TextLayout};
pub use hyphen::Language;
pub use markup::{RichText, SpanStyle};
pub use vertical::Orientation;

const GO_REGULAR: &[u8] = include_bytes!("../../fonts/Go-Regular.ttf");
const GO_BOLD: &[u8] = include_bytes!("../../fonts/Go-Bold.ttf");
const GO_ITALIC: &[u8] = include_bytes!("../../fonts/Go-Italic.ttf");
const GO_MONO: &[u8] = include_bytes!("../../fonts/Go-Mono.ttf");

/// A font face loaded into a renderer, optionally with a chain of fallback
/// fonts. Cloning is cheap; a font is only valid with the [`Graphics`]
/// that loaded it.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub(crate) face: fontdb::ID,
    pub(crate) family: Arc<str>,
    pub(crate) weight: fontdb::Weight,
    pub(crate) style: fontdb::Style,
    pub(crate) stretch: fontdb::Stretch,
    pub(crate) fallback: Arc<[Font]>,
}

impl Font {
    /// Loads a TrueType or OpenType font from its file's bytes. For a
    /// collection, the first face is used. Colour fonts (COLR, and bitmap
    /// fonts such as CBDT and sbix) draw in colour; the system and
    /// outline-only fonts draw as masks tinted by the text colour.
    pub fn from_bytes(gfx: &mut Graphics, bytes: Vec<u8>) -> Result<Font> {
        gfx.text.load(bytes)
    }

    /// Loads a font file from `path`.
    pub fn from_file(gfx: &mut Graphics, path: impl AsRef<std::path::Path>) -> Result<Font> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| GfxError::FontFile(e.to_string()))?;
        gfx.text.load(bytes)
    }

    /// The font's family name.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// This font followed by `fallback`, tried in order for each character
    /// the earlier fonts have no glyph for. Text set in the result uses the
    /// first font in the chain that covers each character; spaces and
    /// combining marks stay with the character before them. The fallback
    /// fonts' own chains are ignored.
    pub fn with_fallback(&self, fallback: &[Font]) -> Font {
        Font {
            fallback: fallback
                .iter()
                .map(|f| Font {
                    fallback: Arc::from([]),
                    ..f.clone()
                })
                .collect(),
            ..self.clone()
        }
    }

    pub(crate) fn chain_ids(&self) -> Vec<fontdb::ID> {
        std::iter::once(self.face)
            .chain(self.fallback.iter().map(|f| f.face))
            .collect()
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

/// How text is set: size, line spacing, colour, wrap width, alignment and
/// the optional features of the text renderer.
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
    /// newlines. Wrapping follows Unicode line breaking, and breaks inside
    /// a word that is too long for a line.
    pub width: Option<f32>,
    /// Line alignment within `width` (or within the widest line).
    pub align: TextAlign,
    /// Breaks long words at hyphenation points, with a hyphen, when
    /// wrapping. `None` breaks only between words.
    pub hyphenate: Option<Language>,
    /// Horizontal, upright vertical or sideways vertical text.
    pub orientation: Orientation,
    /// Draws glyphs from signed distance fields: sharp at any size and
    /// under any transform, at the cost of a little detail on small text.
    pub sdf: bool,
    /// Underlines the whole text.
    pub underline: bool,
    /// Strikes through the whole text.
    pub strikethrough: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,
            line_height: 1.25,
            color: Color::WHITE,
            width: None,
            align: TextAlign::Left,
            hyphenate: None,
            orientation: Orientation::Horizontal,
            sdf: false,
            underline: false,
            strikethrough: false,
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

    /// Hyphenates wrapped words with the patterns of `language`.
    pub fn hyphenate(self, language: Language) -> Self {
        Self {
            hyphenate: Some(language),
            ..self
        }
    }

    /// Sets the orientation.
    pub fn orientation(self, orientation: Orientation) -> Self {
        Self {
            orientation,
            ..self
        }
    }

    /// Turns signed-distance-field drawing on or off.
    pub fn sdf(self, sdf: bool) -> Self {
        Self { sdf, ..self }
    }

    /// Underlines the text.
    pub fn underline(self) -> Self {
        Self {
            underline: true,
            ..self
        }
    }

    /// Strikes the text through.
    pub fn strikethrough(self) -> Self {
        Self {
            strikethrough: true,
            ..self
        }
    }

    pub(crate) fn drawable(&self) -> bool {
        self.size.is_finite() && self.size > 0.0 && self.line_height.is_finite()
    }
}

/// The font database, rasteriser, atlases and caches of one renderer.
pub(crate) struct TextSystem {
    pub(crate) fonts: FontSystem,
    pub(crate) swash: SwashCache,
    pub(crate) mask: GlyphAtlas,
    pub(crate) color: Option<GlyphAtlas>,
    pub(crate) sdf: Option<GlyphAtlas>,
    pub(crate) glyphs: glyphs::GlyphCache,
    pub(crate) layouts: LayoutCache,
    pub(crate) hyph: hyphen::Hyphenators,
    regular: Font,
    mono: Font,
    pub(crate) full: bool,
}

impl TextSystem {
    pub(crate) fn new(shared: &Shared) -> Result<Self> {
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), fontdb::Database::new());
        let regular = load_font(&mut fonts, GO_REGULAR.to_vec())?;
        load_font(&mut fonts, GO_BOLD.to_vec())?;
        load_font(&mut fonts, GO_ITALIC.to_vec())?;
        let mono = load_font(&mut fonts, GO_MONO.to_vec())?;
        Ok(Self {
            fonts,
            swash: SwashCache::new(),
            mask: GlyphAtlas::new(shared, AtlasKind::Mask)?,
            color: None,
            sdf: None,
            glyphs: glyphs::GlyphCache::default(),
            layouts: LayoutCache::default(),
            hyph: hyphen::Hyphenators::default(),
            regular,
            mono,
            full: false,
        })
    }

    fn load(&mut self, bytes: Vec<u8>) -> Result<Font> {
        load_font(&mut self.fonts, bytes)
    }

    /// Ends a frame: ages the layout cache and, if an atlas filled up,
    /// clears the atlases so the next frame packs only what it draws.
    pub(crate) fn end_frame(&mut self) {
        self.layouts.end_frame();
        if std::mem::take(&mut self.full) {
            self.mask.reset();
            for a in [&mut self.color, &mut self.sdf].into_iter().flatten() {
                a.reset();
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
    font_for(fonts, ids.first().copied())
}

fn font_for(fonts: &FontSystem, id: Option<fontdb::ID>) -> Result<Font> {
    let id = id.ok_or(GfxError::NoFontFace)?;
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
        fallback: Arc::from([]),
    })
}

/// Files that commonly hold the system's colour emoji font.
const EMOJI_FONT_FILES: &[&str] = &[
    "/System/Library/Fonts/Apple Color Emoji.ttc",
    "C:\\Windows\\Fonts\\seguiemj.ttf",
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
    "/usr/share/fonts/google-noto-color-emoji-fonts/Noto-COLRv1.ttf",
];

impl Graphics {
    /// The bundled proportional font, Go Regular. Its bold and italic
    /// faces are bundled too: spans marked bold or italic use them.
    pub fn default_font(&self) -> Font {
        self.text.regular.clone()
    }

    /// The bundled monospaced font, Go Mono.
    pub fn default_mono_font(&self) -> Font {
        self.text.mono.clone()
    }

    /// Loads every font installed on the system into the font database, so
    /// that [`Font::with_fallback`] chains and spans naming a family
    /// (`[font=Menlo]`) can reach them. This scans the system's font
    /// directories and takes noticeable time; call it once at start-up, and
    /// only when the game wants system fonts. Output then depends on the
    /// machine's fonts.
    pub fn load_system_fonts(&mut self) {
        self.text.fonts.db_mut().load_system_fonts();
    }

    /// The installed font family `family` as a [`Font`], loading the system
    /// fonts first if needed. `None` when no such family exists.
    pub fn system_font(&mut self, family: &str) -> Option<Font> {
        let db = self.text.fonts.db_mut();
        if db.faces().next().is_some_and(|_| db.len() <= 4) {
            db.load_system_fonts();
        }
        let id = self.text.fonts.db().query(&fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            ..fontdb::Query::default()
        })?;
        font_for(&self.text.fonts, Some(id)).ok()
    }

    /// Loads the operating system's colour emoji font (Apple Color Emoji,
    /// Segoe UI Emoji or Noto Color Emoji) from its usual location. `None`
    /// when it is not installed; a game that needs emoji everywhere should
    /// ship a colour font and load it with [`Font::from_bytes`].
    pub fn system_emoji_font(&mut self) -> Option<Font> {
        for path in EMOJI_FONT_FILES {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            if let Ok(font) = self.text.load(bytes) {
                return Some(font);
            }
        }
        None
    }

    /// Lays out plain `text` in `font` and `style`. The layout is cached
    /// by content, so asking again for the same text is cheap, but holding
    /// the result avoids even the lookup.
    pub fn layout_text(&mut self, font: &Font, text: &str, style: &TextStyle) -> TextLayout {
        self.cached_layout(font, text, false, style)
    }

    /// Lays out text written in the bracket markup described on
    /// [`RichText`]. Cached like [`layout_text`](Self::layout_text).
    pub fn layout_markup(&mut self, font: &Font, markup: &str, style: &TextStyle) -> TextLayout {
        self.cached_layout(font, markup, true, style)
    }

    fn cached_layout(
        &mut self,
        font: &Font,
        text: &str,
        markup: bool,
        style: &TextStyle,
    ) -> TextLayout {
        if !style.drawable() {
            return self.empty_layout(style);
        }
        let t = &mut self.text;
        let key = cache::Key::new(font, text, markup, style);
        let data = match t.layouts.lookup(&key) {
            Some(d) => d,
            None => {
                let flat = text.replace('\r', "");
                let mut rich = if markup {
                    RichText::parse(&flat)
                } else {
                    let mut r = RichText::new();
                    r.push(&flat, SpanStyle::default());
                    r
                };
                apply_style_flags(&mut rich, style);
                let d = Arc::new(layout_with(&mut t.fonts, &mut t.hyph, font, &rich, style));
                t.layouts.store(key, d.clone());
                d
            }
        };
        TextLayout {
            data,
            color: style.color,
            sdf: style.sdf,
        }
    }

    /// Lays out styled text built by hand. Not cached: hold the layout.
    pub fn layout_rich(&mut self, font: &Font, text: &RichText, style: &TextStyle) -> TextLayout {
        if !style.drawable() {
            return self.empty_layout(style);
        }
        let t = &mut self.text;
        let mut rich = text.clone();
        apply_style_flags(&mut rich, style);
        let data = Arc::new(layout_with(&mut t.fonts, &mut t.hyph, font, &rich, style));
        TextLayout {
            data,
            color: style.color,
            sdf: style.sdf,
        }
    }

    fn empty_layout(&self, style: &TextStyle) -> TextLayout {
        TextLayout {
            data: Arc::new(LayoutData::default()),
            color: style.color,
            sdf: style.sdf,
        }
    }

    /// The size of `text` set in `font` and `style`, in view units: the
    /// widest line (or the wrap width, for aligned text) and the height of
    /// all lines.
    pub fn measure_text(&mut self, font: &Font, text: &str, style: &TextStyle) -> Vec2 {
        self.layout_text(font, text, style).size()
    }

    /// Like [`measure_text`](Self::measure_text) for bracket markup.
    pub fn measure_markup(&mut self, font: &Font, markup: &str, style: &TextStyle) -> Vec2 {
        self.layout_markup(font, markup, style).size()
    }

    /// Queues `text` with its layout box's top-left corner at `position`.
    /// Glyphs rasterise at the output's pixel density, so text is sharp on
    /// high-density displays and inside fixed views; with
    /// [`TextStyle::sdf`] they are sharp at any scale.
    pub fn draw_text(&mut self, font: &Font, text: &str, position: Vec2, style: &TextStyle) {
        if text.is_empty() {
            return;
        }
        let layout = self.layout_text(font, text, style);
        self.draw_layout(&layout, position);
    }

    /// Queues text written in bracket markup; see [`RichText`].
    pub fn draw_markup(&mut self, font: &Font, markup: &str, position: Vec2, style: &TextStyle) {
        if markup.is_empty() {
            return;
        }
        let layout = self.layout_markup(font, markup, style);
        self.draw_layout(&layout, position);
    }
}

fn apply_style_flags(rich: &mut RichText, style: &TextStyle) {
    if style.underline || style.strikethrough {
        let mut flagged = RichText::new();
        for (range, s) in rich.spans() {
            let mut s = s.clone();
            s.underline |= style.underline;
            s.strikethrough |= style.strikethrough;
            flagged.push(&rich.plain()[range.clone()], s);
        }
        *rich = flagged;
    }
}

fn layout_with(
    fonts: &mut FontSystem,
    hyph: &mut hyphen::Hyphenators,
    font: &Font,
    rich: &RichText,
    style: &TextStyle,
) -> LayoutData {
    let upright = style.orientation == Orientation::Upright;
    let spec = ShapeSpec {
        font,
        text: rich,
        size: style.size,
        line_height: style.line_height,
        width: if upright { None } else { style.width },
        align: style.align,
        hyphenate: style.hyphenate,
    };
    let mut data = layout::shape(fonts, hyph, &spec);
    match style.orientation {
        Orientation::Horizontal => {}
        Orientation::Upright => vertical::upright(&mut data),
        Orientation::Sideways => vertical::sideways(&mut data),
    }
    data
}
