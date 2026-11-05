//! The debug panels window and the always-available panels: frame
//! timings, GPU, input and services.

use yowie_core::{Color, Rect, vec2};
use yowie_input::{GamepadAxis, GamepadButton, InputState, MouseButton};
use yowie_ui::{Frame, Size, TextOptions, WindowOptions};

use crate::console::Core;

const BUILTIN_PANELS: [&str; 4] = ["Engine", "GPU", "Input", "Services"];

impl Core {
    /// The tab names, in order: the built-in panels, the simulation panels
    /// that have data, then the game's own.
    pub(crate) fn panel_names(&self) -> Vec<String> {
        let mut names: Vec<String> = BUILTIN_PANELS.iter().map(|s| (*s).to_owned()).collect();
        names.extend(self.sim.panel_names().into_iter().map(str::to_owned));
        names.extend(self.user_panels.iter().map(|(n, _)| n.clone()));
        names
    }

    pub(crate) fn build_panels(&mut self, f: &mut Frame<'_>, input: &InputState) {
        if self.panel_rect.w <= 0.0 {
            self.panel_rect = Core::default_panel_rect(f.view());
        }
        let names = self.panel_names();
        self.tab = self.tab.min(names.len().saturating_sub(1));
        let mut rect = self.panel_rect;
        let opts = WindowOptions {
            closable: true,
            min_size: vec2(300.0, 200.0),
            ..WindowOptions::default()
        };
        let ((), closed) = f.window_with("Debug panels", opts, &mut rect, |f| {
            f.tabs("panel tabs", &names, &mut self.tab);
            f.space(4.0);
            let name = names.get(self.tab).cloned().unwrap_or_default();
            match name.as_str() {
                "Engine" => self.engine_panel(f),
                "GPU" => self.gpu_panel(f),
                "Input" => self.input_panel(f, input),
                "Services" => self.services_panel(f),
                other => {
                    if !self.sim_panel(other, f)
                        && let Some((_, body)) = self.user_panels.iter_mut().find(|(n, _)| n == other)
                    {
                        body(f);
                    }
                }
            }
        });
        self.panel_rect = rect;
        if closed {
            self.panels_open = false;
        }
    }

    fn kv(f: &mut Frame<'_>, key: &str, value: &str) {
        f.row(&[Size::Fixed(130.0), Size::Fill(1.0)], |f| {
            f.label_dim(key);
            f.label(value);
        });
    }

    fn engine_panel(&mut self, f: &mut Frame<'_>) {
        let Some(last) = self.frames.back().copied() else {
            f.label_dim("no frames recorded yet");
            return;
        };
        Self::kv(f, "frame rate", &format!("{:.1} fps", last.fps));
        Self::kv(f, "frame time", &format!("{:.2} ms", last.frame_ms));
        Self::kv(f, "update", &format!("{:.2} ms", last.update_ms));
        Self::kv(f, "draw", &format!("{:.2} ms", last.draw_ms));
        f.space(4.0);
        let graph = f.allocate(0.0, 110.0);
        let (panel, accent, orange) = (f.theme().track, f.theme().accent, Color::rgb(1.0, 0.6, 0.25));
        f.fill_rect(graph, panel);
        const TOP_MS: f32 = 1000.0 / 20.0;
        let n = self.frames.len().max(1);
        let col_w = (graph.w / 240.0).max(1.0);
        let start = graph.x + graph.w - n as f32 * col_w;
        for (i, t) in self.frames.iter().enumerate() {
            let u = (t.update_ms / TOP_MS).min(1.0) * graph.h;
            let d = ((t.draw_ms / TOP_MS) * graph.h).min(graph.h - u);
            let x = start + i as f32 * col_w;
            f.fill_rect(Rect::new(x, graph.y + graph.h - u, col_w, u), accent);
            f.fill_rect(Rect::new(x, graph.y + graph.h - u - d, col_w, d), orange);
        }
        let target = graph.y + graph.h - (1000.0 / 60.0 / TOP_MS) * graph.h;
        f.fill_rect(Rect::new(graph.x, target, graph.w, 1.0), Color::rgba(1.0, 1.0, 1.0, 0.5));
        let dim = f.theme().text_dim;
        f.text_at(vec2(graph.x + 4.0, target - 16.0), "16.7 ms", dim);
        f.label_dim("blue: update, orange: draw");
        f.separator();
        let s = last.stats;
        Self::kv(f, "draw calls", &s.draw_calls.to_string());
        Self::kv(f, "vertices", &s.vertices.to_string());
        Self::kv(f, "indices", &s.indices.to_string());
        Self::kv(f, "outputs", &s.outputs.to_string());
    }

    fn gpu_panel(&mut self, f: &mut Frame<'_>) {
        let g = self.gpu.clone();
        Self::kv(f, "adapter", &g.adapter);
        Self::kv(f, "backend", &g.backend);
        Self::kv(f, "type", &g.device_type);
        Self::kv(f, "output", &format!("{} x {} px, {}", g.output.0, g.output.1, g.format));
        Self::kv(f, "view", &format!("{:.0} x {:.0} units, {:.2} px per unit", g.view.0, g.view.1, g.pixels_per_unit));
        Self::kv(f, "draw calls", &g.stats.draw_calls.to_string());
        f.separator();
        f.heading("Textures");
        if self.textures.is_empty() {
            f.label_dim("none watched; call Console::watch_texture");
            return;
        }
        let mut total = 0u64;
        for (name, t) in &self.textures {
            let bytes = u64::from(t.width()) * u64::from(t.height()) * 4;
            total += bytes;
            Self::kv(f, name, &format!("{} x {}  {}", t.width(), t.height(), human(bytes)));
        }
        Self::kv(f, "total (estimate)", &human(total));
    }

    fn input_panel(&mut self, f: &mut Frame<'_>, input: &InputState) {
        let keys: Vec<&str> = input.keys_down().map(|k| k.name()).collect();
        Self::kv(f, "keys down", &if keys.is_empty() { "none".to_owned() } else { keys.join(" ") });
        let m = input.mouse();
        Self::kv(f, "pointer", &format!("{:.0}, {:.0}", m.x, m.y));
        let buttons: Vec<&str> = MouseButton::ALL
            .iter()
            .filter(|b| input.mouse_down(**b))
            .map(|b| b.name())
            .collect();
        Self::kv(f, "buttons", &if buttons.is_empty() { "none".to_owned() } else { buttons.join(" ") });
        let w = input.wheel();
        Self::kv(f, "wheel", &format!("{:.1}, {:.1}", w.x, w.y));
        let mods = input.modifiers();
        let mod_names: Vec<&str> = [
            (yowie_input::Modifiers::SHIFT, "shift"),
            (yowie_input::Modifiers::CONTROL, "ctrl"),
            (yowie_input::Modifiers::ALT, "alt"),
            (yowie_input::Modifiers::SUPER, "super"),
        ]
        .iter()
        .filter(|(m, _)| mods.contains(*m))
        .map(|(_, n)| *n)
        .collect();
        Self::kv(f, "modifiers", &if mod_names.is_empty() { "none".to_owned() } else { mod_names.join(" ") });
        f.separator();
        let mut any = false;
        for pad in input.gamepads() {
            any = true;
            f.heading(&format!("gamepad {}: {}", pad.id().0, pad.name()));
            let down: Vec<&str> = GamepadButton::ALL
                .iter()
                .filter(|b| pad.down(**b))
                .map(|b| b.name())
                .collect();
            Self::kv(f, "buttons", &if down.is_empty() { "none".to_owned() } else { down.join(" ") });
            for axis in GamepadAxis::ALL {
                let v = pad.axis(*axis);
                let bar = f.allocate(0.0, 10.0);
                let (track, accent) = (f.theme().track, f.theme().accent);
                f.fill_rect(bar, track);
                let half = bar.w * 0.5;
                let len = v.abs().min(1.0) * half;
                let x = if v >= 0.0 { bar.x + half } else { bar.x + half - len };
                f.fill_rect(Rect::new(x, bar.y, len, bar.h), accent);
                let dim = f.theme().text_dim;
                f.text_at(vec2(bar.x + 4.0, bar.y - 5.0), axis.name(), dim);
            }
        }
        if !any {
            f.label_dim("no gamepads connected");
        }
    }

    fn services_panel(&mut self, f: &mut Frame<'_>) {
        f.heading("Variables");
        if self.vars.is_empty() {
            f.label_dim("none registered");
        }
        let names: Vec<String> = self.vars.keys().cloned().collect();
        for name in names {
            let Some(var) = self.vars.get(&name).cloned() else {
                continue;
            };
            f.with_id(&name, |f| {
                f.row(&[Size::Fixed(150.0), Size::Fill(1.0)], |f| {
                    f.label(&name);
                    let buf = self.var_edits.entry(name.clone()).or_default();
                    let focused = f.has_focus(f.peek_id("##value"));
                    if !focused {
                        *buf = var.show();
                    }
                    let r = f.text_field_with("##value", buf, &TextOptions { mono: true, ..TextOptions::default() });
                    if r.submitted {
                        if let Err(e) = var.assign(buf) {
                            self.pending_error = Some(e);
                        }
                        *buf = var.show();
                    }
                });
            });
        }
        if let Some(e) = self.pending_error.take() {
            self.print_error(&e);
        }
        f.separator();
        f.heading("Services");
        if self.infos.is_empty() {
            f.label_dim("none attached; call Console::add_info");
        }
        for (name, info) in &mut self.infos {
            let text = info();
            Self::kv(f, name, &text);
        }
        f.separator();
        f.heading("Key bindings");
        if self.binds.is_empty() {
            f.label_dim("none; try bind F5 echo hello");
        }
        for (k, c) in &self.binds {
            Self::kv(f, k.name(), c);
        }
    }
}

/// A byte count in KiB or MiB.
fn human(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / f64::from(1 << 20))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::human;

    #[test]
    fn byte_counts_read_in_kib_and_mib() {
        assert_eq!(human(2048), "2.0 KiB");
        assert_eq!(human(3 << 20), "3.0 MiB");
    }
}
