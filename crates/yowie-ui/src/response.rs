//! What a widget reports back.

use yowie_core::Rect;

use crate::id::WidgetId;

/// The outcome of adding a widget this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Response {
    /// The widget's id, for [`Ui::set_focus`](crate::Ui::set_focus) and
    /// the accessibility tree.
    pub id: WidgetId,
    /// Where the widget was placed, in view units.
    pub rect: Rect,
    /// The pointer is over the widget and nothing else holds it.
    pub hovered: bool,
    /// The pointer button went down on the widget and is still down.
    pub held: bool,
    /// The widget was activated: released after pressing on it, or
    /// confirmed from the keyboard or a gamepad while focused.
    pub clicked: bool,
    /// The widget was clicked twice quickly.
    pub double_clicked: bool,
    /// The right button was released over the widget.
    pub right_clicked: bool,
    /// The value the widget edits changed this frame.
    pub changed: bool,
    /// The widget has keyboard focus.
    pub focused: bool,
    /// Enter was pressed in a single-line text field.
    pub submitted: bool,
}

impl Response {
    pub(crate) fn blank(id: WidgetId, rect: Rect) -> Response {
        Response {
            id,
            rect,
            ..Response::default()
        }
    }
}
