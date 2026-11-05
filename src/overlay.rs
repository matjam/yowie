//! The F3 frame-timing overlay: frame rate, update and draw times, draw
//! calls, and a graph of recent frame times.

use std::time::Instant;

use yowie_core::{Color, Rect, vec2};
use yowie_gfx::{BlendMode, FrameStats, Graphics, TextStyle, layers};

/// Frames kept for the graph.
const HISTORY: usize = 120;
/// Frame time at the top of the graph, in milliseconds.
const GRAPH_MS: f32 = 1000.0 / 30.0;

/// Frame timings and the overlay that shows them.
#[derive(Debug)]
pub(crate) struct Overlay {
    pub(crate) enabled: bool,
    frames: [f32; HISTORY],
    next: usize,
    update_ms: f32,
    draw_ms: f32,
    text: String,
    refreshed: Option<Instant>,
}

impl Overlay {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            frames: [0.0; HISTORY],
            next: 0,
            update_ms: 0.0,
            draw_ms: 0.0,
            text: String::new(),
            refreshed: None,
        }
    }

    /// Records one frame's wall time and the time its updates and draw
    /// took, all in milliseconds.
    pub(crate) fn record(&mut self, frame_ms: f32, update_ms: f32, draw_ms: f32) {
        self.frames[self.next] = frame_ms;
        self.next = (self.next + 1) % HISTORY;
        // Smooth the per-frame costs so the figures can be read.
        self.update_ms += (update_ms - self.update_ms) * 0.1;
        self.draw_ms += (draw_ms - self.draw_ms) * 0.1;
    }

    /// Queues the overlay over everything else in screen space.
    pub(crate) fn draw(&mut self, gfx: &mut Graphics, fps: f32, stats: FrameStats) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        if self
            .refreshed
            .is_none_or(|t| now.duration_since(t).as_secs_f32() >= 0.25)
        {
            let frame_ms = if fps > 0.0 { 1000.0 / fps } else { 0.0 };
            self.text = format!(
                "{fps:5.1} fps  {frame_ms:5.2} ms\nupdate {:5.2} ms  draw {:5.2} ms\n{} draws  {} vertices",
                self.update_ms, self.draw_ms, stats.draw_calls, stats.vertices
            );
            self.refreshed = Some(now);
        }
        gfx.set_camera(None);
        gfx.set_clip(None);
        gfx.set_shader(None);
        gfx.set_blend(BlendMode::Alpha);
        gfx.set_sort_key(0.0);
        gfx.set_layer(layers::DEBUG);
        let (x, y, w) = (8.0, 8.0, 2.0 * HISTORY as f32 + 16.0);
        let graph_h = 40.0;
        let font = gfx.default_mono_font();
        let style = TextStyle::new(12.0).color(Color::WHITE);
        let text_h = gfx.measure_text(&font, &self.text, &style).y;
        let panel = Rect::new(x, y, w, text_h + graph_h + 24.0);
        gfx.fill_rect(panel, Color::rgba(0.0, 0.0, 0.0, 0.7));
        gfx.draw_text(&font, &self.text, vec2(x + 8.0, y + 6.0), &style);
        let base = y + 14.0 + text_h + graph_h;
        for i in 0..HISTORY {
            let ms = self.frames[(self.next + i) % HISTORY];
            let h = (ms / GRAPH_MS).min(1.0) * graph_h;
            let color = if ms <= 17.0 {
                Color::rgb(0.3, 0.9, 0.4)
            } else if ms <= GRAPH_MS {
                Color::rgb(1.0, 0.8, 0.2)
            } else {
                Color::rgb(1.0, 0.3, 0.3)
            };
            gfx.fill_rect(Rect::new(x + 8.0 + 2.0 * i as f32, base - h, 2.0, h), color);
        }
        let target = base - (1000.0 / 60.0) / GRAPH_MS * graph_h;
        gfx.fill_rect(
            Rect::new(x + 8.0, target, 2.0 * HISTORY as f32, 1.0),
            Color::rgba(1.0, 1.0, 1.0, 0.5),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yowie_core::uvec2;
    use yowie_gfx::GraphicsSettings;

    #[test]
    fn overlay_draws_a_panel_with_text_and_bars() {
        if !yowie_gfx::adapter_available() {
            println!("skipping GPU test: no GPU adapter");
            return;
        }
        let settings = GraphicsSettings {
            clear_color: Color::WHITE,
            ..GraphicsSettings::default()
        };
        let mut gfx = match Graphics::new_headless(uvec2(300, 140), settings) {
            Ok(g) => g,
            Err(e) => panic!("{e}"),
        };
        let mut overlay = Overlay::new(true);
        for i in 0..HISTORY {
            overlay.record(if i % 10 == 0 { 40.0 } else { 16.0 }, 0.5, 1.0);
        }
        overlay.draw(&mut gfx, 60.0, FrameStats::default());
        let img = match gfx.end_frame().and_then(|_| gfx.read_output()) {
            Ok(img) => img,
            Err(e) => panic!("{e}"),
        };
        if let Ok(dir) = std::env::var("YOWIE_TEST_OUT") {
            let _ = img.save(std::path::Path::new(&dir).join("overlay.png"));
        }
        // The panel darkens the white frame (70% black over white in linear
        // light is sRGB 149); outside it stays white.
        assert!(img.get_pixel(12, 12).0[0] < 170);
        assert_eq!(img.get_pixel(290, 130).0, [255, 255, 255, 255]);
        let lit_text = (16..200)
            .flat_map(|x| (12..24).map(move |y| (x, y)))
            .filter(|&(x, y)| img.get_pixel(x, y).0[0] > 220)
            .count();
        assert!(lit_text > 20, "text in the panel: {lit_text} lit pixels");
    }
}
