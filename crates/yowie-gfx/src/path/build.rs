//! Vector paths and the builder that makes them.

use lyon::geom::{Angle, Arc, ArcFlags, SvgArc};
use lyon::math::{Box2D, Point, point, vector};
use lyon::path::Winding;
use lyon::path::builder::BorderRadii;
use yowie_core::{Rect, Vec2, vec2};

/// A vector path: one or more subpaths of lines, curves and arcs. Build
/// one with [`Path::builder`] or the shape constructors, then fill it with
/// [`Graphics::fill_path`](crate::Graphics::fill_path) or outline it with
/// [`Graphics::stroke_path`](crate::Graphics::stroke_path). Coordinates are
/// drawing units. Cloning copies the path.
#[derive(Clone, Debug, Default)]
pub struct Path {
    pub(crate) inner: lyon::path::Path,
}

fn pt(v: Vec2) -> Point {
    point(v.x, v.y)
}

fn box2d(rect: Rect) -> Box2D {
    Box2D::new(pt(rect.min()), pt(rect.max()))
}

impl Path {
    /// Starts a path.
    pub fn builder() -> PathBuilder {
        PathBuilder::new()
    }

    /// A closed rectangle.
    pub fn rect(rect: Rect) -> Path {
        let mut b = PathBuilder::new();
        b.add_rect(rect);
        b.build()
    }

    /// A closed rectangle with corners rounded to `radius`, which is
    /// limited to half the shorter side.
    pub fn rounded_rect(rect: Rect, radius: f32) -> Path {
        let mut b = PathBuilder::new();
        b.add_rounded_rect(rect, radius);
        b.build()
    }

    /// A closed circle.
    pub fn circle(center: Vec2, radius: f32) -> Path {
        let mut b = PathBuilder::new();
        b.add_circle(center, radius);
        b.build()
    }

    /// A closed ellipse with the given radii along X and Y.
    pub fn ellipse(center: Vec2, radii: Vec2) -> Path {
        let mut b = PathBuilder::new();
        b.add_ellipse(center, radii);
        b.build()
    }

    /// Whether the path has no segments.
    pub fn is_empty(&self) -> bool {
        self.inner.iter().next().is_none()
    }

    /// The rectangle enclosing every endpoint and control point of the
    /// path, or `None` when it is empty. Curves stay inside it.
    pub fn bounds(&self) -> Option<Rect> {
        let mut points = Vec::new();
        for event in self.inner.iter() {
            use lyon::path::PathEvent as E;
            match event {
                E::Begin { at } => points.push(at),
                E::Line { to, .. } => points.push(to),
                E::Quadratic { ctrl, to, .. } => points.extend([ctrl, to]),
                E::Cubic {
                    ctrl1, ctrl2, to, ..
                } => points.extend([ctrl1, ctrl2, to]),
                E::End { .. } => {}
            }
        }
        Rect::bounding(points.into_iter().map(|p| vec2(p.x, p.y)))
    }
}

/// Builds a [`Path`] from move, line, curve, arc and close commands.
///
/// Drawing commands start a subpath at the current point when none is
/// open, so a path that begins with `line_to` starts at the origin or at
/// the end of the previous subpath.
pub struct PathBuilder {
    inner: lyon::path::path::Builder,
    current: Vec2,
    start: Vec2,
    open: bool,
}

impl std::fmt::Debug for PathBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathBuilder")
            .field("current", &self.current)
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

impl Default for PathBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PathBuilder {
    /// An empty builder with the current point at the origin.
    pub fn new() -> Self {
        Self {
            inner: lyon::path::Path::builder(),
            current: Vec2::ZERO,
            start: Vec2::ZERO,
            open: false,
        }
    }

    /// The current point: where the next segment starts.
    pub fn current_point(&self) -> Vec2 {
        self.current
    }

    fn ensure_open(&mut self) {
        if !self.open {
            self.inner.begin(pt(self.current));
            self.start = self.current;
            self.open = true;
        }
    }

    /// Ends the open subpath, if any, without closing it.
    fn finish_open(&mut self) {
        if self.open {
            self.inner.end(false);
            self.open = false;
        }
    }

    /// Starts a new subpath at `to`.
    pub fn move_to(&mut self, to: Vec2) -> &mut Self {
        self.finish_open();
        self.current = to;
        self.ensure_open();
        self
    }

    /// Adds a straight segment to `to`.
    pub fn line_to(&mut self, to: Vec2) -> &mut Self {
        self.ensure_open();
        self.inner.line_to(pt(to));
        self.current = to;
        self
    }

    /// Adds a quadratic Bezier curve with control point `ctrl`, ending at
    /// `to`.
    pub fn quad_to(&mut self, ctrl: Vec2, to: Vec2) -> &mut Self {
        self.ensure_open();
        self.inner.quadratic_bezier_to(pt(ctrl), pt(to));
        self.current = to;
        self
    }

    /// Adds a cubic Bezier curve with control points `ctrl1` and `ctrl2`,
    /// ending at `to`.
    pub fn cubic_to(&mut self, ctrl1: Vec2, ctrl2: Vec2, to: Vec2) -> &mut Self {
        self.ensure_open();
        self.inner.cubic_bezier_to(pt(ctrl1), pt(ctrl2), pt(to));
        self.current = to;
        self
    }

    /// Adds an elliptical arc that starts at the current point on the
    /// ellipse centred at `center` with the given radii. The arc turns by
    /// `sweep` radians (positive is clockwise on screen). `x_rotation`
    /// turns the ellipse's axes. The current point must lie on the
    /// ellipse; if it does not, the arc starts at its nearest angle.
    pub fn arc(&mut self, center: Vec2, radii: Vec2, sweep: f32, x_rotation: f32) -> &mut Self {
        self.ensure_open();
        let (s, c) = x_rotation.sin_cos();
        let d = self.current - center;
        let local = vec2(d.x * c + d.y * s, -d.x * s + d.y * c) / radii.max(Vec2::splat(1e-6));
        let arc = Arc {
            center: pt(center),
            radii: vector(radii.x, radii.y),
            start_angle: Angle::radians(local.y.atan2(local.x)),
            sweep_angle: Angle::radians(sweep),
            x_rotation: Angle::radians(x_rotation),
        };
        let inner = &mut self.inner;
        arc.for_each_cubic_bezier(&mut |seg| {
            inner.cubic_bezier_to(seg.ctrl1, seg.ctrl2, seg.to);
        });
        let end = arc.sample(1.0);
        self.current = vec2(end.x, end.y);
        self
    }

    /// Adds an SVG-style arc from the current point to `to`: an ellipse
    /// with `radii` turned by `x_rotation` radians, taking the larger of
    /// the two possible arcs when `large_arc` is set and the clockwise
    /// direction when `sweep` is set. Radii too small to reach `to` are
    /// scaled up.
    pub fn arc_to(
        &mut self,
        radii: Vec2,
        x_rotation: f32,
        large_arc: bool,
        sweep: bool,
        to: Vec2,
    ) -> &mut Self {
        self.ensure_open();
        let svg = SvgArc {
            from: pt(self.current),
            to: pt(to),
            radii: vector(radii.x, radii.y),
            x_rotation: Angle::radians(x_rotation),
            flags: ArcFlags { large_arc, sweep },
        };
        if svg.is_straight_line() {
            self.inner.line_to(pt(to));
        } else {
            let inner = &mut self.inner;
            svg.for_each_cubic_bezier(&mut |seg| {
                inner.cubic_bezier_to(seg.ctrl1, seg.ctrl2, seg.to);
            });
        }
        self.current = to;
        self
    }

    /// Closes the open subpath with a straight segment back to its start.
    /// The current point returns to the start.
    pub fn close(&mut self) -> &mut Self {
        if self.open {
            self.inner.end(true);
            self.open = false;
            self.current = self.start;
        }
        self
    }

    /// Adds a closed rectangle as its own subpath.
    pub fn add_rect(&mut self, rect: Rect) -> &mut Self {
        self.finish_open();
        self.inner.add_rectangle(&box2d(rect), Winding::Positive);
        self
    }

    /// Adds a closed rounded rectangle as its own subpath.
    pub fn add_rounded_rect(&mut self, rect: Rect, radius: f32) -> &mut Self {
        self.finish_open();
        let r = radius.clamp(0.0, rect.w.min(rect.h) * 0.5);
        self.inner
            .add_rounded_rectangle(&box2d(rect), &BorderRadii::new(r), Winding::Positive);
        self
    }

    /// Adds a closed circle as its own subpath.
    pub fn add_circle(&mut self, center: Vec2, radius: f32) -> &mut Self {
        self.finish_open();
        self.inner
            .add_circle(pt(center), radius.abs(), Winding::Positive);
        self
    }

    /// Adds a closed ellipse as its own subpath.
    pub fn add_ellipse(&mut self, center: Vec2, radii: Vec2) -> &mut Self {
        self.finish_open();
        self.inner.add_ellipse(
            pt(center),
            vector(radii.x.abs(), radii.y.abs()),
            Angle::zero(),
            Winding::Positive,
        );
        self
    }

    /// Finishes the path. An open subpath stays open (strokes get caps).
    pub fn build(mut self) -> Path {
        self.finish_open();
        Path {
            inner: self.inner.build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_have_bounds() {
        let cases = [
            (
                Path::rect(Rect::new(1.0, 2.0, 3.0, 4.0)),
                (1.0, 2.0, 4.0, 6.0),
            ),
            (Path::circle(vec2(10.0, 10.0), 5.0), (5.0, 5.0, 15.0, 15.0)),
        ];
        for (path, (x0, y0, x1, y1)) in cases {
            let b = path.bounds().unwrap_or(Rect::ZERO);
            assert!(
                (b.x - x0).abs() < 0.1
                    && (b.y - y0).abs() < 0.1
                    && (b.x + b.w - x1).abs() < 0.1
                    && (b.y + b.h - y1).abs() < 0.1,
                "{b:?}"
            );
        }
        assert!(Path::default().is_empty());
    }

    #[test]
    fn arc_ends_where_the_sweep_says() {
        let mut b = Path::builder();
        b.move_to(vec2(10.0, 0.0));
        b.arc(
            Vec2::ZERO,
            Vec2::splat(10.0),
            std::f32::consts::FRAC_PI_2,
            0.0,
        );
        let end = b.current_point();
        assert!((end - vec2(0.0, 10.0)).length() < 1e-3, "{end:?}");
    }
}
