//! Styled text: spans of one text with their own colour, weight, size,
//! font, decoration and link, and the bracket markup that writes them.

use std::ops::Range;

use yowie_core::Color;

/// How one stretch of a [`RichText`] differs from the text style it is set
/// in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpanStyle {
    /// Replaces the text colour (sRGB, straight alpha).
    pub color: Option<Color>,
    /// Uses the bold face of the font's family.
    pub bold: bool,
    /// Uses the italic face of the font's family.
    pub italic: bool,
    /// Draws a line under the span.
    pub underline: bool,
    /// Draws a line through the span.
    pub strikethrough: bool,
    /// Replaces the font size, in view units.
    pub size: Option<f32>,
    /// Sets the span in a loaded font family by name instead of the
    /// layout's font.
    pub font: Option<String>,
    /// A name reported back by hit testing
    /// ([`TextLayout::link_at`](crate::TextLayout::link_at)).
    pub link: Option<String>,
}

/// Text made of styled spans.
///
/// [`RichText::parse`] reads a bracket markup:
///
/// | Markup | Effect |
/// |---|---|
/// | `[b]..[/b]`, `[i]..[/i]` | bold, italic |
/// | `[u]..[/u]`, `[s]..[/s]` | underline, strikethrough |
/// | `[#ff8800]..[/#]`, `[color=#ff8800]..[/color]` | colour |
/// | `[size=24]..[/size]` | font size in view units |
/// | `[font=Family]..[/font]` | a loaded font family |
/// | `[link=name]..[/link]` | a link, drawn underlined |
/// | `[[` | a literal `[` |
///
/// Tags nest. Any closing tag ends the innermost open one. An unknown or
/// malformed tag stays in the text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    plain: String,
    spans: Vec<(Range<usize>, SpanStyle)>,
}

impl RichText {
    /// An empty text.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `text` in `style`.
    pub fn push(&mut self, text: &str, style: SpanStyle) -> &mut Self {
        if text.is_empty() {
            return self;
        }
        let start = self.plain.len();
        self.plain.push_str(text);
        let range = start..self.plain.len();
        match self.spans.last_mut() {
            Some((r, s)) if *s == style => r.end = range.end,
            _ => self.spans.push((range, style)),
        }
        self
    }

    /// The text without styling.
    pub fn plain(&self) -> &str {
        &self.plain
    }

    /// The spans: byte ranges of [`plain`](Self::plain) and their styles,
    /// in order and covering all of it.
    pub fn spans(&self) -> &[(Range<usize>, SpanStyle)] {
        &self.spans
    }

    /// Reads the bracket markup described on [`RichText`].
    pub fn parse(markup: &str) -> RichText {
        let mut out = RichText::new();
        let mut stack: Vec<SpanStyle> = Vec::new();
        let mut cur = SpanStyle::default();
        let mut buf = String::new();
        let mut rest = markup;
        while let Some(at) = rest.find('[') {
            buf.push_str(&rest[..at]);
            rest = &rest[at..];
            if let Some(after) = rest.strip_prefix("[[") {
                buf.push('[');
                rest = after;
                continue;
            }
            let Some(end) = rest.find(']') else {
                buf.push('[');
                rest = &rest[1..];
                continue;
            };
            let tag = &rest[1..end];
            let mut next = cur.clone();
            let opened = if tag.starts_with('/') {
                if let Some(prev) = stack.pop() {
                    out.push(&std::mem::take(&mut buf), cur.clone());
                    cur = prev;
                    rest = &rest[end + 1..];
                    continue;
                }
                false
            } else {
                apply_tag(tag, &mut next)
            };
            if opened {
                out.push(&std::mem::take(&mut buf), cur.clone());
                stack.push(std::mem::replace(&mut cur, next));
            } else {
                buf.push_str(&rest[..=end]);
            }
            rest = &rest[end + 1..];
        }
        buf.push_str(rest);
        out.push(&buf, cur);
        out
    }
}

/// Applies an opening tag to `style`; false when `tag` is not one.
fn apply_tag(tag: &str, style: &mut SpanStyle) -> bool {
    match tag {
        "b" => style.bold = true,
        "i" => style.italic = true,
        "u" => style.underline = true,
        "s" => style.strikethrough = true,
        _ => {
            if let Some(v) = tag.strip_prefix("color=") {
                let Some(c) = Color::parse_hex(v) else {
                    return false;
                };
                style.color = Some(c);
            } else if tag.starts_with('#') {
                let Some(c) = Color::parse_hex(tag) else {
                    return false;
                };
                style.color = Some(c);
            } else if let Some(v) = tag.strip_prefix("size=") {
                match v.parse::<f32>() {
                    Ok(s) if s.is_finite() && s > 0.0 => style.size = Some(s),
                    _ => return false,
                }
            } else if let Some(v) = tag.strip_prefix("font=") {
                if v.is_empty() {
                    return false;
                }
                style.font = Some(v.to_owned());
            } else if let Some(v) = tag.strip_prefix("link=") {
                style.link = Some(v.to_owned());
                style.underline = true;
            } else {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_cases() {
        let cases: &[(&str, &str, usize)] = &[
            ("plain", "plain", 1),
            ("a [b]bold[/b] c", "a bold c", 3),
            ("[[tag] [x] [#zzz]", "[tag] [x] [#zzz]", 1),
            (
                "[#ff0000]red [i]and slanted[/i][/#]!",
                "red and slanted!",
                3,
            ),
            ("[size=30]big[/size]small", "bigsmall", 2),
            ("open [b]never closed", "open never closed", 2),
        ];
        for (markup, plain, spans) in cases {
            let rt = RichText::parse(markup);
            assert_eq!(rt.plain(), *plain, "{markup}");
            assert_eq!(rt.spans().len(), *spans, "{markup}");
            let covered: usize = rt.spans().iter().map(|(r, _)| r.len()).sum();
            assert_eq!(covered, plain.len());
        }
    }

    #[test]
    fn styles_nest_and_restore() {
        let rt = RichText::parse("[b]a[#00ff00]b[/#]c[/b]d");
        let flags: Vec<(bool, bool)> = rt
            .spans()
            .iter()
            .map(|(_, s)| (s.bold, s.color.is_some()))
            .collect();
        assert_eq!(
            flags,
            [(true, false), (true, true), (true, false), (false, false)]
        );
        let link = RichText::parse("[link=home]go[/link]");
        assert_eq!(link.spans()[0].1.link.as_deref(), Some("home"));
        assert!(link.spans()[0].1.underline);
    }
}
