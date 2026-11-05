//! Menu bars, menus, submenus and context menus.

use yowie_core::{Rect, Vec2, vec2};
use yowie_input::Key;

use crate::access::{AccessState, Role};
use crate::draw::{Arrow, BState, BoxKind};
use crate::frame::{Frame, MenuFrame, Sense, depth};
use crate::id::{WidgetId, visible_label};
use crate::layout::{Align, Layout};
use crate::response::Response;

const INSET: f32 = 3.0;

impl Frame<'_> {
    /// Adds a bar across the container whose `body` adds [`menu`](Self::menu)
    /// titles side by side.
    pub fn menu_bar<R>(&mut self, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        let id = self.id("menu bar");
        let h = self.ui.theme.row_height + 2.0;
        let bar = self.alloc(0.0, h, true);
        self.draw_box(bar, BoxKind::Title, BState::Normal);
        let node = self.node(
            id,
            Role::MenuBar,
            "menu bar",
            None,
            AccessState::default(),
            bar,
        );
        self.push_node(node);
        self.ids.push(id);
        self.push_clip(bar);
        self.layouts
            .push(Layout::horizontal(bar.min() + vec2(INSET, 0.0), bar.w, 0.0));
        let r = body(self);
        self.layouts.pop();
        self.pop_clip();
        self.ids.pop();
        self.pop_node();
        r
    }

    /// Adds a menu: a title in a [`menu_bar`](Self::menu_bar) or a submenu
    /// row inside another menu. While it is open, `body` adds its items.
    /// The response's `changed` says the menu opened or closed.
    pub fn menu(&mut self, label: &str, body: impl FnOnce(&mut Frame<'_>)) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let level = self.menus.len();
        let h = self.ui.theme.row_height;
        let pad = self.ui.theme.padding;
        let s = self.measure(shown);
        let rect = if level == 0 {
            self.alloc(s.x + pad * 2.0, h + 2.0, false)
        } else {
            self.alloc(s.x + pad * 3.0, h, true)
        };
        let mut r = self.interact(
            id,
            rect,
            Sense {
                click: true,
                focus: false,
            },
        );
        let was_open = self.ui.menu_path.get(level) == Some(&id);
        if level == 0 {
            if r.clicked {
                self.ui.menu_path.clear();
                if !was_open {
                    self.ui.menu_path.push(id);
                }
            } else if r.hovered && !self.ui.menu_path.is_empty() && !was_open {
                self.ui.menu_path.clear();
                self.ui.menu_path.push(id);
            }
        } else {
            let index = self.next_menu_index();
            let (cursor, deepest) = self.menu_cursor(level);
            let by_key = cursor == index && deepest && self.step_key(Key::Right);
            if (r.hovered && self.mouse != self.ui.mouse_prev) || r.clicked || by_key {
                self.menu_hover(level, index);
                self.ui.menu_path.truncate(level);
                self.ui.menu_path.push(id);
            }
        }
        let open = self.ui.menu_path.get(level) == Some(&id);
        r.changed = open != was_open;
        let (hover, accent, text) = {
            let t = &self.ui.theme;
            (t.button_hover, t.accent, t.text)
        };
        if open {
            self.fill_rect(rect, accent.with_alpha(0.5));
        } else if r.hovered {
            self.fill_rect(rect, hover);
        } else if level > 0 && self.menu_cursor(level).0 == self.menu_index(level) - 1 {
            self.fill_rect(rect, hover.with_alpha(0.6));
        }
        if level == 0 {
            self.text_in(rect, shown, text, Align::Center);
        } else {
            self.text_in(
                Rect::new(rect.x + pad, rect.y, rect.w - pad * 2.0, h),
                shown,
                text,
                Align::Start,
            );
            self.arrow(
                vec2(rect.x + rect.w - pad - 2.0, rect.y + h * 0.5),
                8.0,
                Arrow::Right,
                text,
            );
        }
        let st = AccessState {
            expanded: Some(open),
            ..AccessState::default()
        };
        let role = if level == 0 {
            Role::Menu
        } else {
            Role::MenuItem
        };
        let node = self.node(id, role, shown, None, st, rect);
        if open {
            let pos = if level == 0 {
                vec2(rect.x, rect.y + rect.h)
            } else {
                vec2(rect.x + rect.w - 2.0, rect.y)
            };
            self.push_node(node);
            self.popup_menu(id, level, pos, rect, body);
            self.pop_node();
            if level == 0 && self.pressed && !rect.contains(self.mouse) {
                self.close_menus_if_outside();
            }
        }
        r
    }

    /// Opens a context menu when `target` is right-clicked; `body` adds the
    /// items while it is open.
    pub fn context_menu(&mut self, target: &Response, body: impl FnOnce(&mut Frame<'_>)) {
        let id = target.id.child("context menu");
        if target.right_clicked {
            self.ui.menu_path.clear();
            self.ui.menu_path.push(id);
            self.ui.context_menu = Some((id, self.mouse));
        }
        if self.menus.is_empty()
            && self.ui.menu_path.first() == Some(&id)
            && let Some((cid, pos)) = self.ui.context_menu
            && cid == id
        {
            self.popup_menu(id, 0, pos, Rect::ZERO, body);
            if self.pressed && !target.rect.contains(self.mouse)
                || self.pressed && target.right_clicked
            {
                self.close_menus_if_outside();
            }
        }
    }

    fn close_menus_if_outside(&mut self) {
        let m = self.mouse;
        let inside = self
            .ui
            .layers
            .iter()
            .any(|l| self.ui.menu_path.contains(&l.id) && l.rect.contains(m));
        if !inside {
            self.ui.menu_path.clear();
        }
    }

    fn next_menu_index(&mut self) -> usize {
        match self.menus.last_mut() {
            Some(m) => {
                m.index += 1;
                m.index - 1
            }
            None => 0,
        }
    }

    fn menu_index(&self, level: usize) -> usize {
        self.menus.get(level.wrapping_sub(1)).map_or(0, |m| m.index)
    }

    /// The highlighted item of the popup at `level` and whether that popup
    /// is the deepest open one, so it owns the keyboard.
    fn menu_cursor(&self, level: usize) -> (usize, bool) {
        let m = self.menus.get(level.wrapping_sub(1));
        (
            m.map_or(usize::MAX, |m| m.cursor),
            self.ui.menu_path.len() == level,
        )
    }

    fn menu_hover(&mut self, level: usize, index: usize) {
        if let Some(m) = self.menus.get_mut(level.wrapping_sub(1)) {
            m.cursor = index;
        }
    }

    fn popup_menu(
        &mut self,
        id: WidgetId,
        level: usize,
        pos: Vec2,
        _title: Rect,
        body: impl FnOnce(&mut Frame<'_>),
    ) {
        let frame = self.ui.frame_no;
        let (size, count, cursor) = {
            let st = self.ui.lists.get(id, frame);
            let size = if st.size == Vec2::ZERO {
                vec2(150.0, 60.0)
            } else {
                st.size
            };
            (size, st.count, st.cursor)
        };
        let view = self.view;
        let at = vec2(
            pos.x.min(view.w - size.x).max(0.0),
            pos.y.min(view.h - size.y).max(0.0),
        );
        let prect = Rect::new(at.x, at.y, size.x, size.y);
        let deepest = self.ui.menu_path.len() == level + 1;
        let mut cursor = cursor;
        if deepest && count > 0 {
            if self.step_key(Key::Up) {
                cursor = if cursor == 0 || cursor >= count {
                    count - 1
                } else {
                    cursor - 1
                };
            }
            if self.step_key(Key::Down) {
                cursor = if cursor + 1 >= count { 0 } else { cursor + 1 };
            }
        }
        if deepest && (self.nav.cancel || (level > 0 && self.step_key(Key::Left))) {
            self.ui.menu_path.truncate(level);
            return;
        }
        let activate =
            deepest && (self.input.key_pressed(Key::Enter) || self.input.key_pressed(Key::Space));
        self.unclipped(|f| {
            f.push_layer(id, depth::POPUP + 1.0 + level as f32);
            f.draw_box(prect, BoxKind::Panel, BState::Normal);
            let ac = f.ui.theme.border;
            f.stroke_rect(prect, 1.0, ac);
            f.menus.push(MenuFrame { index: 0, cursor });
            f.activate_menu = activate;
            f.ids.push(id);
            f.layouts.push(Layout::column(
                prect.min() + vec2(INSET, INSET),
                size.x - INSET * 2.0,
                0.0,
            ));
            body(f);
            let used = f.layouts.pop().map_or(Vec2::ZERO, |l| l.used());
            f.ids.pop();
            let done = f.menus.pop();
            let st = f.ui.lists.get(id, frame);
            st.size = used + vec2(INSET * 2.0, INSET * 2.0);
            if let Some(m) = done {
                st.count = m.index;
                st.cursor = m.cursor;
            }
            f.pop_layer(prect);
        });
    }

    /// Adds an item to the open menu. `clicked` on the response says it
    /// was chosen with the pointer, Enter or Space; choosing closes the
    /// menus.
    pub fn menu_item(&mut self, label: &str) -> Response {
        self.menu_item_with(label, "", None)
    }

    /// Adds a menu item with a shortcut hint on the right and, when
    /// `checked` is given, a check mark.
    pub fn menu_item_with(
        &mut self,
        label: &str,
        shortcut: &str,
        checked: Option<bool>,
    ) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let level = self.menus.len();
        let (h, pad) = (self.ui.theme.row_height, self.ui.theme.padding);
        let s = self.measure(shown);
        let sc = if shortcut.is_empty() {
            0.0
        } else {
            self.measure(shortcut).x + pad * 2.0
        };
        let rect = self.alloc(s.x + sc + pad * 4.0, h, true);
        let index = self.next_menu_index();
        let mut r = self.interact(
            id,
            rect,
            Sense {
                click: true,
                focus: false,
            },
        );
        let (cursor, deepest) = self.menu_cursor(level);
        if r.hovered && self.mouse != self.ui.mouse_prev {
            self.menu_hover(level, index);
            self.ui.menu_path.truncate(level);
        }
        let selected = cursor == index && deepest;
        if selected && self.activate_menu {
            r.clicked = true;
        }
        if r.clicked {
            self.ui.menu_path.clear();
        }
        let (hover, accent, text, dim) = {
            let t = &self.ui.theme;
            (t.button_hover, t.accent, t.text, t.text_dim)
        };
        if r.hovered || selected {
            self.fill_rect(
                rect,
                if selected {
                    accent.with_alpha(0.5)
                } else {
                    hover
                },
            );
        }
        if checked == Some(true) {
            let m = vec2(rect.x + pad, rect.y + h * 0.5);
            self.line(m + vec2(-3.0, 0.0), m + vec2(-1.0, 3.0), 2.0, text);
            self.line(m + vec2(-1.0, 3.0), m + vec2(4.0, -3.0), 2.0, text);
        }
        let tr = Rect::new(rect.x + pad * 2.0, rect.y, rect.w - pad * 3.0, h);
        self.text_in(tr, shown, text, Align::Start);
        if !shortcut.is_empty() {
            self.text_in(
                Rect::new(rect.x, rect.y, rect.w - pad, h),
                shortcut,
                dim,
                Align::End,
            );
        }
        let st = AccessState {
            checked,
            ..AccessState::default()
        };
        self.node(id, Role::MenuItem, shown, None, st, rect);
        r
    }

    /// Adds a dividing line to the open menu.
    pub fn menu_separator(&mut self) {
        let r = self.alloc(0.0, 5.0, true);
        let c = self.ui.theme.border;
        self.fill_rect(Rect::new(r.x + 4.0, r.y + 2.0, r.w - 8.0, 1.0), c);
    }
}
