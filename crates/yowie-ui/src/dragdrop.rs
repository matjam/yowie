//! Drag and drop between widgets.

use std::any::Any;

use crate::frame::Frame;
use crate::response::Response;
use crate::ui::DragPayload;

/// How far the pointer must move with the button held before a press on a
/// drag source becomes a drag, in view units.
const DRAG_START: f32 = 5.0;

impl Frame<'_> {
    /// Makes the widget `r` (added just before) a drag source carrying
    /// `payload`. Once the pointer moves a few units with the button held,
    /// the drag starts and `label` follows the pointer. Returns true while
    /// the drag started from this widget is in progress.
    ///
    /// A release over the source widget still reports `clicked` on its
    /// response; check this method's result to tell a drag from a click.
    pub fn drag_source<T: Any>(&mut self, r: &Response, payload: T, label: &str) -> bool {
        if self.ui.drag.is_none()
            && r.held
            && self.mouse.distance(self.ui.press_origin) > DRAG_START
        {
            self.ui.drag = Some(DragPayload {
                source: r.id,
                payload: Box::new(payload),
                label: label.to_owned(),
            });
        }
        self.ui.drag.as_ref().is_some_and(|d| d.source == r.id)
    }

    /// Makes the widget `r` a drop target for payloads of type `T`. While a
    /// matching drag is over it, the widget is outlined; the frame the
    /// button is released over it, the payload is returned.
    pub fn drop_target<T: Any>(&mut self, r: &Response) -> Option<T> {
        let accepts = self.ui.drag.as_ref().is_some_and(|d| d.payload.is::<T>());
        if !accepts || !self.pointer_over(r.rect) {
            return None;
        }
        let accent = self.ui.theme.accent;
        self.stroke_rect(r.rect, 2.0, accent);
        if !self.released {
            return None;
        }
        let drag = self.ui.drag.take()?;
        drag.payload.downcast::<T>().ok().map(|b| *b)
    }

    /// The payload of the drag in progress, if it is a `T`.
    pub fn dragging<T: Any>(&self) -> Option<&T> {
        self.ui
            .drag
            .as_ref()
            .and_then(|d| d.payload.downcast_ref::<T>())
    }

    /// Whether any drag is in progress.
    pub fn is_dragging(&self) -> bool {
        self.ui.drag.is_some()
    }
}
