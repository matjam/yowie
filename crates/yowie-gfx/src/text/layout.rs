//! Shaped, wrapped and aligned text: the layout data and the code that
//! builds it from cosmic-text.

use std::ops::Range;
use std::sync::Arc;

use cosmic_text::{
    Align, Attrs, Buffer, Family, FontSystem, LayoutGlyph, Metrics, Shaping, TextDecoration,
    UnderlineStyle, Weight, Wrap, fontdb,
};
use yowie_core::{Color, Rect, Vec2, vec2};

use super::hyphen::{Hyphenators, Language};
use super::markup::RichText;
use super::{Font, TextAlign};

/// A soft hyphen, which the shaper may break a line after.
const SHY: char = '\u{ad}';

/// One glyph placed in a layout.
#[derive(Clone, Debug)]
pub(crate) struct GlyphRec {
    pub(crate) glyph: LayoutGlyph,
    /// Baseline of the glyph's line, before `dy`.
    pub(crate) line_y: f32,
    /// Offsets added by hyphenation, alignment and vertical layout.
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    /// The span's colour, if it has one.
    pub(crate) color: Option<Color>,
    /// Index into the layout's spans.
    pub(crate) span: u32,
    /// Byte range of the glyph's cluster in the plain text.
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) hyphen: bool,
}

impl GlyphRec {
    /// Left edge of the glyph's box in layout space.
    pub(crate) fn left(&self) -> f32 {
        self.glyph.x + self.dx
    }
}

/// One line of a layout.
#[derive(Clone, Debug)]
pub(crate) struct LineRec {
    pub(crate) top: f32,
    pub(crate) height: f32,
    pub(crate) width: f32,
    pub(crate) rtl: bool,
    pub(crate) glyphs: Range<usize>,
    /// Byte range of the line in the plain text.
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// An underline or strikethrough, in layout space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DecoRect {
    pub(crate) rect: Rect,
    pub(crate) color: Option<Color>,
}

/// The span data a layout keeps for drawing and hit testing.
#[derive(Clone, Debug, Default)]
pub(crate) struct SpanInfo {
    pub(crate) link: Option<Arc<str>>,
}

/// A laid-out text, in view units from its top-left corner.
#[derive(Clone, Debug, Default)]
pub(crate) struct LayoutData {
    pub(crate) glyphs: Vec<GlyphRec>,
    pub(crate) lines: Vec<LineRec>,
    pub(crate) decos: Vec<DecoRect>,
    pub(crate) spans: Vec<SpanInfo>,
    pub(crate) plain: String,
    pub(crate) size: Vec2,
    /// Whether the layout is turned a quarter turn when drawn.
    pub(crate) sideways: bool,
}

/// What to lay out and how.
pub(crate) struct ShapeSpec<'a> {
    pub(crate) font: &'a Font,
    pub(crate) text: &'a RichText,
    pub(crate) size: f32,
    pub(crate) line_height: f32,
    pub(crate) width: Option<f32>,
    pub(crate) align: TextAlign,
    pub(crate) hyphenate: Option<Language>,
}

fn from_cosmic(c: cosmic_text::Color) -> Color {
    let (r, g, b, a) = c.as_rgba_tuple();
    Color::rgba8(r, g, b, a)
}

/// A stretch of one span set in one face of the fallback chain.
struct Piece {
    range: Range<usize>,
    span: usize,
    /// Index into the font chain; 0 is the layout's font.
    face: usize,
}

/// Splits spans into pieces that each have one face, choosing for every
/// character the first font in `chain` that has a glyph for it. Spaces,
/// marks and joiners stay with the character before them.
fn split_pieces(fonts: &mut FontSystem, spec: &ShapeSpec, chain: &[&Font]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for (i, (range, style)) in spec.text.spans().iter().enumerate() {
        if chain.len() < 2 || style.font.is_some() {
            pieces.push(Piece {
                range: range.clone(),
                span: i,
                face: 0,
            });
            continue;
        }
        let text = &spec.text.plain()[range.clone()];
        let mut current = 0;
        let mut start = range.start;
        for (at, c) in text.char_indices() {
            let sticky = c.is_whitespace()
                || c.is_control()
                || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{300}'..='\u{36f}');
            let face = if sticky {
                current
            } else {
                chain
                    .iter()
                    .position(|f| has_glyph(fonts, f, c))
                    .unwrap_or(0)
            };
            if face != current {
                let at = range.start + at;
                if at > start {
                    pieces.push(Piece {
                        range: start..at,
                        span: i,
                        face: current,
                    });
                    start = at;
                }
                current = face;
            }
        }
        pieces.push(Piece {
            range: start..range.end,
            span: i,
            face: current,
        });
    }
    pieces
}

fn has_glyph(fonts: &mut FontSystem, font: &Font, c: char) -> bool {
    fonts
        .get_font(font.face, font.weight)
        .is_some_and(|f| f.as_swash().charmap().map(c) != 0)
}

/// The glyph and advance for a hyphen in `font_id` at `size`.
fn hyphen_glyph(
    fonts: &mut FontSystem,
    font_id: fontdb::ID,
    weight: fontdb::Weight,
    size: f32,
) -> Option<(u16, f32)> {
    let font = fonts.get_font(font_id, weight)?;
    let swash = font.as_swash();
    let gid = swash.charmap().map('-');
    if gid == 0 {
        return None;
    }
    let advance = swash.glyph_metrics(&[]).scale(size).advance_width(gid);
    Some((gid, advance))
}

/// Everything one pass through the shaper produced.
struct Pass {
    glyphs: Vec<GlyphRec>,
    shy: Vec<bool>,
    lines: Vec<LineRec>,
    decos: Vec<DecoRect>,
    size: Vec2,
}

/// Shapes and wraps with soft hyphens inserted at `points` (byte offsets
/// in the plain text).
fn run_pass(
    fonts: &mut FontSystem,
    spec: &ShapeSpec,
    chain: &[&Font],
    pieces: &[Piece],
    points: &[usize],
) -> Pass {
    let plain = spec.text.plain();
    let mut shaped = String::with_capacity(plain.len() + points.len() * 2);
    let mut last = 0;
    for &p in points {
        shaped.push_str(&plain[last..p]);
        shaped.push(SHY);
        last = p;
    }
    shaped.push_str(&plain[last..]);
    let to_shaped = |p: usize| p + 2 * points.partition_point(|&q| q <= p);
    let to_plain = |q: usize| {
        let before = points
            .iter()
            .enumerate()
            .take_while(|(k, p)| **p + 2 * k < q)
            .count();
        q - 2 * before
    };

    let base = spec.font;
    let attrs_for = |piece: &Piece| -> Attrs<'_> {
        let style = &spec.text.spans()[piece.span].1;
        let family = match &style.font {
            Some(name) => Family::Name(name),
            None => Family::Name(&chain[piece.face].family),
        };
        let mut a = Attrs::new()
            .family(family)
            .weight(if style.bold {
                Weight::BOLD
            } else {
                base.weight
            })
            .style(if style.italic {
                cosmic_text::Style::Italic
            } else {
                base.style
            })
            .stretch(base.stretch)
            .metadata(piece.span);
        if let Some(c) = style.color {
            let [r, g, b, al] = c.to_rgba8();
            a = a.color(cosmic_text::Color::rgba(r, g, b, al));
        }
        if let Some(s) = style.size {
            a = a.metrics(Metrics::new(s, s * spec.line_height.max(0.01)));
        }
        if style.underline || style.strikethrough {
            a.text_decoration = TextDecoration {
                underline: if style.underline {
                    UnderlineStyle::Single
                } else {
                    UnderlineStyle::None
                },
                strikethrough: style.strikethrough,
                ..TextDecoration::new()
            };
        }
        a
    };
    let rich: Vec<(&str, Attrs<'_>)> = pieces
        .iter()
        .filter(|p| !p.range.is_empty())
        .map(|p| {
            let (a, b) = (to_shaped(p.range.start), to_shaped(p.range.end));
            (&shaped[a..b], attrs_for(p))
        })
        .collect();

    let line_height = spec.size * spec.line_height.max(0.01);
    let mut buffer = Buffer::new_empty(Metrics::new(spec.size, line_height));
    buffer.set_size(spec.width.map(|w| w.max(0.0)), None);
    buffer.set_wrap(if spec.width.is_some() {
        Wrap::WordOrGlyph
    } else {
        Wrap::None
    });
    let align = match spec.align {
        TextAlign::Left => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::Right,
        TextAlign::Justified => Align::Justified,
    };
    let default = Attrs::new()
        .family(Family::Name(&base.family))
        .weight(base.weight)
        .style(base.style)
        .stretch(base.stretch);
    buffer.set_rich_text(rich, &default, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(fonts, false);

    let mut para_start = Vec::new();
    let mut at = 0;
    for line in shaped.split('\n') {
        para_start.push(at);
        at += line.len() + 1;
    }
    let mut pass = Pass {
        glyphs: Vec::new(),
        shy: Vec::new(),
        lines: Vec::new(),
        decos: Vec::new(),
        size: Vec2::ZERO,
    };
    for run in buffer.layout_runs() {
        let para = para_start.get(run.line_i).copied().unwrap_or(0);
        let first = pass.glyphs.len();
        for g in run.glyphs {
            let (s, e) = (para + g.start, para + g.end);
            pass.shy.push(shaped.get(s..e) == Some("\u{ad}"));
            pass.glyphs.push(GlyphRec {
                glyph: g.clone(),
                line_y: run.line_y,
                dx: 0.0,
                dy: 0.0,
                color: g.color_opt.map(from_cosmic),
                span: g.metadata as u32,
                start: to_plain(s) as u32,
                end: to_plain(e) as u32,
                hyphen: false,
            });
        }
        for d in run.decorations {
            let glyphs = &run.glyphs[d.glyph_range.clone()];
            let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
            for g in glyphs {
                x0 = x0.min(g.x);
                x1 = x1.max(g.x + g.w);
            }
            if x1 <= x0 {
                continue;
            }
            let td = &d.data.text_decoration;
            let color = d.color_opt.map(from_cosmic);
            let size = d.font_size;
            if td.underline != UnderlineStyle::None {
                let m = d.data.underline_metrics;
                let th = (m.thickness * size).max(1.0);
                let y = run.line_y - m.offset * size;
                pass.decos.push(DecoRect {
                    rect: Rect::new(x0, y, x1 - x0, th),
                    color: td.underline_color_opt.map(from_cosmic).or(color),
                });
            }
            if td.strikethrough {
                let m = d.data.strikethrough_metrics;
                let th = (m.thickness * size).max(1.0);
                let y = run.line_y - m.offset * size;
                pass.decos.push(DecoRect {
                    rect: Rect::new(x0, y, x1 - x0, th),
                    color: td.strikethrough_color_opt.map(from_cosmic).or(color),
                });
            }
        }
        let own = &pass.glyphs[first..];
        let (start, end) = match (
            own.iter()
                .filter(|g| g.end > g.start)
                .map(|g| g.start)
                .min(),
            own.iter().map(|g| g.end).max(),
        ) {
            (Some(s), Some(e)) => (s, e),
            _ => {
                let p = to_plain(para) as u32;
                (p, p)
            }
        };
        pass.lines.push(LineRec {
            top: run.line_top,
            height: run.line_height,
            width: run.line_w,
            rtl: run.rtl,
            glyphs: first..pass.glyphs.len(),
            start,
            end,
        });
        pass.size = vec2(pass.size.x.max(run.line_w), run.line_top + run.line_height);
    }
    pass
}

/// Lays out `spec`, inserting hyphens where the language allows and the
/// line has room for one.
pub(crate) fn shape(
    fonts: &mut FontSystem,
    hyph: &mut Hyphenators,
    spec: &ShapeSpec,
) -> LayoutData {
    let mut chain: Vec<&Font> = vec![spec.font];
    chain.extend(spec.font.fallback.iter());
    let pieces = split_pieces(fonts, spec, &chain);
    let plain = spec.text.plain();
    let mut points = match (spec.hyphenate, spec.width) {
        (Some(lang), Some(_)) => hyph.points(lang, plain),
        _ => Vec::new(),
    };
    let mut pass = run_pass(fonts, spec, &chain, &pieces, &points);
    // A hyphen needs room on its line. Drop the break points whose line is
    // too full and shape again until every remaining hyphen fits.
    for _ in 0..8 {
        if points.is_empty() {
            break;
        }
        let mut too_full = Vec::new();
        for (line_i, line) in pass.lines.iter().enumerate() {
            let Some(shy) = line_end_hyphen(&pass, line_i) else {
                continue;
            };
            let g = &pass.glyphs[shy].glyph;
            let Some((_, adv)) = hyphen_glyph(fonts, g.font_id, g.font_weight, g.font_size) else {
                continue;
            };
            let limit = spec.width.unwrap_or(f32::INFINITY);
            if spec.align != TextAlign::Justified && line.width + adv > limit + 0.01 {
                // Which break point this soft hyphen is: its plain offset.
                let at = pass.glyphs[shy].start as usize;
                if let Ok(k) = points.binary_search(&at) {
                    too_full.push(k);
                }
            }
        }
        if too_full.is_empty() {
            break;
        }
        for k in too_full.into_iter().rev() {
            points.remove(k);
        }
        pass = run_pass(fonts, spec, &chain, &pieces, &points);
    }
    finish(fonts, spec, pass)
}

/// The glyph index of the soft hyphen that ends `line_i`, if a line break
/// fell on one.
fn line_end_hyphen(pass: &Pass, line_i: usize) -> Option<usize> {
    let line = &pass.lines[line_i];
    let last = line
        .glyphs
        .clone()
        .max_by_key(|&i| pass.glyphs[i].glyph.start)?;
    pass.shy[last].then_some(last)
}

/// Replaces line-ending soft hyphens by hyphen glyphs, drops the other
/// soft hyphens and assembles the layout.
fn finish(fonts: &mut FontSystem, spec: &ShapeSpec, mut pass: Pass) -> LayoutData {
    for line_i in 0..pass.lines.len() {
        let Some(i) = line_end_hyphen(&pass, line_i) else {
            continue;
        };
        let (font_id, weight, size) = {
            let g = &pass.glyphs[i].glyph;
            (g.font_id, g.font_weight, g.font_size)
        };
        let Some((gid, adv)) = hyphen_glyph(fonts, font_id, weight, size) else {
            continue;
        };
        let rtl = pass.lines[line_i].rtl;
        let rec = &mut pass.glyphs[i];
        rec.glyph.glyph_id = gid;
        rec.glyph.w = adv;
        rec.hyphen = true;
        if rtl {
            rec.glyph.x -= adv;
        }
        // Keep centred and right-aligned lines aligned with the hyphen.
        let shift = match spec.align {
            TextAlign::Center => -adv * 0.5,
            TextAlign::Right if !rtl => -adv,
            _ => 0.0,
        };
        if shift != 0.0 {
            for k in pass.lines[line_i].glyphs.clone() {
                pass.glyphs[k].dx += shift;
            }
        }
        pass.shy[i] = false;
    }
    let mut glyphs = Vec::with_capacity(pass.glyphs.len());
    let mut lines = pass.lines;
    for line in &mut lines {
        let from = glyphs.len();
        for k in line.glyphs.clone() {
            if !pass.shy[k] {
                glyphs.push(pass.glyphs[k].clone());
            }
        }
        line.glyphs = from..glyphs.len();
    }
    let mut size = pass.size;
    if let Some(w) = spec.width
        && spec.align != TextAlign::Left
    {
        size.x = size.x.max(w);
    }
    let spans = spec
        .text
        .spans()
        .iter()
        .map(|(_, s)| SpanInfo {
            link: s.link.as_deref().map(Arc::from),
        })
        .collect();
    LayoutData {
        glyphs,
        lines,
        decos: pass.decos,
        spans,
        plain: spec.text.plain().to_owned(),
        size,
        sideways: false,
    }
}
