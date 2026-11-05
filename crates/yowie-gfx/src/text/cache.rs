//! Layouts drawn or measured recently, cached by content and style.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::fontdb;

use super::layout::LayoutData;
use super::{Font, Language, Orientation, TextAlign, TextStyle};

/// Frames a layout may go unused before the cache drops it.
const KEEP_FRAMES: u64 = 120;

/// Everything that decides a layout: the text, the font chain and the
/// style fields that affect shaping. Colour is not part of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    faces: Vec<fontdb::ID>,
    size: u32,
    line_height: u32,
    width: u32,
    align: TextAlign,
    hyphenate: Option<Language>,
    orientation: Orientation,
    decorations: (bool, bool),
    markup: bool,
    text: String,
}

impl Key {
    pub(crate) fn new(font: &Font, text: &str, markup: bool, style: &TextStyle) -> Self {
        Self {
            faces: font.chain_ids(),
            size: style.size.to_bits(),
            line_height: style.line_height.to_bits(),
            width: style.width.map_or(u32::MAX, f32::to_bits),
            align: style.align,
            hyphenate: style.hyphenate,
            orientation: style.orientation,
            decorations: (style.underline, style.strikethrough),
            markup,
            text: text.to_owned(),
        }
    }
}

#[derive(Debug)]
struct Entry {
    layout: Arc<LayoutData>,
    used: u64,
}

/// The cache of layouts.
#[derive(Debug, Default)]
pub(crate) struct LayoutCache {
    entries: HashMap<Key, Entry>,
    frame: u64,
}

impl LayoutCache {
    /// The cached layout for `key`, marking it used.
    pub(crate) fn lookup(&mut self, key: &Key) -> Option<Arc<LayoutData>> {
        let e = self.entries.get_mut(key)?;
        e.used = self.frame;
        Some(e.layout.clone())
    }

    /// Remembers `layout` for `key`.
    pub(crate) fn store(&mut self, key: Key, layout: Arc<LayoutData>) {
        self.entries.insert(
            key,
            Entry {
                layout,
                used: self.frame,
            },
        );
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
