//! Widgets that choose from a list: list boxes, combo boxes and tabs.

use yowie_core::Rect;
use yowie_input::Key;

use crate::access::{AccessState, Role};
use crate::draw::{BState, BoxKind};
use crate::frame::{Frame, Sense};
use crate::id::WidgetId;
use crate::layout::Align;
use crate::response::Response;

impl Frame<'_> {
    pub(crate) fn step_key(&self, key: Key) -> bool {
        self.input.key_pressed(key) || self.input.key_repeated(key)
    }

    /// Moves a list cursor with the navigation keys. Returns the new index.
    pub(crate) fn list_keys(&self, cur: usize, n: usize, page: usize) -> usize {
        if n == 0 {
            return 0;
        }
        let mut i = cur.min(n - 1);
        if self.step_key(Key::Up) {
            i = i.saturating_sub(1);
        }
        if self.step_key(Key::Down) {
            i = (i + 1).min(n - 1);
        }
        if self.step_key(Key::PageUp) {
            i = i.saturating_sub(page);
        }
        if self.step_key(Key::PageDown) {
            i = (i + page).min(n - 1);
        }
        if self.input.key_pressed(Key::Home) {
            i = 0;
        }
        if self.input.key_pressed(Key::End) {
            i = n - 1;
        }
        i
    }

    /// Draws the visible rows of a fixed-height list into `full`, calling
    /// `row` with the index and rectangle of each.
    pub(crate) fn visible_rows(
        &mut self,
        full: Rect,
        n: usize,
        mut row: impl FnMut(&mut Frame<'_>, usize, Rect),
    ) {
        let rh = self.ui.theme.row_height;
        let clip = self.clip_rect();
        let first = (((clip.y - full.y) / rh).floor().max(0.0)) as usize;
        let last = ((((clip.y + clip.h) - full.y) / rh).ceil().max(0.0) as usize).min(n);
        for i in first..last {
            let r = Rect::new(full.x, full.y + i as f32 * rh, full.w, rh);
            row(self, i, r);
        }
    }

    pub(crate) fn scroll_to_row(&mut self, scroll_id: WidgetId, row: usize, view_h: f32) {
        let rh = self.ui.theme.row_height;
        let frame = self.ui.frame_no;
        let s = self.ui.scrolls.get(scroll_id, frame);
        let top = row as f32 * rh;
        if top < s.offset.y {
            s.offset.y = top;
        } else if top + rh > s.offset.y + view_h {
            s.offset.y = top + rh - view_h;
        }
    }

    pub(crate) fn draw_row(&mut self, r: Rect, label: &str, selected: bool, hovered: bool) {
        let (accent, hover, text, on_accent) = {
            let t = &self.ui.theme;
            (t.accent, t.button_hover, t.text, t.on_accent)
        };
        if selected {
            self.fill_rect(r, accent);
        } else if hovered {
            self.fill_rect(r, hover);
        }
        let pad = self.ui.theme.padding;
        let tr = Rect::new(r.x + pad, r.y, r.w - pad * 2.0, r.h);
        self.text_in(
            tr,
            label,
            if selected { on_accent } else { text },
            Align::Start,
        );
    }

    /// Adds a list `height` tall showing `items`, one selectable at a time.
    /// The list is a single keyboard stop; Up, Down, Home, End, PageUp and
    /// PageDown move the selection.
    pub fn list_box<S: AsRef<str>>(
        &mut self,
        label: &str,
        height: f32,
        items: &[S],
        selected: &mut usize,
    ) -> Response {
        let id = self.id(label);
        let rect = self.alloc(160.0, height, true);
        let mut r = self.interact(
            id,
            rect,
            Sense {
                click: false,
                focus: true,
            },
        );
        self.set_wants(&r, false, true, false);
        let n = items.len();
        let old = *selected;
        if n > 0 && *selected >= n {
            *selected = n - 1;
        }
        let scroll_id = id.child("scroll");
        let rh = self.ui.theme.row_height;
        if r.focused {
            let page = ((rect.h / rh) as usize).saturating_sub(1).max(1);
            *selected = self.list_keys(*selected, n, page);
            if *selected != old {
                self.scroll_to_row(scroll_id, *selected, rect.h - 2.0);
            }
        }
        self.draw_box(rect, BoxKind::Field, BState::Normal);
        let node = self.node(id, Role::ListBox, label, None, AccessState::default(), rect);
        self.push_node(node);
        self.ids.push(id);
        let inner = rect.inflate(-1.0);
        let sel = &mut *selected;
        self.scroll_region(scroll_id, inner, false, |f| {
            let full = f.alloc(0.0, n as f32 * rh, true);
            f.visible_rows(full, n, |f, i, row| {
                let rid = id.child(i);
                let rr = f.interact(
                    rid,
                    row,
                    Sense {
                        click: true,
                        focus: false,
                    },
                );
                if rr.clicked {
                    *sel = i;
                    f.ui.focus = id;
                }
                let name = items[i].as_ref();
                f.draw_row(row, name, i == *sel, rr.hovered);
                let st = AccessState {
                    selected: i == *sel,
                    ..AccessState::default()
                };
                f.node(rid, Role::ListItem, name, None, st, row);
            });
        });
        self.ids.pop();
        self.pop_node();
        r.changed = *selected != old;
        self.focus_ring(&r);
        r
    }
}
