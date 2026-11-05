//! Layout containers: rows, columns, grids, spacing, alignment.

use yowie_core::{Rect, Vec2};

use crate::frame::Frame;
use crate::layout::{Align, Layout, Size};

impl Frame<'_> {
    /// Runs `body` in a child layout that `make` builds from the origin and
    /// width the parent gives it, then advances the parent past it.
    pub(crate) fn nested<R>(
        &mut self,
        natural_w: f32,
        fill: bool,
        make: impl FnOnce(Vec2, f32, f32) -> Layout,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let spacing = self.ui.theme.spacing;
        let (start, w) = self.layout().begin_child(natural_w, fill);
        self.layouts.push(make(start.min(), w, spacing));
        let r = body(self);
        let used = self.layouts.pop().map_or(Vec2::ZERO, |mut l| {
            l.finish_line();
            l.used()
        });
        self.layout().end_child(used);
        r
    }

    /// Lays `body`'s widgets out in slots of the given sizes, wrapping to a
    /// new line after the last slot, so a row of three slots with six
    /// widgets is a grid of two lines.
    pub fn row<R>(&mut self, sizes: &[Size], body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        let sizes = sizes.to_vec();
        self.nested(
            0.0,
            true,
            move |o, w, s| Layout::slots(o, w, s, &sizes),
            body,
        )
    }

    /// A row of `n` equal slots.
    pub fn columns<R>(&mut self, n: usize, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        let sizes = vec![Size::Fill(1.0); n.max(1)];
        self.row(&sizes, body)
    }

    /// A grid of `columns` equal columns; widgets fill it row by row.
    pub fn grid<R>(&mut self, columns: usize, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.columns(columns, body)
    }

    /// Places widgets side by side at their natural widths.
    pub fn horizontal<R>(&mut self, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.nested(0.0, true, Layout::horizontal, body)
    }

    /// Stacks widgets vertically inside a group, for use in a row slot or
    /// horizontal line.
    pub fn vertical<R>(&mut self, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.nested(0.0, true, Layout::column, body)
    }

    /// Indents `body` by `amount` view units.
    pub fn indent<R>(&mut self, amount: f32, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.nested(
            0.0,
            true,
            move |o, w, s| Layout::column(o + Vec2::new(amount, 0.0), (w - amount).max(0.0), s),
            body,
        )
    }

    /// Insets `body` by `amount` on the left and right.
    pub fn padded<R>(&mut self, amount: f32, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.nested(
            0.0,
            true,
            move |o, w, s| {
                Layout::column(o + Vec2::new(amount, 0.0), (w - amount * 2.0).max(0.0), s)
            },
            body,
        )
    }

    /// Lines up natural-width widgets (labels, images, swatches) inside
    /// `body` within the container's width.
    pub fn aligned<R>(&mut self, align: Align, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        let old = self.layout().align();
        self.layout().set_align(align);
        let r = body(self);
        self.layout().set_align(old);
        r
    }

    /// Lays `body` out inside `rect` without drawing a background, clipped
    /// to it. For heads-up displays and custom layouts.
    pub fn area<R>(&mut self, rect: Rect, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        let spacing = self.ui.theme.spacing;
        self.push_clip(rect);
        self.layouts
            .push(Layout::column(rect.min(), rect.w, spacing));
        let r = body(self);
        self.layouts.pop();
        self.pop_clip();
        r
    }
}
