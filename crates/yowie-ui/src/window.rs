//! Panels, movable and resizable windows, and modal dialogs.

use yowie_core::{Rect, Vec2, vec2};

use crate::access::{AccessState, Role};
use crate::draw::{BState, BoxKind};
use crate::frame::{Frame, Sense, depth};
use crate::id::{WidgetId, visible_label};
use crate::layout::{Align, Anchor, anchored};
use crate::ui::LayerRec;

/// How a [`Frame::window_with`] behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowOptions {
    /// Whether dragging the title bar moves the window.
    pub movable: bool,
    /// Whether dragging the bottom-right corner resizes the window.
    pub resizable: bool,
    /// Whether the title bar has a close button.
    pub closable: bool,
    /// The smallest size resizing allows, in view units.
    pub min_size: Vec2,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            movable: true,
            resizable: true,
            closable: false,
            min_size: vec2(140.0, 80.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Panel,
    Window,
}

impl Frame<'_> {
    /// Adds a fixed panel at `rect`, with a title bar when `title` is given.
    /// Its contents scroll when they do not fit.
    pub fn panel<R>(
        &mut self,
        title: Option<&str>,
        rect: Rect,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let mut r = rect;
        let opts = WindowOptions {
            movable: false,
            resizable: false,
            ..WindowOptions::default()
        };
        self.container(Kind::Panel, title, opts, &mut r, body).0
    }

    /// Adds a window the player can move and resize. The rectangle lives in
    /// the caller so it can be saved; the window updates it.
    pub fn window<R>(
        &mut self,
        title: &str,
        rect: &mut Rect,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        self.container(
            Kind::Window,
            Some(title),
            WindowOptions::default(),
            rect,
            body,
        )
        .0
    }

    /// Adds a window with the given behaviour. The second result is true
    /// when the close button was pressed this frame.
    pub fn window_with<R>(
        &mut self,
        title: &str,
        options: WindowOptions,
        rect: &mut Rect,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> (R, bool) {
        self.container(Kind::Window, Some(title), options, rect, body)
    }

    fn container<R>(
        &mut self,
        kind: Kind,
        title: Option<&str>,
        opts: WindowOptions,
        rect: &mut Rect,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> (R, bool) {
        let label = title.unwrap_or("panel");
        let id = self.id(label);
        let z = match kind {
            Kind::Panel => depth::PANEL,
            Kind::Window => {
                let order = &mut self.ui.window_order;
                if !order.contains(&id) {
                    order.push(id);
                }
                if self.pressed
                    && self.hover_layer == id
                    && let Some(i) = order.iter().position(|w| *w == id)
                {
                    order.remove(i);
                    order.push(id);
                }
                let rank = order.iter().position(|w| *w == id).unwrap_or(0);
                depth::WINDOW + rank as f32
            }
        };
        self.push_layer(id, z);
        let (pad, row, border) = {
            let t = &self.ui.theme;
            (t.padding, t.row_height, t.border_width)
        };
        let th = if title.is_some() { row + 2.0 } else { 0.0 };
        let mut closed = false;
        if kind == Kind::Window {
            rect.w = rect.w.max(opts.min_size.x);
            rect.h = rect.h.max(opts.min_size.y);
            rect.x = rect
                .x
                .clamp(-(rect.w - 48.0), (self.view.w - 48.0).max(0.0));
            rect.y = rect.y.clamp(0.0, (self.view.h - th.max(24.0)).max(0.0));
        }
        self.draw_box(*rect, BoxKind::Panel, BState::Normal);
        let name_node = self.node(
            id,
            if kind == Kind::Window {
                Role::Window
            } else {
                Role::Panel
            },
            label,
            None,
            AccessState::default(),
            *rect,
        );
        if let Some(t) = title {
            let tr = Rect::new(rect.x, rect.y, rect.w, th);
            self.draw_box(tr, BoxKind::Title, BState::Normal);
            let title_color = self.ui.theme.title;
            let close_w = if opts.closable { th } else { 0.0 };
            let text_r = Rect::new(tr.x + pad, tr.y, tr.w - pad * 2.0 - close_w, th);
            self.text_in(text_r, visible_label(t), title_color, Align::Start);
            if opts.closable {
                let cr = Rect::new(tr.x + tr.w - th, tr.y, th, th);
                let cid = id.child("close");
                let r = self.interact(cid, cr, Sense::CLICK);
                if r.hovered || r.held {
                    let c = self.ui.theme.button_hover;
                    self.fill_rect(cr.inflate(-3.0), c);
                }
                let c = self.ui.theme.text;
                let m = cr.center();
                self.line(m + vec2(-4.0, -4.0), m + vec2(4.0, 4.0), 2.0, c);
                self.line(m + vec2(-4.0, 4.0), m + vec2(4.0, -4.0), 2.0, c);
                self.focus_ring(&r);
                closed = r.clicked;
                self.node(cid, Role::Button, "Close", None, AccessState::default(), cr);
            }
            if opts.movable {
                let drag = Rect::new(tr.x, tr.y, tr.w - close_w, th);
                self.drag_window(id.child("title"), drag, rect);
            }
        }
        if opts.resizable {
            self.resize_grip(id.child("grip"), rect, opts.min_size);
        }
        let top = rect.y + th;
        let content = Rect::new(
            rect.x + pad,
            top + pad,
            (rect.w - pad * 2.0).max(1.0),
            (rect.h - th - pad * 2.0).max(1.0),
        );
        if border > 0.0 && title.is_some() {
            let line = Rect::new(rect.x, top - 1.0, rect.w, 1.0);
            let c = self.ui.theme.border;
            self.fill_rect(line, c);
        }
        self.push_node(name_node);
        self.ids.push(id);
        let out = self.scroll_region(id.child("body"), content, false, body);
        self.ids.pop();
        self.pop_node();
        self.pop_layer(*rect);
        (out, closed)
    }

    fn drag_window(&mut self, id: WidgetId, area: Rect, rect: &mut Rect) {
        let r = self.interact(
            id,
            area,
            Sense {
                click: true,
                focus: false,
            },
        );
        let frame = self.ui.frame_no;
        if r.held {
            let d = self.ui.window_drags.get(id, frame);
            if self.pressed {
                d.grab = self.mouse - rect.min();
            }
            let g = d.grab;
            rect.x = self.mouse.x - g.x;
            rect.y = self.mouse.y - g.y;
        }
    }

    fn resize_grip(&mut self, id: WidgetId, rect: &mut Rect, min: Vec2) {
        let g = Rect::new(rect.x + rect.w - 16.0, rect.y + rect.h - 16.0, 16.0, 16.0);
        let r = self.interact(
            id,
            g,
            Sense {
                click: true,
                focus: false,
            },
        );
        let frame = self.ui.frame_no;
        if r.held {
            let d = self.ui.window_drags.get(id, frame);
            if self.pressed {
                d.grab = rect.max() - self.mouse;
            }
            let grab = d.grab;
            rect.w = (self.mouse.x + grab.x - rect.x).max(min.x);
            rect.h = (self.mouse.y + grab.y - rect.y).max(min.y);
        }
        let c = if r.hovered || r.held {
            self.ui.theme.accent
        } else {
            self.ui.theme.text_dim
        };
        let (br, w) = (g.max(), 1.5);
        for k in [4.0, 8.0, 12.0] {
            self.line(vec2(br.x - k, br.y - 2.0), vec2(br.x - 2.0, br.y - k), w, c);
        }
    }

    /// Adds a centred dialog `size` large while `*open` is true, dimming and
    /// blocking everything behind it. Escape or a gamepad's east button
    /// closes it. Returns what `body` returns while the dialog is shown.
    pub fn modal<R>(
        &mut self,
        title: &str,
        open: &mut bool,
        size: Vec2,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> Option<R> {
        if !*open {
            return None;
        }
        let id = self.id(title);
        let rect = anchored(self.view, Anchor::Center, size, 0.0);
        let (pad, row) = (self.ui.theme.padding, self.ui.theme.row_height);
        let th = row + 2.0;
        if self.nav.cancel && self.ui.popup == WidgetId::NONE {
            *open = false;
        }
        let out = self.unclipped(|f| {
            f.push_layer(id, depth::MODAL);
            let backdrop = f.ui.theme.backdrop;
            let view = f.view;
            f.fill_rect(view, backdrop);
            f.draw_box(rect, BoxKind::Panel, BState::Normal);
            let tr = Rect::new(rect.x, rect.y, rect.w, th);
            f.draw_box(tr, BoxKind::Title, BState::Normal);
            let tc = f.ui.theme.title;
            f.text_in(
                Rect::new(tr.x + pad, tr.y, tr.w - pad * 2.0, th),
                visible_label(title),
                tc,
                Align::Start,
            );
            let content = Rect::new(
                rect.x + pad,
                rect.y + th + pad,
                rect.w - pad * 2.0,
                rect.h - th - pad * 2.0,
            );
            let node = f.node(id, Role::Dialog, title, None, AccessState::default(), rect);
            f.push_node(node);
            f.ids.push(id);
            let r = f.scroll_region(id.child("body"), content, false, body);
            f.ids.pop();
            f.pop_node();
            f.pop_layer(rect);
            f.ui.modal = Some(LayerRec {
                id,
                z: depth::MODAL,
                rect,
            });
            r
        });
        Some(out)
    }
}
