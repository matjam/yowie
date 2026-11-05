//! Mapping the game's 2D view into the window: fixed virtual resolutions,
//! letterboxing and integer scaling.

use yowie_core::{Rect, UVec2, Vec2, vec2};

/// How a fixed view fills a window of a different shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScaleMode {
    /// The largest scale that keeps the aspect ratio, centred, with bars
    /// on two sides.
    #[default]
    Letterbox,
    /// The largest whole-number scale that fits, centred, with bars around
    /// it; keeps pixel art crisp.
    Integer,
    /// Fills the whole window and distorts the aspect ratio.
    Stretch,
}

/// The game's 2D coordinate space: either the window itself in logical
/// points, or a fixed size scaled into the window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewSettings {
    /// A fixed view size in view units, or `None` for the window's size in
    /// logical points.
    pub fixed: Option<UVec2>,
    /// How a fixed view fills the window; ignored without one.
    pub scale: ScaleMode,
}

/// Where the view lands in an output of a given pixel size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewMapping {
    /// The view's size in view units.
    pub view_size: Vec2,
    /// The output's size in pixels.
    pub output_size: UVec2,
    /// The rectangle of the output the view covers, in pixels. Outside it
    /// are the letterbox bars.
    pub viewport: Rect,
}

impl ViewMapping {
    /// Places `settings`' view in an output of `output` pixels whose
    /// display has `scale_factor` pixels per logical point.
    pub fn new(settings: ViewSettings, output: UVec2, scale_factor: f32) -> Self {
        let out = output.max(UVec2::ONE).as_vec2();
        let full = Rect::new(0.0, 0.0, out.x, out.y);
        let Some(fixed) = settings.fixed else {
            let sf = if scale_factor > 0.0 {
                scale_factor
            } else {
                1.0
            };
            return Self {
                view_size: out / sf,
                output_size: output,
                viewport: full,
            };
        };
        let view = fixed.max(UVec2::ONE).as_vec2();
        let size = match settings.scale {
            ScaleMode::Stretch => out,
            ScaleMode::Letterbox => view * (out.x / view.x).min(out.y / view.y),
            ScaleMode::Integer => {
                let s = (out.x / view.x).min(out.y / view.y).floor().max(1.0);
                view * s
            }
        };
        let origin = ((out - size) * 0.5).round();
        Self {
            view_size: view,
            output_size: output,
            viewport: Rect::new(origin.x, origin.y, size.x, size.y),
        }
    }

    /// A mapping that fills an output exactly, with view units equal to
    /// pixels unless `view_size` says otherwise. Render textures use it.
    pub fn filling(output: UVec2, view_size: Option<Vec2>) -> Self {
        let out = output.max(UVec2::ONE).as_vec2();
        Self {
            view_size: view_size.unwrap_or(out),
            output_size: output,
            viewport: Rect::new(0.0, 0.0, out.x, out.y),
        }
    }

    /// Pixels per view unit on each axis.
    pub fn pixels_per_unit(&self) -> Vec2 {
        self.viewport.size() / self.view_size
    }

    /// Whether the viewport leaves bars uncovered.
    pub fn letterboxed(&self) -> bool {
        let out = self.output_size.as_vec2();
        self.viewport.x > 0.0
            || self.viewport.y > 0.0
            || self.viewport.w < out.x
            || self.viewport.h < out.y
    }

    /// Converts a position in output pixels to view units. Points over the
    /// bars map outside the view rectangle.
    pub fn pixels_to_view(&self, p: Vec2) -> Vec2 {
        (p - self.viewport.min()) / self.pixels_per_unit()
    }

    /// Converts a position in view units to output pixels.
    pub fn view_to_pixels(&self, p: Vec2) -> Vec2 {
        p * self.pixels_per_unit() + self.viewport.min()
    }

    /// Converts a rectangle in view units to output pixels.
    pub fn rect_to_pixels(&self, r: Rect) -> Rect {
        let min = self.view_to_pixels(r.min());
        let max = self.view_to_pixels(r.max());
        Rect::from_min_max(min, max)
    }
}

impl Default for ViewMapping {
    fn default() -> Self {
        Self::filling(UVec2::ONE, Some(vec2(1.0, 1.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::uvec2;

    #[test]
    fn mappings() {
        let fixed = |scale| ViewSettings {
            fixed: Some(uvec2(320, 180)),
            scale,
        };
        let cases = [
            // (settings, output, scale factor, view size, viewport)
            (
                ViewSettings::default(),
                uvec2(1600, 1200),
                2.0,
                vec2(800.0, 600.0),
                Rect::new(0.0, 0.0, 1600.0, 1200.0),
            ),
            (
                fixed(ScaleMode::Letterbox),
                uvec2(1280, 1000),
                1.0,
                vec2(320.0, 180.0),
                Rect::new(0.0, 140.0, 1280.0, 720.0),
            ),
            (
                fixed(ScaleMode::Integer),
                uvec2(1000, 600),
                1.0,
                vec2(320.0, 180.0),
                Rect::new(20.0, 30.0, 960.0, 540.0),
            ),
            (
                fixed(ScaleMode::Stretch),
                uvec2(500, 500),
                1.0,
                vec2(320.0, 180.0),
                Rect::new(0.0, 0.0, 500.0, 500.0),
            ),
            (
                fixed(ScaleMode::Integer),
                uvec2(100, 100),
                1.0,
                vec2(320.0, 180.0),
                Rect::new(-110.0, -40.0, 320.0, 180.0),
            ),
        ];
        for (settings, out, sf, view, viewport) in cases {
            let m = ViewMapping::new(settings, out, sf);
            assert_eq!(m.view_size, view, "{settings:?}");
            assert_eq!(m.viewport, viewport, "{settings:?}");
            let p = vec2(10.0, 20.0);
            assert!((m.pixels_to_view(m.view_to_pixels(p)) - p).length() < 1e-4);
        }
    }

    #[test]
    fn bars_map_outside_the_view() {
        let m = ViewMapping::new(
            ViewSettings {
                fixed: Some(uvec2(320, 180)),
                scale: ScaleMode::Letterbox,
            },
            uvec2(1280, 1000),
            1.0,
        );
        assert!(m.letterboxed());
        assert!(m.pixels_to_view(vec2(640.0, 10.0)).y < 0.0);
        assert_eq!(m.pixels_to_view(vec2(640.0, 500.0)), vec2(160.0, 90.0));
    }
}
