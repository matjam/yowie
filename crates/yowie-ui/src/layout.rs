//! Layout arithmetic: anchors, splits, slot sizes and the flow that places
//! widgets one after another. Nothing here draws or reads input.

use yowie_core::{Rect, Vec2, vec2};

/// Where a rectangle sits inside an area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Top-left corner.
    TopLeft,
    /// Top edge, centred.
    Top,
    /// Top-right corner.
    TopRight,
    /// Left edge, centred.
    Left,
    /// Centre.
    Center,
    /// Right edge, centred.
    Right,
    /// Bottom-left corner.
    BottomLeft,
    /// Bottom edge, centred.
    Bottom,
    /// Bottom-right corner.
    BottomRight,
}

/// Places a `size` rectangle in `area` at `anchor`, `margin` away from the
/// edges it touches.
pub fn anchored(area: Rect, anchor: Anchor, size: Vec2, margin: f32) -> Rect {
    use Anchor::*;
    let x = match anchor {
        TopLeft | Left | BottomLeft => area.x + margin,
        Top | Center | Bottom => area.x + (area.w - size.x) * 0.5,
        TopRight | Right | BottomRight => area.x + area.w - size.x - margin,
    };
    let y = match anchor {
        TopLeft | Top | TopRight => area.y + margin,
        Left | Center | Right => area.y + (area.h - size.y) * 0.5,
        BottomLeft | Bottom | BottomRight => area.y + area.h - size.y - margin,
    };
    Rect::new(x, y, size.x, size.y)
}

/// The part of `area` left after trimming the given distance from each
/// edge. The result never has a negative size.
pub fn stretched(area: Rect, left: f32, top: f32, right: f32, bottom: f32) -> Rect {
    Rect::new(
        area.x + left,
        area.y + top,
        (area.w - left - right).max(0.0),
        (area.h - top - bottom).max(0.0),
    )
}

/// `area` shrunk by `amount` on every side.
pub fn padded(area: Rect, amount: f32) -> Rect {
    stretched(area, amount, amount, amount, amount)
}

/// Splits `area` side by side: the first part takes fraction `t` of the
/// width left after `gap`.
pub fn split_horizontal(area: Rect, t: f32, gap: f32) -> (Rect, Rect) {
    let usable = (area.w - gap).max(0.0);
    let first = usable * t.clamp(0.0, 1.0);
    (
        Rect::new(area.x, area.y, first, area.h),
        Rect::new(area.x + first + gap, area.y, usable - first, area.h),
    )
}

/// Splits `area` top and bottom: the first part takes fraction `t` of the
/// height left after `gap`.
pub fn split_vertical(area: Rect, t: f32, gap: f32) -> (Rect, Rect) {
    let usable = (area.h - gap).max(0.0);
    let first = usable * t.clamp(0.0, 1.0);
    (
        Rect::new(area.x, area.y, area.w, first),
        Rect::new(area.x, area.y + first + gap, area.w, usable - first),
    )
}

/// How an item lines up inside the space it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Against the start edge.
    #[default]
    Start,
    /// Centred.
    Center,
    /// Against the end edge.
    End,
}

impl Align {
    /// The offset of an item of `size` within `space`.
    pub fn offset(self, space: f32, size: f32) -> f32 {
        match self {
            Align::Start => 0.0,
            Align::Center => ((space - size) * 0.5).max(0.0),
            Align::End => (space - size).max(0.0),
        }
    }
}

/// The width of one slot in a row or grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// An exact width in view units.
    Fixed(f32),
    /// A share of the width left after fixed slots and gaps, in proportion
    /// to the weight.
    Fill(f32),
}

/// Resolves slot sizes into widths that, with `spacing` between them, fill
/// `total`. Fixed slots keep their width; fill slots share the rest.
pub fn solve_slots(total: f32, spacing: f32, sizes: &[Size]) -> Vec<f32> {
    let gaps = spacing * sizes.len().saturating_sub(1) as f32;
    let fixed: f32 = sizes
        .iter()
        .map(|s| if let Size::Fixed(w) = s { *w } else { 0.0 })
        .sum();
    let weights: f32 = sizes
        .iter()
        .map(|s| {
            if let Size::Fill(w) = s {
                w.max(0.0)
            } else {
                0.0
            }
        })
        .sum();
    let rest = (total - gaps - fixed).max(0.0);
    sizes
        .iter()
        .map(|s| match s {
            Size::Fixed(w) => *w,
            Size::Fill(w) if weights > 0.0 => rest * w.max(0.0) / weights,
            Size::Fill(_) => 0.0,
        })
        .collect()
}

#[derive(Clone, Debug)]
enum Flow {
    /// Items stack downward, each as wide as the container.
    Column,
    /// Items sit side by side at their natural width.
    Horizontal,
    /// Items fill slots left to right, wrapping after the last slot.
    Slots { widths: Vec<f32>, index: usize },
}

/// Places items one after another inside a container.
#[derive(Clone, Debug)]
pub struct Layout {
    origin: Vec2,
    width: f32,
    spacing: f32,
    flow: Flow,
    cursor: Vec2,
    line_height: f32,
    extent: Vec2,
    align: Align,
}

impl Layout {
    /// A column of full-width items starting at `origin`.
    pub fn column(origin: Vec2, width: f32, spacing: f32) -> Layout {
        Layout {
            origin,
            width,
            spacing,
            flow: Flow::Column,
            cursor: origin,
            line_height: 0.0,
            extent: origin,
            align: Align::Start,
        }
    }

    /// A line of natural-width items starting at `origin`.
    pub fn horizontal(origin: Vec2, width: f32, spacing: f32) -> Layout {
        Layout {
            flow: Flow::Horizontal,
            ..Layout::column(origin, width, spacing)
        }
    }

    /// Slots of the given sizes, repeating row after row.
    pub fn slots(origin: Vec2, width: f32, spacing: f32, sizes: &[Size]) -> Layout {
        Layout {
            flow: Flow::Slots {
                widths: solve_slots(width, spacing, sizes),
                index: 0,
            },
            ..Layout::column(origin, width, spacing)
        }
    }

    /// Sets how natural-width items line up in a column.
    pub fn set_align(&mut self, align: Align) {
        self.align = align;
    }

    /// How natural-width items line up in a column.
    pub fn align(&self) -> Align {
        self.align
    }

    /// The width of the container.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The width the next item gets when it fills: the container, or the
    /// next slot.
    pub fn available_width(&self) -> f32 {
        match &self.flow {
            Flow::Column => self.width,
            Flow::Horizontal => (self.width - (self.cursor.x - self.origin.x)).max(0.0),
            Flow::Slots { widths, index } => widths.get(*index).copied().unwrap_or(self.width),
        }
    }

    /// Reserves space for an item of height `h`. In a column the item is
    /// `natural_w` wide, or the container's width when `fill`; in a line it
    /// is `natural_w` wide; in a slot it fills the slot.
    pub fn allocate(&mut self, natural_w: f32, h: f32, fill: bool) -> Rect {
        let (rect, _) = self.begin_child(natural_w, fill);
        let rect = Rect::new(rect.x, rect.y, rect.w, h);
        self.end_child(rect.size());
        rect
    }

    /// Starts a child container at the next position. Returns its origin
    /// and width (the rectangle's height is zero); call
    /// [`end_child`](Self::end_child) with the size the child used.
    pub fn begin_child(&mut self, natural_w: f32, fill: bool) -> (Rect, f32) {
        let avail = self.available_width();
        let (x, w) = match &self.flow {
            Flow::Column if fill => (self.cursor.x, avail),
            Flow::Column => {
                let w = natural_w.min(avail);
                (self.cursor.x + self.align.offset(avail, w), w)
            }
            Flow::Horizontal => (self.cursor.x, natural_w),
            Flow::Slots { .. } => (self.cursor.x, avail),
        };
        (Rect::new(x, self.cursor.y, w, 0.0), w)
    }

    /// Finishes a child started with [`begin_child`](Self::begin_child) that
    /// used `size`, moving on to the next position.
    pub fn end_child(&mut self, size: Vec2) {
        let x0 = self.cursor.x;
        let y0 = self.cursor.y;
        self.extent = self.extent.max(vec2(x0 + size.x, y0 + size.y));
        self.line_height = self.line_height.max(size.y);
        match &mut self.flow {
            Flow::Column => {
                self.cursor.y += size.y + self.spacing;
                self.line_height = 0.0;
            }
            Flow::Horizontal => {
                self.cursor.x += size.x + self.spacing;
            }
            Flow::Slots { widths, index } => {
                let w = widths.get(*index).copied().unwrap_or(size.x);
                self.cursor.x += w + self.spacing;
                *index += 1;
                if *index >= widths.len() {
                    *index = 0;
                    self.cursor.x = self.origin.x;
                    self.cursor.y += self.line_height + self.spacing;
                    self.line_height = 0.0;
                }
            }
        }
    }

    /// Adds vertical space below the current line.
    pub fn space(&mut self, amount: f32) {
        self.finish_line();
        self.cursor.y += amount;
    }

    /// Ends a partly filled line of a line or slot flow, so the next item
    /// starts below it.
    pub fn finish_line(&mut self) {
        if self.line_height > 0.0 || self.cursor.x > self.origin.x {
            self.cursor.x = self.origin.x;
            self.cursor.y += self.line_height + self.spacing;
            self.line_height = 0.0;
            if let Flow::Slots { index, .. } = &mut self.flow {
                *index = 0;
            }
        }
    }

    /// The size the items used so far, from the origin, without the
    /// trailing gap.
    pub fn used(&self) -> Vec2 {
        let end = self.extent;
        vec2(
            (end.x - self.origin.x).max(0.0),
            (end.y - self.origin.y).max(0.0),
        )
    }

    /// The next position, for widgets that draw at the cursor.
    pub fn cursor(&self) -> Vec2 {
        self.cursor
    }

    /// Moves the start edge right by `amount`; later items are narrower.
    pub fn indent(&mut self, amount: f32) {
        self.origin.x += amount;
        self.cursor.x += amount;
        self.width = (self.width - amount).max(0.0);
        if let Flow::Slots { widths, .. } = &mut self.flow {
            let total: f32 = widths.iter().sum();
            if total > 0.0 {
                let k = self.width / (total + self.spacing * widths.len().saturating_sub(1) as f32);
                for w in widths {
                    *w *= k.min(1.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_place_rectangles() {
        let area = Rect::new(100.0, 50.0, 400.0, 300.0);
        let size = vec2(40.0, 20.0);
        let cases = [
            (Anchor::TopLeft, Rect::new(110.0, 60.0, 40.0, 20.0)),
            (Anchor::Center, Rect::new(280.0, 190.0, 40.0, 20.0)),
            (Anchor::BottomRight, Rect::new(450.0, 320.0, 40.0, 20.0)),
            (Anchor::Top, Rect::new(280.0, 60.0, 40.0, 20.0)),
            (Anchor::Left, Rect::new(110.0, 190.0, 40.0, 20.0)),
        ];
        for (a, want) in cases {
            assert_eq!(anchored(area, a, size, 10.0), want, "{a:?}");
        }
    }

    #[test]
    fn stretch_and_split() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(
            stretched(a, 10.0, 5.0, 10.0, 5.0),
            Rect::new(10.0, 5.0, 80.0, 40.0)
        );
        assert_eq!(stretched(a, 80.0, 0.0, 80.0, 0.0).w, 0.0);
        let (l, r) = split_horizontal(a, 0.25, 4.0);
        assert_eq!(l, Rect::new(0.0, 0.0, 24.0, 50.0));
        assert_eq!(r, Rect::new(28.0, 0.0, 72.0, 50.0));
        let (t, b) = split_vertical(a, 0.5, 0.0);
        assert_eq!((t.h, b.y, b.h), (25.0, 25.0, 25.0));
    }

    #[test]
    fn slots_share_the_remaining_width() {
        let w = solve_slots(
            200.0,
            10.0,
            &[Size::Fixed(50.0), Size::Fill(1.0), Size::Fill(3.0)],
        );
        assert_eq!(w, vec![50.0, 32.5, 97.5]);
        assert_eq!(w.iter().sum::<f32>() + 20.0, 200.0);
        assert_eq!(solve_slots(10.0, 0.0, &[Size::Fill(0.0)]), vec![0.0]);
        assert_eq!(solve_slots(10.0, 0.0, &[Size::Fixed(30.0)]), vec![30.0]);
    }

    #[test]
    fn column_stacks_with_spacing() {
        let mut l = Layout::column(vec2(10.0, 10.0), 100.0, 4.0);
        let a = l.allocate(0.0, 20.0, true);
        let b = l.allocate(30.0, 10.0, false);
        assert_eq!(a, Rect::new(10.0, 10.0, 100.0, 20.0));
        assert_eq!(b, Rect::new(10.0, 34.0, 30.0, 10.0));
        assert_eq!(l.used(), vec2(100.0, 34.0));
    }

    #[test]
    fn column_alignment_moves_natural_width_items() {
        let mut l = Layout::column(Vec2::ZERO, 100.0, 0.0);
        l.set_align(Align::End);
        assert_eq!(l.allocate(30.0, 10.0, false).x, 70.0);
        l.set_align(Align::Center);
        assert_eq!(l.allocate(30.0, 10.0, false).x, 35.0);
    }

    #[test]
    fn slots_wrap_into_a_grid() {
        let mut l = Layout::slots(Vec2::ZERO, 110.0, 10.0, &[Size::Fill(1.0), Size::Fill(1.0)]);
        let cells: Vec<Rect> = (0..4)
            .map(|i| l.allocate(0.0, 10.0 + i as f32, true))
            .collect();
        assert_eq!(cells[0], Rect::new(0.0, 0.0, 50.0, 10.0));
        assert_eq!(cells[1], Rect::new(60.0, 0.0, 50.0, 11.0));
        assert_eq!(
            cells[2].y, 21.0,
            "the second line starts under the taller cell"
        );
        assert_eq!(cells[3].x, 60.0);
        assert_eq!(l.used().y, 21.0 + 13.0);
    }

    #[test]
    fn horizontal_places_natural_widths() {
        let mut l = Layout::horizontal(Vec2::ZERO, 200.0, 5.0);
        let a = l.allocate(40.0, 10.0, false);
        let b = l.allocate(60.0, 20.0, false);
        assert_eq!((a.x, b.x), (0.0, 45.0));
        l.finish_line();
        assert_eq!(l.cursor().y, 25.0);
    }
}
