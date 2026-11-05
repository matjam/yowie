//! Reusable text layouts with measurement and hit testing.

use std::ops::Range;
use std::sync::Arc;

use yowie_core::{Color, Vec2, vec2};

use super::layout::{GlyphRec, LayoutData, LineRec};

/// Text laid out once, to draw many times with
/// [`Graphics::draw_layout`](crate::Graphics::draw_layout), measure, and
/// ask where a point or a text offset falls. Make one with
/// [`Graphics::layout_text`](crate::Graphics::layout_text) or
/// [`Graphics::layout_rich`](crate::Graphics::layout_rich). Cloning is
/// cheap. A layout is only valid with the [`Graphics`](crate::Graphics)
/// that made it.
///
/// Positions are view units from the layout's top-left corner. Offsets
/// are byte offsets into [`plain`](Self::plain), always on `char`
/// boundaries. Hit testing follows horizontal text, including right-to-left
/// and mixed-direction lines; upright and sideways layouts report only
/// their size.
#[derive(Clone, Debug)]
pub struct TextLayout {
    pub(crate) data: Arc<LayoutData>,
    pub(crate) color: Color,
    pub(crate) sdf: bool,
}

/// Where a text cursor sits at some offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    /// The top of the caret, in view units from the layout's top-left.
    pub position: Vec2,
    /// The caret's height: the height of its line.
    pub height: f32,
}

impl TextLayout {
    /// The text with markup removed.
    pub fn plain(&self) -> &str {
        &self.data.plain
    }

    /// The layout's size: the widest line (or the wrap width, for aligned
    /// text) and the height of all lines.
    pub fn size(&self) -> Vec2 {
        self.data.size
    }

    /// Returns the layout drawn in `color` where spans do not set their
    /// own.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// The number of lines after wrapping.
    pub fn line_count(&self) -> usize {
        self.data.lines.len()
    }

    /// The byte range of line `index` in [`plain`](Self::plain), or `None`
    /// past the last line.
    pub fn line_range(&self, index: usize) -> Option<Range<usize>> {
        let l = self.data.lines.get(index)?;
        Some(l.start as usize..l.end as usize)
    }

    /// The offset in [`plain`](Self::plain) nearest to `point`: the
    /// position a click there should put the cursor. Points above or below
    /// the text clamp to the first or last line, and points left or right
    /// of a line to its ends.
    pub fn hit_test(&self, point: Vec2) -> usize {
        let d = &self.data;
        let Some(line) = d
            .lines
            .iter()
            .find(|l| point.y < l.top + l.height)
            .or(d.lines.last())
        else {
            return 0;
        };
        let glyphs = line_glyphs(d, line);
        let (Some(first), Some(last)) = (
            glyphs.iter().min_by(|a, b| a.left().total_cmp(&b.left())),
            glyphs.iter().max_by(|a, b| a.left().total_cmp(&b.left())),
        ) else {
            return line.start as usize;
        };
        let (logical_start, logical_end) = (line.start as usize, line.end as usize);
        if point.x < first.left() {
            return if line.rtl { logical_end } else { logical_start };
        }
        if point.x > last.left() + last.glyph.w {
            return if line.rtl { logical_start } else { logical_end };
        }
        let hit = glyphs
            .iter()
            .find(|g| point.x >= g.left() && point.x <= g.left() + g.glyph.w)
            .or_else(|| {
                glyphs
                    .iter()
                    .min_by(|a, b| distance_x(a, point.x).total_cmp(&distance_x(b, point.x)))
            });
        let Some(g) = hit else {
            return logical_start;
        };
        let mut frac = if g.glyph.w > 0.0 {
            ((point.x - g.left()) / g.glyph.w).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if g.glyph.level.is_rtl() {
            frac = 1.0 - frac;
        }
        offset_in_cluster(&d.plain, g, frac)
    }

    /// Where the cursor sits at byte `offset` of [`plain`](Self::plain),
    /// clamped to the text. At a line break the offset belongs to the next
    /// line.
    pub fn caret(&self, offset: usize) -> Caret {
        let d = &self.data;
        let offset = offset.min(d.plain.len());
        // The last line that starts at or before the offset: an offset in
        // the space a wrap consumed belongs to the end of the line before.
        let Some(line) = d
            .lines
            .iter()
            .rev()
            .find(|l| l.start as usize <= offset)
            .or(d.lines.first())
        else {
            return Caret {
                position: Vec2::ZERO,
                height: 0.0,
            };
        };
        let glyphs = line_glyphs(d, line);
        let x = if let Some(g) = glyphs
            .iter()
            .find(|g| (g.start as usize) <= offset && offset < g.end as usize)
        {
            let n = d.plain[g.start as usize..g.end as usize]
                .chars()
                .count()
                .max(1);
            let before = d.plain[g.start as usize..offset].chars().count();
            let mut frac = before as f32 / n as f32;
            if g.glyph.level.is_rtl() {
                frac = 1.0 - frac;
            }
            g.left() + g.glyph.w * frac
        } else if line.rtl {
            glyphs
                .iter()
                .map(|g| g.left())
                .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.min(v))))
                .unwrap_or(0.0)
        } else {
            glyphs
                .iter()
                .map(|g| g.left() + g.glyph.w)
                .fold(0.0, f32::max)
        };
        Caret {
            position: vec2(x, line.top),
            height: line.height,
        }
    }

    /// The link name of the span under `point`, if any.
    pub fn link_at(&self, point: Vec2) -> Option<&str> {
        let d = &self.data;
        let line = d
            .lines
            .iter()
            .find(|l| point.y >= l.top && point.y < l.top + l.height)?;
        let g = line_glyphs(d, line)
            .into_iter()
            .find(|g| point.x >= g.left() && point.x <= g.left() + g.glyph.w)?;
        d.spans.get(g.span as usize)?.link.as_deref()
    }
}

fn line_glyphs<'a>(d: &'a LayoutData, line: &LineRec) -> Vec<&'a GlyphRec> {
    d.glyphs[line.glyphs.clone()]
        .iter()
        .filter(|g| g.end > g.start)
        .collect()
}

fn distance_x(g: &GlyphRec, x: f32) -> f32 {
    if x < g.left() {
        g.left() - x
    } else {
        (x - g.left() - g.glyph.w).max(0.0)
    }
}

/// The offset `frac` of the way through a glyph's cluster, by character.
fn offset_in_cluster(plain: &str, g: &GlyphRec, frac: f32) -> usize {
    let cluster = &plain[g.start as usize..g.end as usize];
    let n = cluster.chars().count().max(1);
    let k = (frac * n as f32).round() as usize;
    let within = cluster
        .char_indices()
        .nth(k)
        .map_or(cluster.len(), |(i, _)| i);
    g.start as usize + within
}
