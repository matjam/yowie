//! Labels in the world: text anchored to a 3D point, projected through the
//! scene's camera and drawn in the 2D pass over the scene, hidden when the
//! point is behind the camera and faded with distance when asked.

use yowie_core::{Color, Rect, Vec2, Vec3};

use super::camera::Camera3D;
use super::submit::Scene;
use crate::Graphics;
use crate::text::{Font, TextStyle};

/// How a world label is set and placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelStyle {
    /// The text's size and colour.
    pub text: TextStyle,
    /// A plate drawn behind the text, sRGB; `None` for none.
    pub background: Option<Color>,
    /// The space between the text and the plate's edge, in view units.
    pub padding: f32,
    /// Moves the label from its projected point, in view units. The label
    /// is centred above the point before this moves it.
    pub offset: Vec2,
    /// Fades the label from fully shown at the first distance (world
    /// units from the camera) to hidden at the second.
    pub fade: Option<(f32, f32)>,
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            text: TextStyle::new(16.0),
            background: Some(Color::rgba(0.0, 0.0, 0.0, 0.55)),
            padding: 4.0,
            offset: Vec2::ZERO,
            fade: None,
        }
    }
}

/// One recorded label.
#[derive(Clone, Debug)]
pub(crate) struct LabelDraw {
    font: Font,
    text: String,
    position: Vec3,
    style: LabelStyle,
}

impl Scene<'_> {
    /// Anchors `text` to the world point `position`. It is drawn over the
    /// scene in the 2D pass, centred above where the point appears, at a
    /// constant size; labels behind the camera or faded out are skipped.
    pub fn label(&mut self, font: &Font, position: Vec3, text: &str, style: &LabelStyle) {
        self.rec.large.labels.push(LabelDraw {
            font: font.clone(),
            text: text.to_owned(),
            position,
            style: *style,
        });
    }
}

/// How visible a label `distance` units away is under `fade`.
fn fade_alpha(fade: Option<(f32, f32)>, distance: f32) -> f32 {
    match fade {
        Some((start, end)) if end > start => {
            1.0 - ((distance - start) / (end - start)).clamp(0.0, 1.0)
        }
        Some((start, _)) if distance > start => 0.0,
        _ => 1.0,
    }
}

/// Where a label anchored at `world` lands in a view of `view_size`
/// units, as the point under its bottom edge's middle, and how far it is
/// from the camera; `None` behind the camera.
pub(crate) fn project(camera: &Camera3D, view_size: Vec2, world: Vec3) -> Option<(Vec2, f32)> {
    let screen = camera.world_to_screen(world, view_size)?;
    let ahead = (world - camera.position).dot(camera.forward());
    if ahead <= camera.near {
        return None;
    }
    Some((screen, (world - camera.position).length()))
}

/// Draws the labels a scene recorded, farthest first so nearer ones are
/// on top.
pub(crate) fn draw_labels(
    gfx: &mut Graphics,
    camera: &Camera3D,
    view_size: Vec2,
    labels: Vec<LabelDraw>,
) {
    let mut placed: Vec<(f32, Vec2, LabelDraw)> = labels
        .into_iter()
        .filter_map(|l| {
            let (at, d) = project(camera, view_size, l.position)?;
            Some((d, at, l))
        })
        .collect();
    placed.sort_by(|a, b| b.0.total_cmp(&a.0));
    let viewport = camera.viewport_rect(view_size);
    for (d, at, l) in placed {
        let alpha = fade_alpha(l.style.fade, d);
        if alpha <= 0.0 {
            continue;
        }
        let size = gfx.measure_text(&l.font, &l.text, &l.style.text);
        let pad = l.style.padding;
        let top_left = at + l.style.offset - Vec2::new(size.x * 0.5, size.y + pad);
        let plate = Rect::new(
            top_left.x - pad,
            top_left.y - pad,
            size.x + pad * 2.0,
            size.y + pad * 2.0,
        );
        if plate.intersection(&viewport).is_none() {
            continue;
        }
        if let Some(bg) = l.style.background {
            gfx.fill_rect(
                plate,
                Color {
                    a: bg.a * alpha,
                    ..bg
                },
            );
        }
        let mut style = l.style.text;
        style.color.a *= alpha;
        gfx.draw_text(&l.font, &l.text, top_left, &style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_fade_and_hide_behind_the_camera() {
        let cases = [
            (None, 100.0, 1.0),
            (Some((10.0, 20.0)), 5.0, 1.0),
            (Some((10.0, 20.0)), 15.0, 0.5),
            (Some((10.0, 20.0)), 25.0, 0.0),
        ];
        for (fade, d, want) in cases {
            assert!((fade_alpha(fade, d) - want).abs() < 1e-6, "{fade:?} {d}");
        }
        let cam = Camera3D::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 1.0);
        let view = Vec2::new(200.0, 100.0);
        let (at, d) = project(&cam, view, Vec3::ZERO).expect("in front");
        assert!((at - Vec2::new(100.0, 50.0)).length() < 1e-3);
        assert!((d - 5.0).abs() < 1e-5);
        assert!(project(&cam, view, Vec3::new(0.0, 0.0, 8.0)).is_none());
    }
}
