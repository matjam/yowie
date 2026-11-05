//! Tables with columns, trees and collapsing headers.

use yowie_core::{Rect, vec2};
use yowie_input::Key;

use crate::access::{AccessState, Role};
use crate::draw::{Arrow, BState, BoxKind};
use crate::frame::{Frame, Sense};
use crate::id::{WidgetId, visible_label};
use crate::layout::{Align, Size, solve_slots};
use crate::response::Response;

/// One column of a [`Frame::table`].
#[derive(Clone, Copy, Debug)]
pub struct Column<'a> {
    /// The header text.
    pub title: &'a str,
    /// The column's width.
    pub width: Size,
}

impl<'a> Column<'a> {
    /// A column that shares the leftover width equally with other fill
    /// columns.
    pub fn fill(title: &'a str) -> Self {
        Self {
            title,
            width: Size::Fill(1.0),
        }
    }

    /// A column of fixed width.
    pub fn fixed(title: &'a str, width: f32) -> Self {
        Self {
            title,
            width: Size::Fixed(width),
        }
    }
}

/// What happened in a table this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TableResponse {
    /// The table's own response (focus, hover).
    pub response: Response,
    /// The row that was clicked, or chosen with the keyboard.
    pub row_clicked: Option<usize>,
    /// The column whose header was clicked, for sorting.
    pub header_clicked: Option<usize>,
}

impl Frame<'_> {
    /// Adds a table `height` tall with a header row and `rows` rows, calling
    /// `cell` with the row and column of each visible cell to add its
    /// widgets. Only visible rows are built, so a table can have many
    /// thousands of rows. The table is one keyboard stop; Up, Down, Home,
    /// End, PageUp and PageDown move `selected`.
    pub fn table(
        &mut self,
        label: &str,
        columns: &[Column<'_>],
        rows: usize,
        height: f32,
        selected: &mut Option<usize>,
        mut cell: impl FnMut(&mut Frame<'_>, usize, usize),
    ) -> TableResponse {
        let id = self.id(label);
        let rh = self.ui.theme.row_height;
        let outer = self.alloc(200.0, height, true);
        let mut out = TableResponse::default();
        let mut r = self.interact(
            id,
            outer,
            Sense {
                click: false,
                focus: true,
            },
        );
        self.set_wants(&r, false, true, false);
        let body = Rect::new(
            outer.x + 1.0,
            outer.y + 1.0 + rh,
            outer.w - 2.0,
            (outer.h - 2.0 - rh).max(1.0),
        );
        let scroll_id = id.child("scroll");
        let old = *selected;
        if r.focused && rows > 0 {
            let page = ((body.h / rh) as usize).saturating_sub(1).max(1);
            let cur = selected.unwrap_or(0).min(rows - 1);
            let next = self.list_keys(cur, rows, page);
            let pressed = [
                Key::Up,
                Key::Down,
                Key::Home,
                Key::End,
                Key::PageUp,
                Key::PageDown,
            ]
            .iter()
            .any(|k| self.step_key(*k));
            if pressed {
                let first = selected.is_none() && next == cur;
                *selected = Some(next);
                if first || next != cur {
                    self.scroll_to_row(scroll_id, next, body.h);
                }
                out.row_clicked = Some(next);
            }
        }
        self.draw_box(outer, BoxKind::Field, BState::Normal);
        let node = self.node(id, Role::Table, label, None, AccessState::default(), outer);
        self.push_node(node);
        self.ids.push(id);
        let bar = if rows as f32 * rh > body.h { 11.0 } else { 0.0 };
        let sizes: Vec<Size> = columns.iter().map(|c| c.width).collect();
        let widths = solve_slots((body.w - bar).max(1.0), 0.0, &sizes);
        // Header.
        let mut x = body.x;
        let (text, dim) = (self.ui.theme.text, self.ui.theme.text_dim);
        for (c, w) in columns.iter().zip(&widths) {
            let hr = Rect::new(x, outer.y + 1.0, *w, rh);
            let hid = id.child(("h", c.title));
            let hv = self.interact(
                hid,
                hr,
                Sense {
                    click: true,
                    focus: false,
                },
            );
            self.draw_box(hr, BoxKind::Button, BState::of(&hv));
            self.text_in(
                Rect::new(hr.x + 6.0, hr.y, hr.w - 8.0, hr.h),
                visible_label(c.title),
                text,
                Align::Start,
            );
            if hv.clicked {
                out.header_clicked = columns.iter().position(|k| k.title == c.title);
            }
            self.node(hid, Role::Cell, c.title, None, AccessState::default(), hr);
            x += w;
        }
        let sel = &mut *selected;
        let clicked = &mut out.row_clicked;
        let (accent, hover, on_accent) = (
            self.ui.theme.accent,
            self.ui.theme.button_hover,
            self.ui.theme.on_accent,
        );
        let _ = (dim, on_accent);
        self.scroll_region(scroll_id, body, false, |f| {
            let full = f.alloc(0.0, rows as f32 * rh, true);
            f.visible_rows(full, rows, |f, i, row| {
                let hovered = f.pointer_over(row);
                let is_sel = *sel == Some(i);
                if is_sel {
                    f.fill_rect(row, accent.with_alpha(0.55));
                } else if hovered {
                    f.fill_rect(row, hover.with_alpha(0.6));
                } else if i % 2 == 1 {
                    f.fill_rect(row, text.with_alpha(0.04));
                }
                let mut cx = row.x;
                let rid = id.child(("row", i));
                let node = f.node(
                    rid,
                    Role::Row,
                    &format!("row {i}"),
                    None,
                    AccessState {
                        selected: is_sel,
                        ..AccessState::default()
                    },
                    row,
                );
                f.push_node(node);
                f.with_id(i, |f| {
                    for (c, w) in widths.iter().enumerate() {
                        let cr = Rect::new(cx, row.y, *w, row.h);
                        f.area(cr.inflate(-2.0), |f| cell(f, i, c));
                        cx += w;
                    }
                });
                f.pop_node();
                let rr = f.interact(
                    rid,
                    row,
                    Sense {
                        click: true,
                        focus: false,
                    },
                );
                if rr.clicked {
                    *sel = Some(i);
                    *clicked = Some(i);
                    f.ui.focus = id;
                }
            });
        });
        self.ids.pop();
        self.pop_node();
        self.focus_ring(&r);
        r.changed = *selected != old;
        out.response = r;
        out
    }

    /// Adds a branch of a tree. The row shows an arrow and `label`; while
    /// open, `body` adds its children, indented. Right and Left open and
    /// close the focused branch.
    pub fn tree_node(&mut self, label: &str, body: impl FnOnce(&mut Frame<'_>)) -> Response {
        self.disclose(
            label,
            Role::TreeItem,
            false,
            false,
            false,
            false,
            Some(body),
        )
    }

    /// Like [`tree_node`](Self::tree_node), choosing whether the branch
    /// starts open and whether it is drawn selected.
    pub fn tree_node_with(
        &mut self,
        label: &str,
        default_open: bool,
        selected: bool,
        body: impl FnOnce(&mut Frame<'_>),
    ) -> Response {
        self.disclose(
            label,
            Role::TreeItem,
            default_open,
            selected,
            false,
            false,
            Some(body),
        )
    }

    /// Adds a leaf of a tree: a row without an arrow.
    pub fn tree_leaf(&mut self, label: &str, selected: bool) -> Response {
        self.disclose::<fn(&mut Frame<'_>)>(
            label,
            Role::TreeItem,
            false,
            selected,
            true,
            false,
            None,
        )
    }

    /// Adds a section header that shows or hides `body` when clicked.
    pub fn collapsing_header(
        &mut self,
        label: &str,
        default_open: bool,
        body: impl FnOnce(&mut Frame<'_>),
    ) -> Response {
        self.disclose(
            label,
            Role::Heading,
            default_open,
            false,
            false,
            true,
            Some(body),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn disclose<F: FnOnce(&mut Frame<'_>)>(
        &mut self,
        label: &str,
        role: Role,
        default_open: bool,
        selected: bool,
        leaf: bool,
        bar: bool,
        body: Option<F>,
    ) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let h = self.ui.theme.row_height;
        let rect = self.alloc(0.0, h, true);
        let mut r = self.interact(id, rect, Sense::CLICK);
        self.set_wants(&r, true, false, false);
        let open = *self.ui.expanded.entry(id).or_insert(default_open);
        let mut now = open;
        if !leaf {
            if r.clicked {
                now = !open;
            }
            if r.focused && self.step_key(Key::Right) {
                now = true;
            }
            if r.focused && self.step_key(Key::Left) {
                now = false;
            }
            if now != open {
                self.ui.expanded.insert(id, now);
                r.changed = true;
            }
        }
        let (accent, text, title) = (
            self.ui.theme.accent,
            self.ui.theme.text,
            self.ui.theme.title,
        );
        if bar {
            self.draw_box(rect, BoxKind::Button, BState::of(&r));
        } else if selected {
            self.fill_rect(rect, accent.with_alpha(0.5));
        } else if r.hovered {
            let c = self.ui.theme.button_hover.with_alpha(0.6);
            self.fill_rect(rect, c);
        }
        let ax = rect.x + 10.0;
        if !leaf {
            self.arrow(
                vec2(ax, rect.y + h * 0.5),
                8.0,
                if now { Arrow::Down } else { Arrow::Right },
                text,
            );
        }
        let tr = Rect::new(rect.x + 22.0, rect.y, rect.w - 22.0, h);
        self.text_in(tr, shown, if bar { title } else { text }, Align::Start);
        self.focus_ring(&r);
        let st = AccessState {
            expanded: (!leaf).then_some(now),
            selected,
            ..AccessState::default()
        };
        let node = self.node(id, role, shown, None, st, rect);
        if now && let Some(body) = body {
            self.push_node(node);
            self.ids.push(id);
            self.indent(if bar { 6.0 } else { 16.0 }, body);
            self.ids.pop();
            self.pop_node();
        }
        r
    }

    /// Whether the tree node or header `id` is open.
    pub fn is_open(&self, id: WidgetId) -> bool {
        self.ui.expanded.get(&id).copied().unwrap_or(false)
    }
}
