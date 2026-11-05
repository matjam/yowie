//! The settings window and the frame statistics.

use yowie::gfx::scene::Terrain;
use yowie::gfx::{Font, FrameStats, Graphics, TextStyle};
use yowie::ui::Frame;
use yowie::{Color, Rect, vec2};

/// What the settings window changes.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Tints each terrain chunk by its level of detail.
    pub show_levels: bool,
    /// Valley mist over the low ground.
    pub fog: bool,
    /// Multiplies the air between the camera and the ground.
    pub aerial: f32,
    /// The sun's height above the horizon in degrees.
    pub sun: f32,
    /// Draws the lake.
    pub water: bool,
    /// Draws far pines as impostors; off draws every pine as a model.
    pub impostors: bool,
    /// Draws the billboard trees.
    pub billboards: bool,
    /// Draws the place names.
    pub labels: bool,
    /// Lights the searchlight.
    pub searchlight: bool,
    /// The radius a click digs, world units.
    pub dig_radius: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_levels: false,
            fog: true,
            aerial: 4.0,
            sun: 14.0,
            water: true,
            impostors: true,
            billboards: true,
            labels: true,
            searchlight: true,
            dig_radius: 4.0,
        }
    }
}

impl Settings {
    /// Adds the window's widgets.
    pub fn widgets(&mut self, f: &mut Frame<'_>) {
        f.checkbox("Show chunk levels", &mut self.show_levels);
        f.checkbox("Valley fog", &mut self.fog);
        f.checkbox("Lake", &mut self.water);
        f.checkbox("Pine impostors", &mut self.impostors);
        f.checkbox("Billboard trees", &mut self.billboards);
        f.checkbox("Labels", &mut self.labels);
        f.checkbox("Searchlight", &mut self.searchlight);
        f.slider("Sun height", &mut self.sun, 2.0..=70.0);
        f.slider("Aerial haze", &mut self.aerial, 1.0..=20.0);
        f.slider("Dig radius", &mut self.dig_radius, 1.0..=10.0);
    }
}

/// Draws the last frame's counts in a box at the top left.
pub fn draw_stats(gfx: &mut Graphics, font: &Font, terrain: &Terrain, st: FrameStats) {
    let mut levels = [0u32; 4];
    for i in 0..terrain.chunk_count() {
        if let Some(l) = terrain.chunk_level(i) {
            levels[(l as usize).min(3)] += 1;
        }
    }
    let lines = [
        format!(
            "terrain: {} chunks drawn, {} culled; levels {} {} {} {}",
            st.terrain_chunks, st.terrain_chunks_culled, levels[0], levels[1], levels[2], levels[3]
        ),
        format!(
            "{} billboards, {} impostors; rock levels {} {} {}",
            st.billboards, st.impostors, st.lod_levels[0], st.lod_levels[1], st.lod_levels[2]
        ),
        format!(
            "{} meshes in {} draw calls, {} culled, {} lights",
            st.draws_3d, st.draw_calls_3d, st.culled_3d, st.lights
        ),
        "Left click digs; right drag looks; WASD, Q, E fly; Shift is fast".to_owned(),
    ];
    gfx.fill_rect(
        Rect::new(12.0, 12.0, 480.0, 92.0),
        Color::rgba(0.03, 0.04, 0.06, 0.7),
    );
    let style = TextStyle::new(14.0).color(Color::rgb8(225, 228, 238));
    for (i, line) in lines.iter().enumerate() {
        gfx.draw_text(font, line, vec2(22.0, 20.0 + i as f32 * 20.0), &style);
    }
}
