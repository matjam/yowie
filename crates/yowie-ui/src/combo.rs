//! Combo boxes and tab bars.

use yowie_core::{Rect, vec2};
use yowie_input::Key;

use crate::access::{AccessState, Role};
use crate::draw::{Arrow, BState, BoxKind};
use crate::frame::{Frame, Sense, depth};
use crate::id::{WidgetId, visible_label};
use crate::layout::Align;
use crate::response::Response;

const MAX_POPUP_ROWS: usize = 8;

impl Frame<'_> {
    /// Adds a drop-down choosing one of `items`. Clicking, Enter or Space
    /// opens the list; Up and Down on the closed box change the choice.
    pub fn combo_box<S: AsRef<str>>(
        &mut self,
        label: &str,
        selected: &mut usize,
        items: &[S],
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
            self.text_in(Rect::new(row.x, row.y, cap, h), shown, color, Align::Start);
        }
        let field = Rect::new(row.x + cap, row.y, row.w - cap, h);
        let n = items.len();
        if n > 0 && *selected >= n {
            *selected = n - 1;
        }
        let old = *selected;
        let mut r = self.interact(id, field, Sense::CLICK);
        self.set_wants(&r, false, true, false);
        let open = self.ui.popup == id;
        if r.clicked {
            if open {
                self.ui.popup = WidgetId::NONE;
            } else {
                self.ui.popup = id;
                let frame = self.ui.frame_no;
                self.ui.lists.get(id, frame).cursor = *selected;
            }
        } else if r.focused && !open && n > 0 {
            *selected = self.list_keys(*selected, n, 1);
        }
        let open = self.ui.popup == id;
        self.draw_box(field, BoxKind::Button, BState::of(&r));
        let pad = self.ui.theme.padding;
        let name = items.get(*selected).map_or("", |s| s.as_ref());
        self.text_in(
            Rect::new(field.x + pad, field.y, field.w - pad * 3.0, h),
            name,
            color,
            Align::Start,
        );
        self.arrow(
            vec2(field.x + field.w - pad - 4.0, field.y + h * 0.5),
            9.0,
            if open { Arrow::Up } else { Arrow::Down },
            color,
        );
        self.focus_ring(&r);
        if open {
            self.combo_popup(id, field, selected, items);
        }
        r.changed = *selected != old;
        let st = AccessState {
            expanded: Some(open),
            ..AccessState::default()
        };
        self.node(id, Role::ComboBox, shown, Some(name.to_owned()), st, field);
        r
    }

    fn combo_popup<S: AsRef<str>>(
        &mut self,
        id: WidgetId,
        field: Rect,
        selected: &mut usize,
        items: &[S],
    ) {
        let n = items.len();
        let rh = self.ui.theme.row_height;
        let ph = n.min(MAX_POPUP_ROWS) as f32 * rh + 2.0;
        let below = field.y + field.h;
        let y = if below + ph > self.view.h && field.y - ph >= 0.0 {
            field.y - ph
        } else {
            below
        };
        let prect = Rect::new(field.x, y, field.w, ph);
        let frame = self.ui.frame_no;
        let mut cursor = self.ui.lists.get(id, frame).cursor.min(n.saturating_sub(1));
        let scroll_id = id.child("popup");
        if n > 0 {
            let before = cursor;
            cursor = self.list_keys(cursor, n, MAX_POPUP_ROWS - 1);
            if cursor != before {
                self.scroll_to_row(scroll_id, cursor, ph - 2.0);
            }
        }
        let mut close = self.nav.cancel || self.input.key_pressed(Key::Tab);
        let mut choose = None;
        if (self.input.key_pressed(Key::Enter) || self.input.key_pressed(Key::Space))
            && n > 0
            && self.popup_was_open(id)
        {
            choose = Some(cursor);
        }
        if self.pressed && !prect.contains(self.mouse) && !field.contains(self.mouse) {
            close = true;
        }
        self.unclipped(|f| {
            f.push_layer(id, depth::POPUP);
            f.draw_box(prect, BoxKind::Field, BState::Normal);
            let ac = f.ui.theme.accent;
            f.stroke_rect(prect, 1.0, ac);
            let node = f.node(
                id.child("list"),
                Role::ListBox,
                "options",
                None,
                AccessState::default(),
                prect,
            );
            f.push_node(node);
            f.scroll_region(scroll_id, prect.inflate(-1.0), false, |f| {
                let full = f.alloc(0.0, n as f32 * rh, true);
                f.visible_rows(full, n, |f, i, row| {
                    let rid = id.child(("item", i));
                    let rr = f.interact(
                        rid,
                        row,
                        Sense {
                            click: true,
                            focus: false,
                        },
                    );
                    if rr.hovered && f.mouse != f.ui.mouse_prev {
                        cursor = i;
                    }
                    if rr.clicked {
                        choose = Some(i);
                    }
                    let name = items[i].as_ref();
                    f.draw_row(row, name, i == cursor, false);
                    let st = AccessState {
                        selected: i == *selected,
                        ..AccessState::default()
                    };
                    f.node(rid, Role::ListItem, name, None, st, row);
                });
            });
            f.pop_node();
            f.pop_layer(prect);
        });
        let st = self.ui.lists.get(id, frame);
        st.cursor = cursor;
        if let Some(i) = choose {
            *selected = i;
            close = true;
        }
        if close {
            self.ui.popup = WidgetId::NONE;
        }
    }

    /// Whether the popup was already open before this frame's key presses,
    /// so the key that opened it is not also taken as a choice.
    fn popup_was_open(&self, id: WidgetId) -> bool {
        self.ui.layers_prev.iter().any(|l| l.id == id)
    }

    /// Adds a row of tabs. Click a tab, or focus the row and use Left and
    /// Right, to change `selected`; the caller draws the matching content.
    pub fn tabs<S: AsRef<str>>(
        &mut self,
        label: &str,
        tabs: &[S],
        selected: &mut usize,
    ) -> Response {
        let id = self.id(label);
        let (pad, h) = (self.ui.theme.padding, self.ui.theme.row_height + 2.0);
        let bar = self.alloc(0.0, h, true);
        let n = tabs.len();
        if n > 0 && *selected >= n {
            *selected = n - 1;
        }
        let old = *selected;
        let mut r = self.interact(
            id,
            bar,
            Sense {
                click: false,
                focus: true,
            },
        );
        self.set_wants(&r, true, false, false);
        if r.focused && n > 0 {
            if self.step_key(Key::Left) {
                *selected = selected.saturating_sub(1);
            }
            if self.step_key(Key::Right) {
                *selected = (*selected + 1).min(n - 1);
            }
        }
        let (border, panel, button, hover, accent, text, dim) = {
            let t = &self.ui.theme;
            (
                t.border,
                t.panel,
                t.button,
                t.button_hover,
                t.accent,
                t.text,
                t.text_dim,
            )
        };
        self.fill_rect(Rect::new(bar.x, bar.y + h - 1.0, bar.w, 1.0), border);
        let node = self.node(id, Role::TabList, label, None, AccessState::default(), bar);
        self.push_node(node);
        let mut x = bar.x;
        for (i, tab) in tabs.iter().enumerate() {
            let name = visible_label(tab.as_ref());
            let w = self.measure(name).x + pad * 2.0;
            let tr = Rect::new(x, bar.y, w, h - 1.0);
            let rr = self.interact(
                id.child(i),
                tr,
                Sense {
                    click: true,
                    focus: false,
                },
            );
            if rr.clicked {
                *selected = i;
                self.ui.focus = id;
            }
            let on = i == *selected;
            self.fill_rect(
                tr,
                if on {
                    panel.with_alpha(1.0)
                } else if rr.hovered {
                    hover
                } else {
                    button
                },
            );
            self.stroke_rect(tr, 1.0, border);
            if on {
                self.over_fill(Rect::new(tr.x, tr.y, tr.w, 2.0), accent);
                self.over_fill(
                    Rect::new(tr.x + 1.0, tr.y + tr.h - 1.0, tr.w - 2.0, 2.0),
                    panel.with_alpha(1.0),
                );
            }
            self.text_in(tr, name, if on { text } else { dim }, Align::Center);
            let st = AccessState {
                selected: on,
                ..AccessState::default()
            };
            self.node(id.child(i), Role::Tab, name, None, st, tr);
            x += w - 1.0;
        }
        self.pop_node();
        self.focus_ring(&r);
        r.changed = *selected != old;
        r
    }
}
