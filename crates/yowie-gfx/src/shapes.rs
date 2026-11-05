//! Filled and outlined shapes, drawn through the sprite batch with the
//! white texture so they sort and clip like sprites.

use std::f32::consts::TAU;

use yowie_core::{Color, Rect, Vec2, vec2};

use crate::Graphics;
use crate::pipelines::Vertex2D;

/// The longest a miter join may reach, as a multiple of half the thickness.
const MITER_LIMIT: f32 = 4.0;

impl Graphics {
    fn shape_vertex(&self, p: Vec2, color: [f32; 4]) -> Vertex2D {
        Vertex2D {
            position: p.to_array(),
            uv: [0.5, 0.5],
            color,
            mode: 0,
        }
    }

    fn push_shape(&mut self, points: &[Vec2], indices: &[u32], color: Color) {
        let c = self.vertex_color(color);
        let v: Vec<Vertex2D> = points.iter().map(|p| self.shape_vertex(*p, c)).collect();
        let white = self.white.clone();
        self.push_triangles(&white, &v, indices);
    }

    /// Fills a rectangle.
    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let pts = [
            rect.min(),
            vec2(rect.x + rect.w, rect.y),
            rect.max(),
            vec2(rect.x, rect.y + rect.h),
        ];
        self.push_shape(&pts, &[0, 1, 2, 0, 2, 3], color);
    }

    /// Outlines a rectangle with a border `thickness` units wide lying
    /// inside it.
    pub fn stroke_rect(&mut self, rect: Rect, thickness: f32, color: Color) {
        let t = thickness.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
        if t <= 0.0 {
            return;
        }
        let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
        self.fill_rect(Rect::new(x, y, w, t), color);
        self.fill_rect(Rect::new(x, y + h - t, w, t), color);
        self.fill_rect(Rect::new(x, y + t, t, h - 2.0 * t), color);
        self.fill_rect(Rect::new(x + w - t, y + t, t, h - 2.0 * t), color);
    }

    /// Draws a line from `a` to `b`, `thickness` units wide, with square
    /// ends at the endpoints.
    pub fn line(&mut self, a: Vec2, b: Vec2, thickness: f32, color: Color) {
        let d = b - a;
        let Some(dir) = d.try_normalize() else {
            return;
        };
        let n = dir.perp() * (thickness * 0.5);
        self.push_shape(&[a + n, b + n, b - n, a - n], &[0, 1, 2, 0, 2, 3], color);
    }

    /// Fills a triangle.
    pub fn fill_triangle(&mut self, a: Vec2, b: Vec2, c: Vec2, color: Color) {
        self.push_shape(&[a, b, c], &[0, 1, 2], color);
    }

    /// Fills a convex polygon given its corners in order. Concave polygons
    /// fill incorrectly.
    pub fn fill_polygon(&mut self, points: &[Vec2], color: Color) {
        if points.len() < 3 {
            return;
        }
        let indices: Vec<u32> = (1..points.len() as u32 - 1)
            .flat_map(|i| [0, i, i + 1])
            .collect();
        self.push_shape(points, &indices, color);
    }

    /// Outlines a closed polygon, `thickness` units wide, centred on its
    /// edges with mitred corners.
    pub fn stroke_polygon(&mut self, points: &[Vec2], thickness: f32, color: Color) {
        self.stroke_points(points, true, thickness, color);
    }

    /// Draws connected line segments through `points`, `thickness` units
    /// wide, with mitred joins and square ends.
    pub fn stroke_polyline(&mut self, points: &[Vec2], thickness: f32, color: Color) {
        self.stroke_points(points, false, thickness, color);
    }

    fn circle_segments(&self, radius: f32) -> usize {
        let zoom = self.cursor.camera.map_or(1.0, |c| c.zoom.max(0.0));
        let r = (radius * self.pixels_per_unit() * zoom).max(0.5);
        // Keep the chord within a quarter pixel of the true circle.
        let step = 2.0 * (1.0 - 0.25 / r).clamp(-1.0, 1.0).acos();
        ((TAU / step.max(1e-3)).ceil() as usize).clamp(8, 256)
    }

    fn circle_points(&self, center: Vec2, radius: f32) -> Vec<Vec2> {
        let n = self.circle_segments(radius);
        (0..n)
            .map(|i| center + Vec2::from_angle(i as f32 / n as f32 * TAU) * radius)
            .collect()
    }

    /// Fills a circle.
    pub fn fill_circle(&mut self, center: Vec2, radius: f32, color: Color) {
        if radius > 0.0 {
            let pts = self.circle_points(center, radius);
            self.fill_polygon(&pts, color);
        }
    }

    /// Outlines a circle with a ring `thickness` units wide centred on the
    /// radius.
    pub fn stroke_circle(&mut self, center: Vec2, radius: f32, thickness: f32, color: Color) {
        if radius > 0.0 {
            let pts = self.circle_points(center, radius + thickness * 0.5);
            self.stroke_ring(&pts, center, radius, thickness, color);
        }
    }

    fn stroke_ring(
        &mut self,
        outer: &[Vec2],
        center: Vec2,
        radius: f32,
        thickness: f32,
        color: Color,
    ) {
        let inner_r = (radius - thickness * 0.5).max(0.0);
        let outer_r = radius + thickness * 0.5;
        let mut pts = Vec::with_capacity(outer.len() * 2);
        for p in outer {
            let d = (*p - center) / outer_r;
            pts.push(center + d * outer_r);
            pts.push(center + d * inner_r);
        }
        let n = outer.len() as u32;
        let indices: Vec<u32> = (0..n)
            .flat_map(|i| {
                let j = (i + 1) % n;
                [2 * i, 2 * j, 2 * j + 1, 2 * i, 2 * j + 1, 2 * i + 1]
            })
            .collect();
        self.push_shape(&pts, &indices, color);
    }

    fn stroke_points(&mut self, points: &[Vec2], closed: bool, thickness: f32, color: Color) {
        let n = points.len();
        if n < 2 || thickness <= 0.0 {
            return;
        }
        let half = thickness * 0.5;
        let edge_normal = |i: usize| -> Vec2 {
            let a = points[i % n];
            let b = points[(i + 1) % n];
            (b - a).try_normalize().map_or(Vec2::ZERO, |d| d.perp())
        };
        let mut pts = Vec::with_capacity(n * 2);
        for (i, &point) in points.iter().enumerate() {
            let has_prev = closed || i > 0;
            let has_next = closed || i + 1 < n;
            let prev = if has_prev {
                edge_normal((i + n - 1) % n)
            } else {
                Vec2::ZERO
            };
            let next = if has_next { edge_normal(i) } else { Vec2::ZERO };
            let offset = match (has_prev, has_next) {
                (true, true) => {
                    let m = (prev + next).try_normalize().unwrap_or(next);
                    let cos = m.dot(next).max(1.0 / MITER_LIMIT);
                    m * (half / cos)
                }
                (true, false) => prev * half,
                _ => next * half,
            };
            // Open ends extend by half the thickness for square caps. The
            // perpendicular of an edge normal points back along the edge.
            let mut p = point;
            if !closed && i == 0 {
                p += next.perp() * half;
            }
            if !closed && i + 1 == n {
                p -= prev.perp() * half;
            }
            pts.push(p + offset);
            pts.push(p - offset);
        }
        let segments = if closed { n } else { n - 1 } as u32;
        let count = n as u32;
        let indices: Vec<u32> = (0..segments)
            .flat_map(|i| {
                let j = (i + 1) % count;
                [2 * i, 2 * j, 2 * j + 1, 2 * i, 2 * j + 1, 2 * i + 1]
            })
            .collect();
        self.push_shape(&pts, &indices, color);
    }
}
