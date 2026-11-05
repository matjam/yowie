//! Drawing helpers shared by widgets: depth passes, culling, text, boxes
//! with optional skins, focus rings and small shapes.

use yowie_core::{Color, Rect, Vec2, vec2};
use yowie_gfx::{NineSlice, Texture};

use crate::frame::{Frame, Pass, depth};
use crate::layout::Align;
use crate::painter::Face;
use crate::response::Response;

/// What a background is for, which picks its colours and skin slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoxKind {
    Panel,
    Title,
    Button,
    Field,
    Track,
    Fill,
}

/// How pressed a button-like background looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BState {
    Normal,
    Hover,
    Active,
}

impl BState {
    pub(crate) fn of(r: &Response) -> BState {
        if r.held {
            BState::Active
        } else if r.hovered {
            BState::Hover
        } else {
            BState::Normal
        }
    }
}

/// The corners of a regular polygon approximating a circle.
pub(crate) fn ngon(center: Vec2, radius: f32, n: usize) -> Vec<Vec2> {
    (0..n)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / n as f32;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// Which way an arrow points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrow {
    Up,
    Down,
    Right,
}

impl Frame<'_> {
    pub(crate) fn dim(&self, c: Color) -> Color {
        if self.disabled > 0 {
            c.with_alpha(c.a * 0.45)
        } else {
            c
        }
    }

    pub(crate) fn culled(&self, r: Rect) -> bool {
        let c = self.clip_rect();
        r.x >= c.x + c.w || r.y >= c.y + c.h || r.x + r.w <= c.x || r.y + r.h <= c.y
    }

    /// Fills `r` with `color`.
    pub fn fill_rect(&mut self, r: Rect, color: Color) {
        if self.culled(r) {
            return;
        }
        self.pass(Pass::Fill);
        let c = self.dim(color);
        self.p.fill(r, c);
    }

    /// Outlines `r` inside its bounds.
    pub fn stroke_rect(&mut self, r: Rect, thickness: f32, color: Color) {
        if self.culled(r) {
            return;
        }
        self.pass(Pass::Fill);
        let c = self.dim(color);
        self.p.stroke(r, thickness, c);
    }

    pub(crate) fn over_fill(&mut self, r: Rect, color: Color) {
        if self.culled(r) {
            return;
        }
        self.pass(Pass::Over);
        let c = self.dim(color);
        self.p.fill(r, c);
    }

    pub(crate) fn polygon(&mut self, pts: &[Vec2], color: Color, over: bool) {
        self.pass(if over { Pass::Over } else { Pass::Fill });
        let c = self.dim(color);
        self.p.polygon(pts, c);
    }

    pub(crate) fn line(&mut self, a: Vec2, b: Vec2, thickness: f32, color: Color) {
        self.pass(Pass::Over);
        let c = self.dim(color);
        self.p.line(a, b, thickness, c);
    }

    /// Draws a texture (or part of it) stretched over `dest`.
    pub fn image_rect(&mut self, texture: &Texture, dest: Rect, source: Option<Rect>, tint: Color) {
        if self.culled(dest) {
            return;
        }
        self.pass(Pass::Image);
        let t = self.dim(tint);
        self.p.image(texture, dest, source, t);
    }

    /// The size `text` takes in the interface font.
    pub fn measure(&mut self, text: &str) -> Vec2 {
        let size = self.ui.theme.text_size;
        self.p.measure(text, Face::Regular, size, None)
    }

    pub(crate) fn measure_wrapped(&mut self, text: &str, width: f32) -> Vec2 {
        let size = self.ui.theme.text_size;
        self.p.measure(text, Face::Regular, size, Some(width))
    }

    pub(crate) fn measure_face(&mut self, text: &str, face: Face) -> Vec2 {
        let theme = &self.ui.theme;
        let size = if face == Face::Mono {
            theme.mono_size
        } else {
            theme.text_size
        };
        self.p.measure(text, face, size, None)
    }

    /// Draws `text` with its top-left at `pos`.
    pub fn text_at(&mut self, pos: Vec2, text: &str, color: Color) {
        self.text_face(pos, text, color, Face::Regular, None);
    }

    pub(crate) fn text_face(
        &mut self,
        pos: Vec2,
        text: &str,
        color: Color,
        face: Face,
        wrap: Option<f32>,
    ) {
        if text.is_empty() {
            return;
        }
        let theme = &self.ui.theme;
        let size = if face == Face::Mono {
            theme.mono_size
        } else {
            theme.text_size
        };
        self.pass(Pass::Text);
        let c = self.dim(color);
        self.p.text(text, pos, face, size, c, wrap);
    }

    /// Draws `text` inside `r`, centred vertically and lined up
    /// horizontally by `align`.
    pub(crate) fn text_in(&mut self, r: Rect, text: &str, color: Color, align: Align) {
        if text.is_empty() || self.culled(r) {
            return;
        }
        let s = self.measure(text);
        let x = r.x + align.offset(r.w, s.x);
        let y = r.y + ((r.h - s.y) * 0.5).max(0.0);
        self.text_at(vec2(x, y), text, color);
    }

    fn slice_for(&self, kind: BoxKind, state: BState) -> Option<(NineSlice, Color)> {
        let skin = self.ui.theme.skin.as_ref()?;
        let gray = |v: f32| Color::rgb(v, v, v);
        let (slice, tint) = match kind {
            BoxKind::Panel => (skin.panel.clone(), Color::WHITE),
            BoxKind::Title => (
                skin.title.clone().or_else(|| skin.panel.clone()),
                Color::WHITE,
            ),
            BoxKind::Field => (skin.field.clone(), Color::WHITE),
            BoxKind::Track => (skin.track.clone(), Color::WHITE),
            BoxKind::Fill => (skin.fill.clone(), Color::WHITE),
            BoxKind::Button => match state {
                BState::Normal => (skin.button.clone(), gray(0.9)),
                BState::Hover => match &skin.button_hover {
                    Some(s) => (Some(s.clone()), Color::WHITE),
                    None => (skin.button.clone(), Color::WHITE),
                },
                BState::Active => match &skin.button_active {
                    Some(s) => (Some(s.clone()), Color::WHITE),
                    None => (skin.button.clone(), gray(0.7)),
                },
            },
        };
        slice.map(|s| (s, tint))
    }

    /// Draws a widget background: the skin's slice if the theme has one,
    /// otherwise flat colours.
    pub(crate) fn draw_box(&mut self, r: Rect, kind: BoxKind, state: BState) {
        if self.culled(r) {
            return;
        }
        if let Some((slice, tint)) = self.slice_for(kind, state) {
            self.pass(Pass::Image);
            let t = self.dim(tint);
            self.p.nine_slice(&slice, r, t);
            return;
        }
        let t = &self.ui.theme;
        let (fill, border) = match kind {
            BoxKind::Panel => (t.panel, t.border),
            BoxKind::Title => (t.button, t.border),
            BoxKind::Field => (t.field, t.border),
            BoxKind::Track => (t.track, t.border),
            BoxKind::Fill => (t.accent, t.accent),
            BoxKind::Button => match state {
                BState::Normal => (t.button, t.border),
                BState::Hover => (t.button_hover, t.border),
                BState::Active => (t.button_active, t.accent),
            },
        };
        let bw = t.border_width;
        self.fill_rect(r, fill);
        if bw > 0.0 && kind != BoxKind::Fill {
            self.stroke_rect(r, bw, border);
        }
    }

    /// Draws the keyboard focus ring around a focused widget when the last
    /// input came from the keyboard or a gamepad.
    pub(crate) fn focus_ring(&mut self, r: &Response) {
        if r.focused && self.ui.nav_visible {
            let w = self.ui.theme.focus_width;
            let c = self.ui.theme.accent;
            if !self.culled(r.rect) {
                self.pass(Pass::Over);
                let col = self.dim(c);
                self.p.stroke(r.rect.inflate(1.0), w, col);
            }
        }
    }

    pub(crate) fn arrow(&mut self, center: Vec2, size: f32, dir: Arrow, color: Color) {
        let h = size * 0.5;
        let pts = match dir {
            Arrow::Down => [
                center + vec2(-h, -h * 0.5),
                center + vec2(h, -h * 0.5),
                center + vec2(0.0, h * 0.6),
            ],
            Arrow::Up => [
                center + vec2(-h, h * 0.5),
                center + vec2(0.0, -h * 0.6),
                center + vec2(h, h * 0.5),
            ],
            Arrow::Right => [
                center + vec2(-h * 0.5, -h),
                center + vec2(h * 0.6, 0.0),
                center + vec2(-h * 0.5, h),
            ],
        };
        self.polygon(&pts, color, true);
    }

    /// Draws what is drawn last: the tooltip and the drag ghost.
    pub(crate) fn draw_deferred(&mut self) {
        self.clips.clear();
        self.p.set_clip(None);
        self.push_layer(crate::id::WidgetId::NONE, depth::TOP);
        if let Some((text, at)) = self.tooltip.take() {
            self.draw_label_box(&text, at + vec2(14.0, 18.0));
        }
        let ghost = self.ui.drag.as_ref().map(|d| d.label.clone());
        if let Some(label) = ghost {
            let at = self.mouse + vec2(12.0, 12.0);
            self.draw_label_box(&label, at);
        }
        self.layer_stack.pop();
    }

    /// A small bordered box with `text`, kept inside the view.
    pub(crate) fn draw_label_box(&mut self, text: &str, at: Vec2) {
        let pad = self.ui.theme.padding * 0.75;
        let s = self.measure(text);
        let size = s + vec2(pad * 2.0, pad);
        let x = at.x.min(self.view.w - size.x).max(0.0);
        let y = at.y.min(self.view.h - size.y).max(0.0);
        let r = Rect::new(x, y, size.x, size.y);
        let (panel, border, text_color) = (
            self.ui.theme.panel,
            self.ui.theme.accent,
            self.ui.theme.text,
        );
        self.fill_rect(r, panel.with_alpha(1.0));
        self.stroke_rect(r, 1.0, border);
        self.text_at(vec2(r.x + pad, r.y + pad * 0.5), text, text_color);
    }
}
