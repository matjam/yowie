//! Single-line and multi-line text editors with selection, clipboard,
//! undo and input-method composition.

use std::hash::{Hash, Hasher};

use yowie_core::{Rect, vec2};
use yowie_input::{Key, Modifiers};

use crate::access::{AccessState, Role};
use crate::draw::{BState, BoxKind};
use crate::edit::{LineSpan, TextEdit, index_at_x, line_of, wrap_lines};
use crate::frame::{Frame, Sense};
use crate::id::{WidgetId, visible_label};
use crate::painter::Face;
use crate::response::Response;

/// Options for [`Frame::text_field_with`].
#[derive(Clone, Copy, Debug, Default)]
pub struct TextOptions<'a> {
    /// Dim text shown while the field is empty.
    pub placeholder: &'a str,
    /// Set the text in the monospaced font.
    pub mono: bool,
    /// Draw no background or border, for fields placed on a surface of
    /// the caller's own.
    pub frameless: bool,
    /// Take keyboard focus now.
    pub request_focus: bool,
    /// The most bytes of text the field accepts.
    pub max_len: Option<usize>,
}

fn text_hash(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl Frame<'_> {
    /// Adds a single-line text field with `label` as its caption.
    pub fn text_field(&mut self, label: &str, text: &mut String) -> Response {
        self.text_field_with(label, text, &TextOptions::default())
    }

    /// Adds a single-line text field. `changed` on the response says the
    /// text changed; `submitted` says Enter was pressed.
    pub fn text_field_with(
        &mut self,
        label: &str,
        text: &mut String,
        opts: &TextOptions<'_>,
    ) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let h = self.ui.theme.row_height;
        let cap = if shown.is_empty() {
            0.0
        } else {
            self.measure(shown).x + 8.0
        };
        let row = self.alloc(120.0 + cap, h, true);
        let color = self.ui.theme.text;
        if cap > 0.0 {
            self.text_in(
                Rect::new(row.x, row.y, cap, h),
                shown,
                color,
                crate::layout::Align::Start,
            );
        }
        let field = Rect::new(row.x + cap, row.y, row.w - cap, h);
        self.edit_text(id, field, shown, text, false, opts)
    }

    /// Adds a multi-line text editor `height` tall, wrapping lines to its
    /// width. Enter inserts a newline.
    pub fn text_area(&mut self, label: &str, text: &mut String, height: f32) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let rect = self.alloc(160.0, height, true);
        self.edit_text(id, rect, shown, text, true, &TextOptions::default())
    }

    fn lines_for(
        &mut self,
        id: WidgetId,
        text: &str,
        wrap: Option<f32>,
        face: Face,
    ) -> Vec<LineSpan> {
        let frame = self.ui.frame_no;
        let key = text_hash(text);
        let width = wrap.unwrap_or(0.0);
        {
            let c = self.ui.text_cache.get(id, frame);
            if c.key == key && c.width == width && !c.lines.is_empty() {
                return c.lines.clone();
            }
        }
        let size = if face == Face::Mono {
            self.ui.theme.mono_size
        } else {
            self.ui.theme.text_size
        };
        let p = &mut *self.p;
        let lines = wrap_lines(text, wrap, &mut |s| p.measure(s, face, size, None).x);
        let c = self.ui.text_cache.get(id, frame);
        c.key = key;
        c.width = width;
        c.lines = lines.clone();
        lines
    }

    #[allow(clippy::too_many_lines)]
    fn edit_text(
        &mut self,
        id: WidgetId,
        field: Rect,
        name: &str,
        text: &mut String,
        multiline: bool,
        opts: &TextOptions<'_>,
    ) -> Response {
        let frame = self.ui.frame_no;
        let face = if opts.mono { Face::Mono } else { Face::Regular };
        let mut r = self.interact(id, field, Sense::CLICK);
        if opts.request_focus && !self.is_disabled() {
            self.ui.focus = id;
            r.focused = true;
        }
        self.set_wants(&r, true, multiline, true);
        let mut ed = std::mem::take(self.ui.edits.get(id, frame));
        ed.clamp(text);
        let (padx, accent) = {
            let t = &self.ui.theme;
            (t.padding * 0.75, t.accent)
        };
        let line_h = self.measure_face("Ag", face).y;
        let inner = Rect::new(
            field.x + padx,
            field.y
                + if multiline {
                    4.0
                } else {
                    (field.h - line_h) * 0.5
                },
            (field.w - padx * 2.0).max(1.0),
            field.h - 8.0,
        );
        let wrap = multiline.then_some(inner.w);
        let mut changed = false;
        let mut submitted = false;

        // Pointer: place the caret, extend the selection by dragging.
        let scroll_id = id.child("scroll");
        let mut scroll = self.ui.scrolls.get(scroll_id, frame).offset;
        let lines = self.lines_for(id, text, wrap, face);
        if r.held || (r.clicked && self.pressed) {
            let ly = ((self.mouse.y - inner.y + scroll.y) / line_h)
                .floor()
                .max(0.0) as usize;
            let line = lines[ly.min(lines.len() - 1)];
            let size = if face == Face::Mono {
                self.ui.theme.mono_size
            } else {
                self.ui.theme.text_size
            };
            let p = &mut *self.p;
            let idx = index_at_x(text, line, self.mouse.x - inner.x + scroll.x, &mut |s| {
                p.measure(s, face, size, None).x
            });
            let shift = self.mods.contains(Modifiers::SHIFT);
            if self.pressed {
                ed.set_cursor(text, idx, shift);
                if self.double {
                    ed.select_word(text);
                }
            } else {
                ed.set_cursor(text, idx, true);
            }
        }

        if r.focused && !self.is_disabled() {
            let before = text.clone();
            self.edit_keys(&mut ed, text, &lines, multiline, face, &mut submitted, opts);
            if let Some(max) = opts.max_len
                && text.len() > max
            {
                let mut cut = max;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
                ed.clamp(text);
            }
            changed = *text != before;
            if self.input.key_pressed(Key::Escape) {
                self.ui.focus = WidgetId::NONE;
            }
        }
        if multiline && self.pointer_over(field) && self.wheel.y != 0.0 {
            scroll.y -= self.wheel.y * line_h * 3.0;
            self.wheel.y = 0.0;
        }

        // Layout after edits.
        let lines = if changed {
            self.lines_for(id, text, wrap, face)
        } else {
            lines
        };
        let li = line_of(&lines, ed.cursor()).min(lines.len() - 1);
        let line = lines[li];
        let cur = ed.cursor().clamp(line.start, line.end);
        let comp = if r.focused {
            self.input.composition().to_owned()
        } else {
            String::new()
        };
        let caret_x = self.measure_face(&text[line.start..cur], face).x;
        let comp_w = if comp.is_empty() {
            0.0
        } else {
            self.measure_face(&comp, face).x
        };
        let caret_y = li as f32 * line_h;

        // Keep the caret in view.
        if r.focused {
            let cx = caret_x + comp_w;
            if cx - scroll.x > inner.w - 2.0 {
                scroll.x = cx - inner.w + 2.0;
            }
            if cx - scroll.x < 0.0 {
                scroll.x = cx;
            }
            if caret_y - scroll.y + line_h > inner.h {
                scroll.y = caret_y + line_h - inner.h;
            }
            if caret_y - scroll.y < 0.0 {
                scroll.y = caret_y;
            }
        }
        if multiline {
            scroll.x = 0.0;
            let max = (lines.len() as f32 * line_h - inner.h).max(0.0);
            scroll.y = scroll.y.clamp(0.0, max);
        } else {
            scroll.y = 0.0;
            if !r.focused {
                scroll.x = 0.0;
            }
        }
        self.ui.scrolls.get(scroll_id, frame).offset = scroll;

        // Draw.
        if !opts.frameless {
            self.draw_box(field, BoxKind::Field, BState::Normal);
            if r.focused {
                self.stroke_rect(field, 1.0, accent);
            }
        }
        let (text_color, dim, sel_color) = {
            let t = &self.ui.theme;
            (t.text, t.text_dim, t.accent.with_alpha(0.45))
        };
        self.push_clip(inner.inflate(0.5));
        let origin = inner.min() - scroll;
        if let Some(sel) = ed.selection() {
            for (i, l) in lines.iter().enumerate() {
                let (s, e) = (sel.start.max(l.start), sel.end.min(l.end));
                let covers_break = sel.end > l.end && sel.start <= l.end;
                if s < e || covers_break {
                    let x0 = self.measure_face(&text[l.start..s.max(l.start)], face).x;
                    let mut x1 = self.measure_face(&text[l.start..e.max(s)], face).x;
                    if covers_break {
                        x1 += line_h * 0.3;
                    }
                    let rect =
                        Rect::new(origin.x + x0, origin.y + i as f32 * line_h, x1 - x0, line_h);
                    self.fill_rect(rect, sel_color);
                }
            }
        }
        if text.is_empty() && comp.is_empty() {
            if !opts.placeholder.is_empty() {
                self.text_face(origin, opts.placeholder, dim, face, None);
            }
        } else {
            for (i, l) in lines.iter().enumerate() {
                let y = origin.y + i as f32 * line_h;
                if y + line_h < inner.y || y > inner.y + inner.h {
                    continue;
                }
                if i == li && !comp.is_empty() {
                    let (a, b) = (&text[l.start..cur], &text[cur..l.end]);
                    self.text_face(vec2(origin.x, y), a, text_color, face, None);
                    self.text_face(vec2(origin.x + caret_x, y), &comp, text_color, face, None);
                    self.text_face(
                        vec2(origin.x + caret_x + comp_w, y),
                        b,
                        text_color,
                        face,
                        None,
                    );
                    let ul = Rect::new(origin.x + caret_x, y + line_h - 1.5, comp_w, 1.0);
                    self.over_fill(ul, text_color);
                } else {
                    self.text_face(vec2(origin.x, y), &text[l.range()], text_color, face, None);
                }
            }
        }
        if r.focused && !self.is_disabled() {
            let blink = (self.ui.time / 0.53) as i64 % 2 == 0 || self.down;
            let cx = origin.x + caret_x + comp_w;
            let cy = origin.y + caret_y;
            if blink && ed.selection().is_none() {
                self.over_fill(Rect::new(cx, cy, 1.5, line_h), text_color);
            }
            self.text_seen = Some(Rect::new(cx, cy, 1.5, line_h));
        }
        self.pop_clip();
        self.focus_ring_inside(&r);
        *self.ui.edits.get(id, frame) = ed;

        r.changed = changed;
        r.submitted = submitted;
        let state = AccessState::default();
        self.node(
            id,
            if multiline {
                Role::TextArea
            } else {
                Role::TextField
            },
            name,
            Some(text.clone()),
            state,
            field,
        );
        r
    }

    fn focus_ring_inside(&mut self, r: &Response) {
        if r.focused && self.ui.nav_visible {
            self.focus_ring(r);
        }
    }

    /// Applies this frame's editing keys and typed text to the editor.
    #[allow(clippy::too_many_arguments)]
    fn edit_keys(
        &mut self,
        ed: &mut TextEdit,
        text: &mut String,
        lines: &[LineSpan],
        multiline: bool,
        face: Face,
        submitted: &mut bool,
        opts: &TextOptions<'_>,
    ) -> bool {
        let shift = self.mods.contains(Modifiers::SHIFT);
        let word = self.mods.contains(Modifiers::CONTROL) || self.mods.contains(Modifiers::ALT);
        let short = self.mods.contains(Modifiers::shortcut());
        let step = |s: &Self, k: Key| s.input.key_pressed(k) || s.input.key_repeated(k);
        let mut any = false;
        if short {
            if self.input.key_pressed(Key::A) {
                ed.select_all(text);
            }
            if self.input.key_pressed(Key::C)
                && let Some(s) = ed.selected(text)
            {
                let s = s.to_owned();
                self.ui.clipboard.set(&s);
            }
            if self.input.key_pressed(Key::X)
                && let Some(s) = ed.cut(text)
            {
                self.ui.clipboard.set(&s);
                any = true;
            }
            if self.input.key_pressed(Key::V)
                && let Some(s) = self.ui.clipboard.get()
            {
                any |= ed.insert(text, &s, !multiline);
            }
            if self.input.key_pressed(Key::Z) {
                any |= if shift { ed.redo(text) } else { ed.undo(text) };
            }
            if self.input.key_pressed(Key::Y) {
                any |= ed.redo(text);
            }
        }
        if step(self, Key::Left) {
            ed.left(text, shift, word);
        }
        if step(self, Key::Right) {
            ed.right(text, shift, word);
        }
        if step(self, Key::Home) {
            if short || self.mods.contains(Modifiers::CONTROL) {
                ed.set_cursor(text, 0, shift);
            } else {
                ed.home(text, lines, shift);
            }
        }
        if step(self, Key::End) {
            if short || self.mods.contains(Modifiers::CONTROL) {
                ed.set_cursor(text, text.len(), shift);
            } else {
                ed.end(text, lines, shift);
            }
        }
        if multiline {
            let size = if face == Face::Mono {
                self.ui.theme.mono_size
            } else {
                self.ui.theme.text_size
            };
            let rows = ((self.ui.theme.row_height * 4.0) / size).max(1.0) as i32;
            for (key, by) in [
                (Key::Up, -1),
                (Key::Down, 1),
                (Key::PageUp, -rows),
                (Key::PageDown, rows),
            ] {
                if step(self, key) {
                    let p = &mut *self.p;
                    ed.vertical(text, lines, by, shift, &mut |s| {
                        p.measure(s, face, size, None).x
                    });
                }
            }
        }
        if step(self, Key::Backspace) {
            any |= ed.backspace(text, word);
        }
        if step(self, Key::Delete) {
            any |= ed.delete(text, word);
        }
        if self.input.key_pressed(Key::Enter) || self.input.key_pressed(Key::KeypadEnter) {
            if multiline {
                any |= ed.insert(text, "\n", false);
            } else {
                *submitted = true;
            }
        }
        let typed = self.input.text();
        if !typed.is_empty() && !short {
            let room = opts
                .max_len
                .map_or(usize::MAX, |m| m.saturating_sub(text.len()));
            let mut s = typed.to_owned();
            if s.len() > room {
                let mut cut = room;
                while !s.is_char_boundary(cut) {
                    cut -= 1;
                }
                s.truncate(cut);
            }
            any |= ed.insert(text, &s, !multiline);
        }
        any
    }
}
