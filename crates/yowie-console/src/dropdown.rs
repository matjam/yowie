//! Keys that control the console, and the drop-down command line.

use yowie_core::{Color, Rect, vec2};
use yowie_input::{InputState, Key};
use yowie_ui::{Frame, TextOptions};

use crate::console::Core;

impl Core {
    /// Reads the toggle keys, fires bindings while the console is shut and
    /// handles history, completion and scrolling while it is open.
    pub(crate) fn handle_keys(&mut self, input: &InputState) {
        self.just_toggled = false;
        let toggle = self.opts.toggle_key;
        let panel = self.opts.panel_key;
        if input.key_pressed(toggle) {
            self.open = !self.open;
            self.just_toggled = true;
            self.scroll = 0;
        }
        if input.key_pressed(panel) {
            self.panels_open = !self.panels_open;
        }
        if !self.open {
            self.fire_bindings(input, toggle, panel);
            return;
        }
        if self.just_toggled {
            return;
        }
        if input.key_pressed(Key::Escape) {
            self.open = false;
            return;
        }
        let step = |k: Key| input.key_pressed(k) || input.key_repeated(k);
        if step(Key::Up) {
            if let Some(s) = self.history.back(&self.line) {
                self.line = s.to_owned();
                self.caret_end = true;
            }
        }
        if step(Key::Down) {
            if let Some(s) = self.history.forward() {
                self.line = s.to_owned();
                self.caret_end = true;
            }
        }
        if input.key_pressed(Key::Tab) {
            self.complete_in_place();
        }
        let page = 8;
        if step(Key::PageUp) {
            self.scroll = (self.scroll + page).min(self.capture.len());
        }
        if step(Key::PageDown) {
            self.scroll = self.scroll.saturating_sub(page);
        }
    }

    fn fire_bindings(&mut self, input: &InputState, toggle: Key, panel: Key) {
        let hits: Vec<String> = self
            .binds
            .iter()
            .filter(|(k, _)| **k != toggle && **k != panel && input.key_pressed(**k))
            .map(|(_, c)| c.clone())
            .collect();
        for line in hits {
            self.run_line(&line, 0);
        }
    }

    fn complete_in_place(&mut self) {
        let c = self.complete_line();
        match c.candidates.len() {
            0 => {}
            1 => {
                self.line = c.apply(&self.line);
                self.caret_end = true;
            }
            _ => {
                let shown = c.candidates.join("  ");
                self.print(&shown);
                self.line = c.apply(&self.line);
                self.caret_end = true;
            }
        }
    }

    /// Builds the drop-down (when open) and the panels window (when shown).
    pub(crate) fn build(&mut self, f: &mut Frame<'_>, input: &InputState) {
        if self.open {
            self.build_dropdown(f, input);
        }
        if self.panels_open {
            self.build_panels(f, input);
        }
    }

    fn build_dropdown(&mut self, f: &mut Frame<'_>, input: &InputState) {
        let view = f.view();
        let theme = f.theme().clone();
        let h = (view.h * self.opts.height).max(140.0).min(view.h);
        let rect = Rect::new(0.0, 0.0, view.w, h);
        let line_h = f.measure_mono("Ag").y.max(1.0);
        let row_h = (line_h + 10.0).max(theme.row_height);
        f.fill_rect(rect, Color::rgba(0.04, 0.05, 0.07, 0.93));
        f.fill_rect(Rect::new(0.0, h - 1.0, view.w, 1.0), theme.accent);
        // Output.
        let out = Rect::new(8.0, 4.0, view.w - 16.0, h - row_h - 10.0);
        let visible = ((out.h / line_h).floor() as usize).max(1);
        let total = self.capture.len();
        if out.contains(f.pointer()) && input.wheel().y != 0.0 {
            let delta = (input.wheel().y * 3.0).round() as i64;
            self.scroll = (self.scroll as i64 + delta).clamp(0, total as i64) as usize;
        }
        self.scroll = self.scroll.min(total.saturating_sub(visible));
        let lines = self.capture.window(visible, self.scroll);
        let y0 = out.y + out.h - lines.len() as f32 * line_h;
        f.area(out, |f| {
            for (i, l) in lines.iter().enumerate() {
                let color = match l.level {
                    log::Level::Error => Color::rgb(1.0, 0.45, 0.45),
                    log::Level::Warn => Color::rgb(1.0, 0.82, 0.4),
                    log::Level::Info => theme.text,
                    log::Level::Debug => Color::rgb(0.55, 0.7, 0.95),
                    log::Level::Trace => theme.accent,
                };
                let text = if l.target == "console" || l.target == "input" {
                    l.text.clone()
                } else {
                    format!("[{}] {}", l.target, l.text)
                };
                f.text_at_mono(vec2(out.x, y0 + i as f32 * line_h), &text, color);
            }
        });
        if self.scroll > 0 {
            let note = format!("-- {} lines up, PageDown to return --", self.scroll);
            let w = f.measure_mono(&note).x;
            f.text_at_mono(vec2(view.w - w - 12.0, out.y + out.h - line_h), &note, theme.text_dim);
        }
        // Command line.
        let input_y = h - row_h - 3.0;
        f.text_at_mono(vec2(10.0, input_y + (row_h - line_h) * 0.5), ">", theme.accent);
        let field = Rect::new(26.0, input_y, view.w - 36.0, row_h);
        let opts = TextOptions {
            placeholder: "type help for commands",
            mono: true,
            frameless: true,
            request_focus: !self.just_toggled,
            caret_to_end: std::mem::take(&mut self.caret_end),
            ..TextOptions::default()
        };
        let mut line = std::mem::take(&mut self.line);
        let mut resp = None;
        if !self.just_toggled {
            f.area(field, |f| {
                resp = Some(f.text_field_with("console input", &mut line, &opts));
            });
        }
        self.line = line;
        if let Some(r) = resp {
            if r.changed {
                self.history.reset();
            }
            if r.submitted {
                let text = std::mem::take(&mut self.line);
                self.execute(&text);
                self.scroll = 0;
            }
        }
    }
}
