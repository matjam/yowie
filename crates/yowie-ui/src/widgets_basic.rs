//! Labels, buttons, toggles, sliders, progress bars, images and swatches.

use std::ops::RangeInclusive;

use yowie_core::{Color, Rect, Vec2, vec2};
use yowie_gfx::Texture;
use yowie_input::Key;

use crate::access::{AccessState, Role};
use crate::draw::{BState, BoxKind, ngon};
use crate::frame::{Frame, Sense};
use crate::id::{WidgetId, visible_label};
use crate::layout::Align;
use crate::response::Response;

impl Frame<'_> {
    /// Adds static text.
    pub fn label(&mut self, text: &str) -> Response {
        let c = self.ui.theme.text;
        self.label_colored(text, c)
    }

    /// Adds static text in the theme's secondary colour.
    pub fn label_dim(&mut self, text: &str) -> Response {
        let c = self.ui.theme.text_dim;
        self.label_colored(text, c)
    }

    /// Adds a section heading in the theme's title colour.
    pub fn heading(&mut self, text: &str) -> Response {
        let c = self.ui.theme.title;
        self.label_colored(text, c)
    }

    /// Adds static text in `color`.
    pub fn label_colored(&mut self, text: &str, color: Color) -> Response {
        let id = self.id(text);
        let shown = visible_label(text);
        let s = self.measure(shown);
        let rect = self.alloc(s.x, s.y + 6.0, false);
        self.text_at(vec2(rect.x, rect.y + 3.0), shown, color);
        self.node(id, Role::Label, shown, None, AccessState::default(), rect);
        Response::blank(id, rect)
    }

    /// Adds text wrapped to the container's width.
    pub fn paragraph(&mut self, text: &str) -> Response {
        let id = self.id(text);
        let w = self.layout().available_width();
        let s = self.measure_wrapped(text, w);
        let rect = self.alloc(w, s.y + 6.0, true);
        let c = self.ui.theme.text;
        self.text_face(
            vec2(rect.x, rect.y + 3.0),
            text,
            c,
            crate::painter::Face::Regular,
            Some(w),
        );
        self.node(id, Role::Label, text, None, AccessState::default(), rect);
        Response::blank(id, rect)
    }

    /// Adds a thin dividing line across the container.
    pub fn separator(&mut self) {
        let id = self.id("separator");
        let rect = self.alloc(0.0, 1.0, true);
        let c = self.ui.theme.border;
        self.fill_rect(rect, c);
        self.node(id, Role::Separator, "", None, AccessState::default(), rect);
    }

    /// Adds `amount` view units of empty space below the current line.
    pub fn space(&mut self, amount: f32) {
        self.layout().space(amount);
    }

    fn button_height(&mut self, text_h: f32) -> f32 {
        self.ui.theme.row_height.max(text_h + self.ui.theme.padding)
    }

    /// Adds a push button. `clicked` on the response says it was pressed.
    pub fn button(&mut self, label: &str) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let s = self.measure(shown);
        let pad = self.ui.theme.padding;
        let h = self.button_height(s.y);
        let rect = self.alloc(s.x + pad * 2.0, h, true);
        let r = self.interact(id, rect, Sense::CLICK);
        self.draw_box(rect, BoxKind::Button, BState::of(&r));
        let c = self.ui.theme.text;
        self.text_in(rect, shown, c, Align::Center);
        self.focus_ring(&r);
        self.node(id, Role::Button, shown, None, AccessState::default(), rect);
        r
    }

    /// Adds a check box bound to `value`. `changed` says it was toggled.
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let s = self.measure(shown);
        let h = self.ui.theme.row_height;
        let rect = self.alloc(s.x + h, h, true);
        let mut r = self.interact(id, rect, Sense::CLICK);
        if r.clicked {
            *value = !*value;
            r.changed = true;
        }
        let (accent, field, border, text) = {
            let t = &self.ui.theme;
            (t.accent, t.field, t.border, t.text)
        };
        let side = (h - 8.0).max(10.0);
        let b = Rect::new(rect.x + 2.0, rect.y + (h - side) * 0.5, side, side);
        self.fill_rect(
            b,
            if r.hovered {
                self.ui.theme.button_hover
            } else {
                field
            },
        );
        self.stroke_rect(b, 1.0, if *value { accent } else { border });
        if *value {
            let a = vec2(b.x + side * 0.22, b.y + side * 0.52);
            let m = vec2(b.x + side * 0.43, b.y + side * 0.74);
            let e = vec2(b.x + side * 0.80, b.y + side * 0.26);
            self.line(a, m, 2.0, accent);
            self.line(m, e, 2.0, accent);
        }
        let tr = Rect::new(b.x + side + 8.0, rect.y, rect.w - side - 10.0, h);
        self.text_in(tr, shown, text, Align::Start);
        self.focus_ring(&r);
        let state = AccessState {
            checked: Some(*value),
            ..AccessState::default()
        };
        self.node(id, Role::CheckBox, shown, None, state, rect);
        r
    }

    /// Adds a radio button that selects `option` in `value`. Radio buttons
    /// sharing one `value` form a group. `changed` says it was chosen.
    pub fn radio<T: PartialEq>(&mut self, label: &str, value: &mut T, option: T) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let s = self.measure(shown);
        let h = self.ui.theme.row_height;
        let rect = self.alloc(s.x + h, h, true);
        let mut r = self.interact(id, rect, Sense::CLICK);
        let mut on = *value == option;
        if r.clicked && !on {
            *value = option;
            r.changed = true;
            on = true;
        }
        let (accent, field, border, text, hover) = {
            let t = &self.ui.theme;
            (t.accent, t.field, t.border, t.text, t.button_hover)
        };
        let side = (h - 8.0).max(10.0);
        let c = vec2(rect.x + 2.0 + side * 0.5, rect.y + h * 0.5);
        self.polygon(
            &ngon(c, side * 0.5, 20),
            if r.hovered { hover } else { field },
            false,
        );
        let ring = ngon(c, side * 0.5, 20);
        let col = if on { accent } else { border };
        for i in 0..ring.len() {
            self.line(ring[i], ring[(i + 1) % ring.len()], 1.0, col);
        }
        if on {
            self.polygon(&ngon(c, side * 0.25, 14), accent, true);
        }
        let tr = Rect::new(rect.x + side + 10.0, rect.y, rect.w - side - 10.0, h);
        self.text_in(tr, shown, text, Align::Start);
        self.focus_ring(&r);
        let state = AccessState {
            checked: Some(on),
            ..AccessState::default()
        };
        self.node(id, Role::RadioButton, shown, None, state, rect);
        r
    }

    /// Adds a slider editing `value` within `range`.
    pub fn slider(&mut self, label: &str, value: &mut f32, range: RangeInclusive<f32>) -> Response {
        let (lo, hi) = (*range.start(), *range.end());
        *value = value.clamp(lo.min(hi), lo.max(hi));
        let frac = if hi > lo {
            (*value - lo) / (hi - lo)
        } else {
            0.0
        };
        let text = format!("{}: {:.2}", visible_label(label), value);
        let (mut r, new, steps) = self.slider_core(label, frac, &text, format!("{value:.2}"));
        let old = *value;
        if let Some(f) = new {
            *value = lo + f * (hi - lo);
        }
        if steps != 0 {
            *value = (*value + steps as f32 * (hi - lo) / 50.0).clamp(lo.min(hi), lo.max(hi));
        }
        r.changed = *value != old;
        r
    }

    /// Adds a slider editing a whole number within `range`.
    pub fn slider_int(
        &mut self,
        label: &str,
        value: &mut i32,
        range: RangeInclusive<i32>,
    ) -> Response {
        let (lo, hi) = (*range.start(), *range.end());
        *value = (*value).clamp(lo.min(hi), lo.max(hi));
        let span = (hi - lo).max(1) as f32;
        let frac = (*value - lo) as f32 / span;
        let text = format!("{}: {}", visible_label(label), value);
        let (mut r, new, steps) = self.slider_core(label, frac, &text, value.to_string());
        let old = *value;
        if let Some(f) = new {
            *value = lo + (f * span).round() as i32;
        }
        *value = (*value + steps).clamp(lo.min(hi), lo.max(hi));
        r.changed = *value != old;
        r
    }

    /// Draws a slider and reports a new fraction (from the pointer or
    /// Home and End) and arrow-key steps.
    fn slider_core(
        &mut self,
        label: &str,
        frac: f32,
        text: &str,
        value_text: String,
    ) -> (Response, Option<f32>, i32) {
        let id = self.id(label);
        let h = self.ui.theme.row_height;
        let rect = self.alloc(160.0, h, true);
        let r = self.interact(id, rect, Sense::CLICK);
        self.set_wants(&r, true, false, false);
        let mut new = None;
        if r.held {
            new = Some(((self.mouse.x - rect.x - 3.0) / (rect.w - 6.0).max(1.0)).clamp(0.0, 1.0));
        }
        let mut steps = 0;
        if r.focused {
            let big = self.mods.contains(yowie_input::Modifiers::SHIFT);
            let k = if big { 10 } else { 1 };
            if self.key_repeat(Key::Left) {
                steps -= k;
            }
            if self.key_repeat(Key::Right) {
                steps += k;
            }
            if self.input.key_pressed(Key::Home) {
                new = Some(0.0);
            }
            if self.input.key_pressed(Key::End) {
                new = Some(1.0);
            }
        }
        self.draw_box(rect, BoxKind::Track, BState::Normal);
        let inner = rect.inflate(-2.0);
        let shown = new.unwrap_or(frac);
        let fill = Rect::new(inner.x, inner.y, inner.w * shown, inner.h);
        if fill.w > 0.5 {
            self.draw_box(fill, BoxKind::Fill, BState::Normal);
        }
        let c = self.ui.theme.text;
        self.text_in(rect, text, c, Align::Center);
        self.focus_ring(&r);
        self.node(
            id,
            Role::Slider,
            label,
            Some(value_text),
            AccessState::default(),
            rect,
        );
        (r, new, steps)
    }

    /// Adds a progress bar filled to `fraction` (0 to 1), with the
    /// percentage drawn over it.
    pub fn progress(&mut self, label: &str, fraction: f32) -> Response {
        let id = self.id(label);
        let f = fraction.clamp(0.0, 1.0);
        let h = self.ui.theme.row_height * 0.8;
        let rect = self.alloc(160.0, h, true);
        self.draw_box(rect, BoxKind::Track, BState::Normal);
        let inner = rect.inflate(-2.0);
        let fill = Rect::new(inner.x, inner.y, inner.w * f, inner.h);
        if fill.w > 0.5 {
            self.draw_box(fill, BoxKind::Fill, BState::Normal);
        }
        let shown = visible_label(label);
        let text = if shown.is_empty() {
            format!("{:.0}%", f * 100.0)
        } else {
            format!("{shown} {:.0}%", f * 100.0)
        };
        let c = self.ui.theme.text;
        self.text_in(rect, &text, c, Align::Center);
        self.node(
            id,
            Role::ProgressBar,
            shown,
            Some(format!("{:.0}%", f * 100.0)),
            AccessState::default(),
            rect,
        );
        Response::blank(id, rect)
    }

    /// Adds a picture `size` view units large.
    pub fn image(&mut self, texture: &Texture, size: Vec2) -> Response {
        self.image_region(texture, None, size)
    }

    /// Adds the `source` texel rectangle of a texture (all of it when
    /// `None`) at `size` view units.
    pub fn image_region(
        &mut self,
        texture: &Texture,
        source: Option<Rect>,
        size: Vec2,
    ) -> Response {
        let id = self.id("image");
        let rect = self.alloc(size.x, size.y, false);
        self.image_rect(texture, rect, source, Color::WHITE);
        self.node(id, Role::Image, "image", None, AccessState::default(), rect);
        Response::blank(id, rect)
    }

    /// Adds a colour swatch with a checkerboard behind translucent colours.
    /// `clicked` on the response says it was pressed, for opening a picker.
    pub fn color_swatch(&mut self, label: &str, color: Color) -> Response {
        let id = self.id(label);
        let shown = visible_label(label);
        let s = self.measure(shown);
        let h = self.ui.theme.row_height;
        let rect = self.alloc(h * 1.6 + 8.0 + s.x, h, true);
        let r = self.interact(id, rect, Sense::CLICK);
        let sw = Rect::new(rect.x, rect.y + 2.0, h * 1.6, h - 4.0);
        if color.a < 1.0 {
            let cell = (sw.h / 2.0).floor().max(2.0);
            let (a, b) = (Color::rgb(0.8, 0.8, 0.8), Color::rgb(0.45, 0.45, 0.45));
            let cols = (sw.w / cell).ceil() as i32;
            for row in 0..2 {
                for col in 0..cols {
                    let w = cell.min(sw.w - col as f32 * cell);
                    let cr = Rect::new(sw.x + col as f32 * cell, sw.y + row as f32 * cell, w, cell);
                    self.fill_rect(cr, if (row + col) % 2 == 0 { a } else { b });
                }
            }
        }
        self.fill_rect(sw, color);
        let (border, text) = (self.ui.theme.border, self.ui.theme.text);
        self.stroke_rect(
            sw,
            1.0,
            if r.hovered {
                self.ui.theme.accent
            } else {
                border
            },
        );
        let tr = Rect::new(sw.x + sw.w + 8.0, rect.y, rect.w - sw.w - 8.0, h);
        self.text_in(tr, shown, text, Align::Start);
        self.focus_ring(&r);
        let [r8, g8, b8, a8] = color.to_rgba8();
        self.node(
            id,
            Role::ColorSwatch,
            shown,
            Some(format!("#{r8:02X}{g8:02X}{b8:02X}{a8:02X}")),
            AccessState::default(),
            rect,
        );
        r
    }

    /// Shows `text` beside the pointer while the widget `r` stays hovered.
    pub fn tooltip(&mut self, r: &Response, text: &str) {
        let wait = 0.4;
        if r.hovered
            && self.ui.hover_id == r.id
            && self.ui.hover_time >= wait
            && self.ui.drag.is_none()
        {
            self.tooltip = Some((text.to_owned(), self.mouse));
        }
    }

    /// The widget id that tooltips and popups attach to.
    pub fn widget_id(&mut self, label: &str) -> WidgetId {
        self.id(label)
    }
}
