//! Scrolling containers with wheel, scrollbar and keyboard-focus support.

use yowie_core::{Rect, Vec2};
use yowie_input::Modifiers;

use crate::access::{AccessState, Role};
use crate::draw::{BState, BoxKind};
use crate::frame::{Frame, Sense};
use crate::id::WidgetId;
use crate::layout::Layout;

const BAR: f32 = 9.0;

impl Frame<'_> {
    /// Adds a bordered area `height` tall and as wide as the container
    /// whose contents scroll when they do not fit.
    pub fn scroll_area<R>(
        &mut self,
        label: &str,
        height: f32,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        self.scroll_area_with(label, height, false, body)
    }

    /// Like [`scroll_area`](Self::scroll_area), optionally scrolling
    /// sideways too when the contents are wider than the area.
    pub fn scroll_area_with<R>(
        &mut self,
        label: &str,
        height: f32,
        horizontal: bool,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let id = self.id(label);
        let rect = self.alloc(0.0, height, true);
        self.draw_box(rect, BoxKind::Field, BState::Normal);
        let node = self.node(id, Role::Group, label, None, AccessState::default(), rect);
        self.push_node(node);
        self.ids.push(id);
        let inner = rect.inflate(-self.ui.theme.border_width.max(1.0));
        let r = self.scroll_region(id, inner, horizontal, body);
        self.ids.pop();
        self.pop_node();
        r
    }

    /// The scroll offset of the scroll area `id`, in view units from the
    /// top-left of its contents. To get the id of an area by its label,
    /// call [`peek_id`](Frame::peek_id) before adding the area.
    pub fn scroll_offset(&mut self, id: WidgetId) -> Vec2 {
        let frame = self.ui.frame_no;
        self.ui.scrolls.get(id, frame).offset
    }

    /// Scrolls the area `id` to `offset` at the next frame.
    pub fn set_scroll_offset(&mut self, id: WidgetId, offset: Vec2) {
        let frame = self.ui.frame_no;
        self.ui.scrolls.get(id, frame).offset = offset;
    }

    /// Lays `body` out inside `rect`, scrolled by the offset kept under
    /// `id`, clipped to `rect` and with scrollbars when the contents
    /// overflow.
    pub(crate) fn scroll_region<R>(
        &mut self,
        id: WidgetId,
        rect: Rect,
        horizontal: bool,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let frame = self.ui.frame_no;
        let mut st = *self.ui.scrolls.get(id, frame);
        let need_v = st.content.y > rect.h + 0.5;
        let need_h = horizontal && st.content.x > rect.w - if need_v { BAR } else { 0.0 } + 0.5;
        let vw = rect.w - if need_v { BAR + 2.0 } else { 0.0 };
        let vh = rect.h - if need_h { BAR + 2.0 } else { 0.0 };
        let viewport = Rect::new(rect.x, rect.y, vw.max(1.0), vh.max(1.0));
        let pad = self.ui.theme.spacing;
        if self.ui.scroll_to_focus {
            let focus = self.ui.focus;
            let target = self
                .ui
                .focusables_prev
                .iter()
                .find(|f| f.id == focus && f.scroll == id)
                .map(|f| f.rect);
            if let Some(fr) = target {
                if fr.y < viewport.y {
                    st.offset.y -= viewport.y - fr.y + pad;
                } else if fr.y + fr.h > viewport.y + viewport.h {
                    st.offset.y += fr.y + fr.h - (viewport.y + viewport.h) + pad;
                }
                if fr.x < viewport.x {
                    st.offset.x -= viewport.x - fr.x + pad;
                } else if fr.x + fr.w > viewport.x + viewport.w {
                    st.offset.x += fr.x + fr.w - (viewport.x + viewport.w) + pad;
                }
            }
        }
        let max = (st.content - viewport.size()).max(Vec2::ZERO);
        st.offset = st.offset.clamp(Vec2::ZERO, max);
        self.push_clip(viewport);
        let width = if horizontal {
            viewport.w.max(st.content.x)
        } else {
            viewport.w
        };
        let spacing = self.ui.theme.spacing;
        self.layouts
            .push(Layout::column(viewport.min() - st.offset, width, spacing));
        self.scroll_stack.push(id);
        let out = body(self);
        self.scroll_stack.pop();
        let used = self.layouts.pop().map_or(Vec2::ZERO, |l| l.used());
        self.pop_clip();
        st.content = used;
        st.viewport = viewport.size();
        let max = (used - viewport.size()).max(Vec2::ZERO);
        self.wheel_scroll(&mut st, viewport, max);
        let bars = [
            (1usize, used.y > viewport.h + 0.5),
            (0usize, horizontal && used.x > viewport.w + 0.5),
        ];
        for (axis, show) in bars {
            if show {
                self.scrollbar(id, &mut st, rect, viewport, max, axis);
            }
        }
        st.offset = st.offset.clamp(Vec2::ZERO, max);
        *self.ui.scrolls.get(id, frame) = st;
        out
    }

    fn wheel_scroll(&mut self, st: &mut crate::ui::ScrollState, viewport: Rect, max: Vec2) {
        if !self.pointer_over(viewport) || self.wheel == Vec2::ZERO {
            return;
        }
        let step = self.ui.theme.row_height * 1.5;
        let shift = self.mods.contains(Modifiers::SHIFT);
        let (mut wx, mut wy) = (self.wheel.x, self.wheel.y);
        if shift {
            std::mem::swap(&mut wx, &mut wy);
        }
        if wy != 0.0 && max.y > 0.0 {
            st.offset.y = (st.offset.y - wy * step).clamp(0.0, max.y);
            if shift {
                self.wheel.x = 0.0;
            } else {
                self.wheel.y = 0.0;
            }
        }
        if wx != 0.0 && max.x > 0.0 {
            st.offset.x = (st.offset.x - wx * step).clamp(0.0, max.x);
            if shift {
                self.wheel.y = 0.0;
            } else {
                self.wheel.x = 0.0;
            }
        }
    }

    fn scrollbar(
        &mut self,
        id: WidgetId,
        st: &mut crate::ui::ScrollState,
        rect: Rect,
        viewport: Rect,
        max: Vec2,
        axis: usize,
    ) {
        let (track, len, content, view_len, off) = if axis == 1 {
            let t = Rect::new(rect.x + rect.w - BAR, rect.y, BAR, viewport.h);
            (t, t.h, max.y + viewport.h, viewport.h, st.offset.y)
        } else {
            let t = Rect::new(rect.x, rect.y + rect.h - BAR, viewport.w, BAR);
            (t, t.w, max.x + viewport.w, viewport.w, st.offset.x)
        };
        let travel_max = if axis == 1 { max.y } else { max.x };
        let thumb_len = (view_len / content * len).clamp(18.0_f32.min(len), len);
        let thumb_pos = if travel_max > 0.0 {
            off / travel_max * (len - thumb_len)
        } else {
            0.0
        };
        let thumb = if axis == 1 {
            Rect::new(track.x, track.y + thumb_pos, BAR, thumb_len)
        } else {
            Rect::new(track.x + thumb_pos, track.y, thumb_len, BAR)
        };
        let bar_id = id.child(if axis == 1 { "vbar" } else { "hbar" });
        let r = self.interact(
            bar_id,
            track,
            Sense {
                click: true,
                focus: false,
            },
        );
        if r.held {
            let m = if axis == 1 {
                self.mouse.y
            } else {
                self.mouse.x
            };
            let tpos = if axis == 1 { thumb.y } else { thumb.x };
            if self.pressed {
                let grab = if thumb.contains(self.mouse) {
                    m - tpos
                } else {
                    thumb_len * 0.5
                };
                st.grab = Some((axis, grab));
            }
            if let Some((a, g)) = st.grab
                && a == axis
            {
                let origin = if axis == 1 { track.y } else { track.x };
                let t = ((m - g - origin) / (len - thumb_len).max(1.0)).clamp(0.0, 1.0);
                if axis == 1 {
                    st.offset.y = t * max.y;
                } else {
                    st.offset.x = t * max.x;
                }
            }
        } else if st.grab.is_some_and(|(a, _)| a == axis) {
            st.grab = None;
        }
        self.draw_box(track, BoxKind::Track, BState::Normal);
        let state = if r.held {
            BState::Active
        } else if r.hovered {
            BState::Hover
        } else {
            BState::Normal
        };
        self.draw_box(thumb.inflate(-1.0), BoxKind::Button, state);
    }
}
