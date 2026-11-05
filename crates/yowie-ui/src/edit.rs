//! Text editing operations: caret and selection movement, insertion and
//! deletion, undo, and line wrapping. Positions are byte offsets on
//! character boundaries. Nothing here draws or reads input, so the
//! operations are testable on their own.

use std::ops::Range;

const UNDO_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Other,
    Typing,
    Deleting,
}

/// One displayed line: a byte range of the text without its newline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSpan {
    /// First byte of the line.
    pub start: usize,
    /// One past the last byte of the line's text.
    pub end: usize,
}

impl LineSpan {
    /// The line's bytes as a range.
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// Splits `text` into lines at newlines and, when `max_width` is given, at
/// word boundaries (or between characters in a word wider than the line).
/// `measure` returns the width of a string. A text always has at least one
/// line.
pub fn wrap_lines(
    text: &str,
    max_width: Option<f32>,
    measure: &mut dyn FnMut(&str) -> f32,
) -> Vec<LineSpan> {
    let mut lines = Vec::new();
    let mut start = 0;
    for para in text.split('\n') {
        let end = start + para.len();
        match max_width {
            Some(w) if w > 0.0 => wrap_paragraph(text, start, end, w, measure, &mut lines),
            _ => lines.push(LineSpan { start, end }),
        }
        start = end + 1;
    }
    lines
}

fn wrap_paragraph(
    text: &str,
    start: usize,
    end: usize,
    width: f32,
    measure: &mut dyn FnMut(&str) -> f32,
    out: &mut Vec<LineSpan>,
) {
    let mut line_start = start;
    let mut last_break: Option<usize> = None;
    for (off, c) in text[start..end].char_indices() {
        let at = start + off;
        let next = at + c.len_utf8();
        if c == ' ' {
            last_break = Some(next);
        }
        if c != ' ' && measure(&text[line_start..next]) > width && at > line_start {
            let cut = match last_break {
                Some(b) if b > line_start && b <= at => b,
                _ => at,
            };
            out.push(LineSpan {
                start: line_start,
                end: cut,
            });
            line_start = cut;
            last_break = None;
        }
    }
    out.push(LineSpan {
        start: line_start,
        end,
    });
}

/// The index of the line holding byte `pos`; a position between lines
/// belongs to the earlier line.
pub fn line_of(lines: &[LineSpan], pos: usize) -> usize {
    let mut found = 0;
    for (i, l) in lines.iter().enumerate() {
        if pos >= l.start {
            found = i;
        }
        if pos <= l.end {
            return i;
        }
    }
    found
}

/// The byte offset in `line` whose left edge is nearest to `x`.
pub fn index_at_x(
    text: &str,
    line: LineSpan,
    x: f32,
    measure: &mut dyn FnMut(&str) -> f32,
) -> usize {
    let s = &text[line.range()];
    let mut best = line.start;
    let mut best_d = x.abs();
    for (off, c) in s.char_indices() {
        let next = off + c.len_utf8();
        let d = (measure(&s[..next]) - x).abs();
        if d < best_d {
            best_d = d;
            best = line.start + next;
        }
    }
    best
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The caret, selection and undo history of one text editor.
#[derive(Clone, Debug)]
pub struct TextEdit {
    cursor: usize,
    anchor: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last: Kind,
    /// The column a run of vertical moves keeps, in view units.
    pub(crate) goal_x: Option<f32>,
}

impl Default for TextEdit {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEdit {
    /// An editor with the caret at the start.
    pub fn new() -> Self {
        Self {
            cursor: 0,
            anchor: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last: Kind::Other,
            goal_x: None,
        }
    }

    /// The caret's byte offset.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The selected byte range, if any.
    pub fn selection(&self) -> Option<Range<usize>> {
        (self.cursor != self.anchor)
            .then(|| self.cursor.min(self.anchor)..self.cursor.max(self.anchor))
    }

    /// The selected text, if any.
    pub fn selected<'a>(&self, text: &'a str) -> Option<&'a str> {
        self.selection().and_then(|r| text.get(r))
    }

    /// Puts the caret at `pos` (clamped into the text), extending the
    /// selection when `select` is set.
    pub fn set_cursor(&mut self, text: &str, pos: usize, select: bool) {
        self.cursor = clamp_boundary(text, pos);
        if !select {
            self.anchor = self.cursor;
        }
        self.goal_x = None;
        self.last = Kind::Other;
    }

    /// Selects `range`, with the caret at its end.
    pub fn select(&mut self, text: &str, range: Range<usize>) {
        self.anchor = clamp_boundary(text, range.start);
        self.cursor = clamp_boundary(text, range.end);
        self.goal_x = None;
    }

    /// Selects all of `text`.
    pub fn select_all(&mut self, text: &str) {
        self.select(text, 0..text.len());
    }

    /// Selects the word under the caret.
    pub fn select_word(&mut self, text: &str) {
        let (s, e) = word_bounds(text, self.cursor);
        self.select(text, s..e);
    }

    /// Keeps the editor valid after the text changed behind its back.
    pub fn clamp(&mut self, text: &str) {
        self.cursor = clamp_boundary(text, self.cursor);
        self.anchor = clamp_boundary(text, self.anchor);
    }

    fn snapshot(&self, text: &str) -> Snapshot {
        Snapshot {
            text: text.to_owned(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    fn checkpoint(&mut self, text: &str, kind: Kind) {
        if kind == Kind::Other || kind != self.last || self.selection().is_some() {
            self.undo.push(self.snapshot(text));
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last = kind;
    }

    fn delete_selection(&mut self, text: &mut String) -> bool {
        match self.selection() {
            Some(r) => {
                text.replace_range(r.clone(), "");
                self.cursor = r.start;
                self.anchor = r.start;
                true
            }
            None => false,
        }
    }

    /// Replaces the selection with `s`. With `single_line`, newlines in `s`
    /// are dropped. Returns whether the text changed.
    pub fn insert(&mut self, text: &mut String, s: &str, single_line: bool) -> bool {
        let clean: String = if single_line {
            s.chars().filter(|c| *c != '\n' && *c != '\r').collect()
        } else {
            s.replace('\r', "")
        };
        if clean.is_empty() {
            return false;
        }
        self.checkpoint(text, Kind::Typing);
        self.delete_selection(text);
        self.clamp(text);
        text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        self.anchor = self.cursor;
        self.goal_x = None;
        true
    }

    /// Deletes the selection, or the character (or word) before the caret.
    pub fn backspace(&mut self, text: &mut String, word: bool) -> bool {
        if self.selection().is_none() && self.cursor == 0 {
            return false;
        }
        self.checkpoint(text, Kind::Deleting);
        if !self.delete_selection(text) {
            let to = if word {
                prev_word(text, self.cursor)
            } else {
                prev_char(text, self.cursor)
            };
            text.replace_range(to..self.cursor, "");
            self.cursor = to;
            self.anchor = to;
        }
        self.goal_x = None;
        true
    }

    /// Deletes the selection, or the character (or word) after the caret.
    pub fn delete(&mut self, text: &mut String, word: bool) -> bool {
        if self.selection().is_none() && self.cursor >= text.len() {
            return false;
        }
        self.checkpoint(text, Kind::Deleting);
        if !self.delete_selection(text) {
            let to = if word {
                next_word(text, self.cursor)
            } else {
                next_char(text, self.cursor)
            };
            text.replace_range(self.cursor..to, "");
        }
        self.goal_x = None;
        true
    }

    /// Removes the selection and returns it, for cut.
    pub fn cut(&mut self, text: &mut String) -> Option<String> {
        let sel = self.selected(text)?.to_owned();
        self.checkpoint(text, Kind::Other);
        self.delete_selection(text);
        Some(sel)
    }

    /// Moves the caret one character left (or to the start of the previous
    /// word). Without `select`, a selection collapses to its start.
    pub fn left(&mut self, text: &str, select: bool, word: bool) {
        if !select && let Some(r) = self.selection() {
            self.set_cursor(text, r.start, false);
            return;
        }
        let to = if word {
            prev_word(text, self.cursor)
        } else {
            prev_char(text, self.cursor)
        };
        self.set_cursor(text, to, select);
    }

    /// Moves the caret one character right (or to the end of the word).
    /// Without `select`, a selection collapses to its end.
    pub fn right(&mut self, text: &str, select: bool, word: bool) {
        if !select && let Some(r) = self.selection() {
            self.set_cursor(text, r.end, false);
            return;
        }
        let to = if word {
            next_word(text, self.cursor)
        } else {
            next_char(text, self.cursor)
        };
        self.set_cursor(text, to, select);
    }

    /// Moves the caret to the start of its line.
    pub fn home(&mut self, text: &str, lines: &[LineSpan], select: bool) {
        let l = lines[line_of(lines, self.cursor).min(lines.len().saturating_sub(1))];
        self.set_cursor(text, l.start, select);
    }

    /// Moves the caret to the end of its line.
    pub fn end(&mut self, text: &str, lines: &[LineSpan], select: bool) {
        let l = lines[line_of(lines, self.cursor).min(lines.len().saturating_sub(1))];
        self.set_cursor(text, l.end, select);
    }

    /// Moves the caret `by` lines up (negative) or down, keeping its column.
    /// `measure` gives string widths.
    pub fn vertical(
        &mut self,
        text: &str,
        lines: &[LineSpan],
        by: i32,
        select: bool,
        measure: &mut dyn FnMut(&str) -> f32,
    ) {
        let cur = line_of(lines, self.cursor);
        let target = (cur as i64 + i64::from(by)).clamp(0, lines.len() as i64 - 1) as usize;
        let x = match self.goal_x {
            Some(x) => x,
            None => {
                let l = lines[cur];
                measure(&text[l.start..self.cursor.clamp(l.start, l.end)])
            }
        };
        let to = if target == cur && by < 0 {
            0
        } else if target == cur {
            text.len()
        } else {
            index_at_x(text, lines[target], x, measure)
        };
        self.cursor = clamp_boundary(text, to);
        if !select {
            self.anchor = self.cursor;
        }
        self.goal_x = Some(x);
        self.last = Kind::Other;
    }

    /// Undoes the last edit. Returns whether anything changed.
    pub fn undo(&mut self, text: &mut String) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot(text));
        self.restore(text, prev);
        true
    }

    /// Redoes the last undone edit.
    pub fn redo(&mut self, text: &mut String) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot(text));
        self.restore(text, next);
        true
    }

    fn restore(&mut self, text: &mut String, s: Snapshot) {
        *text = s.text;
        self.cursor = s.cursor;
        self.anchor = s.anchor;
        self.last = Kind::Other;
        self.goal_x = None;
    }
}

fn clamp_boundary(text: &str, pos: usize) -> usize {
    let mut p = pos.min(text.len());
    while !text.is_char_boundary(p) {
        p -= 1;
    }
    p
}

fn prev_char(text: &str, pos: usize) -> usize {
    text[..pos].char_indices().next_back().map_or(0, |(i, _)| i)
}

fn next_char(text: &str, pos: usize) -> usize {
    text[pos..]
        .chars()
        .next()
        .map_or(pos, |c| pos + c.len_utf8())
}

fn prev_word(text: &str, pos: usize) -> usize {
    let mut at = pos;
    let mut it = text[..pos].char_indices().rev().peekable();
    while let Some(&(i, c)) = it.peek() {
        if is_word(c) {
            break;
        }
        at = i;
        it.next();
    }
    while let Some(&(i, c)) = it.peek() {
        if !is_word(c) {
            break;
        }
        at = i;
        it.next();
    }
    at
}

fn next_word(text: &str, pos: usize) -> usize {
    let rest = &text[pos..];
    let mut end = 0;
    let mut chars = rest.char_indices().peekable();
    while let Some((i, c)) = chars.next_if(|(_, c)| !is_word(*c)) {
        end = i + c.len_utf8();
    }
    while let Some((i, c)) = chars.next_if(|(_, c)| is_word(*c)) {
        end = i + c.len_utf8();
    }
    pos + end
}

/// The byte range of the word (or run of non-word characters) around `pos`.
pub fn word_bounds(text: &str, pos: usize) -> (usize, usize) {
    let pos = clamp_boundary(text, pos);
    let class = |c: char| is_word(c);
    let before = text[..pos].chars().next_back();
    let after = text[pos..].chars().next();
    let Some(probe) = after.or(before) else {
        return (pos, pos);
    };
    let want = class(probe);
    let mut s = pos;
    for (i, c) in text[..pos].char_indices().rev() {
        if class(c) != want || c == '\n' {
            break;
        }
        s = i;
    }
    let mut e = pos;
    for (i, c) in text[pos..].char_indices() {
        if class(c) != want || c == '\n' {
            break;
        }
        e = pos + i + c.len_utf8();
    }
    (s, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn insert_replaces_selection_and_advances() {
        let mut t = String::from("hello world");
        let mut e = TextEdit::new();
        e.select(&t, 6..11);
        assert!(e.insert(&mut t, "rust", true));
        assert_eq!(t, "hello rust");
        assert_eq!(e.cursor(), 10);
        assert!(e.selection().is_none());
    }

    #[test]
    fn single_line_drops_newlines() {
        let mut t = String::new();
        let mut e = TextEdit::new();
        e.insert(&mut t, "a\nb\r\n", true);
        assert_eq!(t, "ab");
        let mut m = String::new();
        let mut e = TextEdit::new();
        e.insert(&mut m, "a\nb", false);
        assert_eq!(m, "a\nb");
    }

    #[test]
    fn backspace_and_delete_by_char_and_word() {
        let cases: [(&str, usize, bool, bool, &str); 6] = [
            ("abc def", 7, true, true, "abc "),
            ("abc def", 7, false, true, "abc de"),
            ("abc def", 4, false, false, "abc ef"),
            ("abc def", 0, true, false, " def"),
            ("abc def", 0, false, true, "abc def"),
            ("héllo", 3, false, true, "hllo"),
        ];
        for (src, at, word, back, want) in cases {
            let mut t = src.to_owned();
            let mut e = TextEdit::new();
            e.set_cursor(&t, at, false);
            if back {
                e.backspace(&mut t, word);
            } else {
                e.delete(&mut t, word);
            }
            assert_eq!(t, want, "{src:?} at {at} word={word} back={back}");
        }
    }

    #[test]
    fn movement_collapses_selection_and_respects_words() {
        let t = "one two  three".to_owned();
        let mut e = TextEdit::new();
        e.set_cursor(&t, 0, false);
        e.right(&t, false, true);
        assert_eq!(e.cursor(), 3);
        e.right(&t, false, true);
        assert_eq!(e.cursor(), 7);
        e.left(&t, false, true);
        assert_eq!(e.cursor(), 4);
        e.select(&t, 2..6);
        e.left(&t, false, false);
        assert_eq!(e.cursor(), 2, "left collapses to the selection start");
        e.select(&t, 2..6);
        e.right(&t, false, false);
        assert_eq!(e.cursor(), 6);
    }

    #[test]
    fn selection_extends_with_shift() {
        let t = "abcdef".to_owned();
        let mut e = TextEdit::new();
        e.set_cursor(&t, 2, false);
        e.right(&t, true, false);
        e.right(&t, true, false);
        assert_eq!(e.selection(), Some(2..4));
        assert_eq!(e.selected(&t), Some("cd"));
        e.left(&t, true, false);
        assert_eq!(e.selection(), Some(2..3));
    }

    #[test]
    fn undo_groups_typing_and_redo_restores() {
        let mut t = String::new();
        let mut e = TextEdit::new();
        for c in ["a", "b", "c"] {
            e.insert(&mut t, c, true);
        }
        e.backspace(&mut t, false);
        assert_eq!(t, "ab");
        assert!(e.undo(&mut t));
        assert_eq!(t, "abc", "a deletion is its own step");
        assert!(e.undo(&mut t));
        assert_eq!(t, "", "typing undoes as one run");
        assert!(!e.undo(&mut t));
        assert!(e.redo(&mut t));
        assert_eq!(t, "abc");
    }

    #[test]
    fn cut_returns_the_selection() {
        let mut t = "hello world".to_owned();
        let mut e = TextEdit::new();
        e.select(&t, 0..5);
        assert_eq!(e.cut(&mut t).as_deref(), Some("hello"));
        assert_eq!(t, " world");
        assert_eq!(e.cut(&mut t), None);
    }

    #[test]
    fn word_selection() {
        let t = "foo bar_baz  qux";
        assert_eq!(word_bounds(t, 5), (4, 11));
        assert_eq!(word_bounds(t, 11), (11, 13));
        assert_eq!(word_bounds(t, 0), (0, 3));
        assert_eq!(word_bounds("", 0), (0, 0));
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_newlines() {
        let t = "aaa bbb ccc\ndd";
        let lines = wrap_lines(t, Some(70.0), &mut mono);
        let got: Vec<&str> = lines.iter().map(|l| &t[l.range()]).collect();
        assert_eq!(got, ["aaa bbb ", "ccc", "dd"]);
        let none = wrap_lines(t, None, &mut mono);
        assert_eq!(none.len(), 2);
        let long = wrap_lines("abcdefghij", Some(40.0), &mut mono);
        assert_eq!(long.len(), 3);
        assert_eq!(wrap_lines("", Some(40.0), &mut mono).len(), 1);
    }

    #[test]
    fn vertical_moves_keep_the_column() {
        let t = "abcdef\nab\nabcdef".to_owned();
        let lines = wrap_lines(&t, None, &mut mono);
        let mut e = TextEdit::new();
        e.set_cursor(&t, 5, false);
        e.vertical(&t, &lines, 1, false, &mut mono);
        assert_eq!(e.cursor(), 9, "clamped to the short line's end");
        e.vertical(&t, &lines, 1, false, &mut mono);
        assert_eq!(e.cursor(), 10 + 5, "the goal column returns");
        e.vertical(&t, &lines, 1, false, &mut mono);
        assert_eq!(e.cursor(), t.len(), "past the last line goes to the end");
        e.vertical(&t, &lines, -2, true, &mut mono);
        assert_eq!(e.selection().map(|r| r.end), Some(t.len()));
    }

    #[test]
    fn home_and_end_use_the_current_line() {
        let t = "abc\ndef".to_owned();
        let lines = wrap_lines(&t, None, &mut mono);
        let mut e = TextEdit::new();
        e.set_cursor(&t, 5, false);
        e.home(&t, &lines, false);
        assert_eq!(e.cursor(), 4);
        e.end(&t, &lines, false);
        assert_eq!(e.cursor(), 7);
    }

    #[test]
    fn index_at_x_picks_the_nearest_edge() {
        let t = "abcd";
        let l = LineSpan { start: 0, end: 4 };
        assert_eq!(index_at_x(t, l, 14.0, &mut mono), 1);
        assert_eq!(index_at_x(t, l, 16.0, &mut mono), 2);
        assert_eq!(index_at_x(t, l, -5.0, &mut mono), 0);
        assert_eq!(index_at_x(t, l, 500.0, &mut mono), 4);
    }
}
