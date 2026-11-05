//! Shaped, wrapped and aligned text, cached by content and style.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cosmic_text::{Attrs, Buffer, FontSystem, LayoutGlyph, Metrics, Shaping};
use yowie_core::{Vec2, vec2};

use super::{Font, TextAlign, TextStyle};

/// Frames a layout may go unused before the cache drops it.
const KEEP_FRAMES: u64 = 120;

/// One glyph placed in a layout, with the baseline of its line.
#[derive(Clone, Debug)]
pub(crate) struct PlacedGlyph {
    pub(crate) glyph: LayoutGlyph,
    pub(crate) line_y: f32,
}

/// Text laid out at one size, in view units from the top-left.
#[derive(Clone, Debug)]
pub(crate) struct TextLayout {
    pub(crate) glyphs: Vec<PlacedGlyph>,
    /// The widest line and the total height, in view units.
    pub(crate) size: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LayoutKey {
    face: cosmic_text::fontdb::ID,
    size: u32,
    line_height: u32,
    width: u32,
    align: TextAlign,
}

#[derive(Debug)]
struct Entry {
    key: LayoutKey,
    text: String,
    layout: Arc<TextLayout>,
    used: u64,
}

/// Layouts drawn or measured recently.
#[derive(Debug, Default)]
pub(crate) struct LayoutCache {
    entries: HashMap<u64, Entry>,
    frame: u64,
}

fn shape(fonts: &mut FontSystem, font: &Font, text: &str, style: &TextStyle) -> TextLayout {
    let line_height = style.size * style.line_height.max(0.01);
    let mut buffer = Buffer::new_empty(Metrics::new(style.size, line_height));
    buffer.set_size(style.width.map(|w| w.max(0.0)), None);
    let attrs = Attrs {
        family: cosmic_text::Family::Name(&font.family),
        weight: font.weight,
        style: font.style,
        stretch: font.stretch,
        ..Attrs::new()
    };
    let align = match style.align {
        TextAlign::Left => cosmic_text::Align::Left,
        TextAlign::Center => cosmic_text::Align::Center,
        TextAlign::Right => cosmic_text::Align::Right,
        TextAlign::Justified => cosmic_text::Align::Justified,
    };
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(fonts, false);
    let mut glyphs = Vec::new();
    let mut size = Vec2::ZERO;
    for run in buffer.layout_runs() {
        glyphs.extend(run.glyphs.iter().map(|g| PlacedGlyph {
            glyph: g.clone(),
            line_y: run.line_y,
        }));
        size = vec2(size.x.max(run.line_w), run.line_top + run.line_height);
    }
    if let Some(w) = style.width
        && style.align != TextAlign::Left
    {
        size.x = size.x.max(w);
    }
    TextLayout { glyphs, size }
}

impl LayoutCache {
    /// The layout of `text` in `font` and `style`, shaping it on a miss.
    /// The style's size must be positive and finite.
    pub(crate) fn get(
        &mut self,
        fonts: &mut FontSystem,
        font: &Font,
        text: &str,
        style: &TextStyle,
    ) -> Arc<TextLayout> {
        let key = LayoutKey {
            face: font.face,
            size: style.size.to_bits(),
            line_height: style.line_height.to_bits(),
            width: style.width.map_or(u32::MAX, f32::to_bits),
            align: style.align,
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        text.hash(&mut h);
        let hash = h.finish();
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&hash)
            && e.key == key
            && e.text == text
        {
            e.used = frame;
            return e.layout.clone();
        }
        let layout = Arc::new(shape(fonts, font, text, style));
        self.entries.insert(
            hash,
            Entry {
                key,
                text: text.to_owned(),
                layout: layout.clone(),
                used: frame,
            },
        );
        layout
    }

    /// Advances the frame count and drops layouts unused for a while.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        if self.frame.is_multiple_of(30) {
            let frame = self.frame;
            self.entries
                .retain(|_, e| frame.saturating_sub(e.used) < KEEP_FRAMES);
        }
    }
}
