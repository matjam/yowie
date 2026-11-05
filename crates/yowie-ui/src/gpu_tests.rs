//! A GPU snapshot of a small interface. Skipped without a GPU adapter.

use yowie_core::{Rect, uvec2};
use yowie_gfx::{Graphics, GraphicsSettings};
use yowie_input::InputState;

use crate::{Theme, Ui};

fn render(theme_name: &str) -> Option<(yowie_gfx::image::RgbaImage, yowie_gfx::FrameStats)> {
    if !yowie_gfx::adapter_available() {
        println!("skipping GPU test: no GPU adapter");
        return None;
    }
    let mut gfx = Graphics::new_headless(uvec2(320, 220), GraphicsSettings::default()).ok()?;
    let mut ui = Ui::new(Theme::named(theme_name)?);
    let input = InputState::new();
    let (mut on, mut v, mut s) = (true, 0.6f32, String::from("Ada"));
    // Two frames so retained measurements settle.
    for _ in 0..2 {
        gfx.begin_frame();
        ui.draw(&mut gfx, &input, |f| {
            f.panel(Some("Snapshot"), Rect::new(10.0, 10.0, 300.0, 200.0), |f| {
                f.button("Press");
                f.checkbox("Enabled", &mut on);
                f.slider("Level", &mut v, 0.0..=1.0);
                f.text_field("Name", &mut s);
                f.progress("Load", 0.4);
            });
        });
    }
    let stats = gfx.end_frame().ok()?;
    let img = gfx.read_output().ok()?;
    if let Ok(dir) = std::env::var("YOWIE_TEST_OUT") {
        let _ = img.save(std::path::Path::new(&dir).join(format!("ui-{theme_name}.png")));
    }
    Some((img, stats))
}

#[test]
fn a_small_interface_draws_a_panel_widgets_and_text() {
    let Some((img, stats)) = render("nord") else {
        return;
    };
    // Outside the panel the clear colour (black) shows.
    assert_eq!(img.get_pixel(3, 3).0, [0, 0, 0, 255]);
    // The panel is nord's background (0x2E3440), blended at 96%.
    let p = img.get_pixel(20, 200).0;
    assert!(
        p[0] > 30 && p[0] < 70 && p[2] > 55 && p[2] < 90,
        "panel colour {p:?}"
    );
    // The slider's fill and the progress bar use the accent colour.
    let accent = img
        .pixels()
        .filter(|p| p.0[2] > 190 && p.0[0] < 160 && p.0[1] > 150)
        .count();
    assert!(accent > 300, "accent pixels: {accent}");
    // Text is light on the dark panel.
    let light = img
        .pixels()
        .filter(|p| p.0[0] > 200 && p.0[1] > 200)
        .count();
    assert!(light > 80, "light text pixels: {light}");
    // Fills, images and text are drawn in separate passes: about eight draw
    // calls (clip changes split runs) for dozens of primitives.
    assert!(stats.draw_calls <= 10, "draw calls: {}", stats.draw_calls);
}

#[test]
fn every_theme_renders_distinct_panels() {
    let mut firsts = Vec::new();
    for name in crate::theme_names() {
        let Some((img, _)) = render(name) else {
            return;
        };
        firsts.push(img.get_pixel(20, 200).0);
    }
    firsts.sort_unstable();
    firsts.dedup();
    assert!(firsts.len() >= 6, "themes look alike: {firsts:?}");
}
