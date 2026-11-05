//! Vertical text: upright glyphs stacked in columns, or whole lines turned
//! a quarter turn.

use yowie_core::vec2;

use super::layout::LayoutData;

/// Which way text runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Left to right (or right to left for scripts that need it), lines
    /// stacking downwards.
    #[default]
    Horizontal,
    /// Each glyph stands upright in a column, reading downwards; new lines
    /// start new columns to the left of the last, as in East Asian
    /// vertical writing. Wrap width is ignored.
    Upright,
    /// Each line is turned a quarter turn clockwise, so it reads
    /// downwards with the glyphs on their sides; new lines start columns
    /// to the left. Wrap width still applies, as a column height.
    Sideways,
}

/// Whether a character is set in a full-width square cell.
fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11ff | 0x2e80..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff
        | 0xfe30..=0xfe6f | 0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x20000..=0x3ffff)
}

/// Rearranges a horizontal layout of unwrapped lines into columns of
/// upright glyphs.
pub(crate) fn upright(data: &mut LayoutData) {
    let columns = data.lines.len();
    let col_w = data.lines.iter().map(|l| l.height).fold(0.0_f32, f32::max);
    let total_w = col_w * columns as f32;
    let mut tallest = 0.0_f32;
    for (c, line) in data.lines.iter_mut().enumerate() {
        let left = total_w - (c as f32 + 1.0) * col_w;
        let mut y = 0.0;
        for g in &mut data.glyphs[line.glyphs.clone()] {
            let ch = data.plain[g.start as usize..g.end as usize]
                .chars()
                .next()
                .unwrap_or(' ');
            let size = g.glyph.font_size;
            let cell = if is_wide(ch) {
                size
            } else {
                g.glyph.w.max(size * 0.6)
            };
            let x = left + (col_w - g.glyph.w) * 0.5;
            let baseline = y + cell * 0.5 + size * 0.35;
            g.dx = x - g.glyph.x;
            g.dy = baseline - g.line_y - g.glyph.y;
            y += cell;
        }
        tallest = tallest.max(y);
        line.top = left;
        line.height = col_w;
    }
    data.decos.clear();
    data.size = vec2(total_w, tallest);
}

/// Marks a layout to be drawn turned a quarter turn clockwise and swaps
/// its size.
pub(crate) fn sideways(data: &mut LayoutData) {
    data.sideways = true;
    data.size = vec2(data.size.y, data.size.x);
}
