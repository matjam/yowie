//! Renders into a headless output, reads the pixels back and checks them.
//! Every test skips with a printed message when no GPU adapter exists.

use image::RgbaImage;
use yowie_core::color::{linear_to_srgb, srgb_to_linear};
use yowie_core::{Color, Rect, UVec2, uvec2, vec2};

use crate::{BlendMode, Camera2D, Graphics, GraphicsSettings, Sprite, Texture, TextureOptions};

/// A headless renderer of `size` pixels, or `None` (with a message) when the
/// machine has no adapter.
pub(crate) fn headless_with(size: UVec2, settings: GraphicsSettings) -> Option<Graphics> {
    if !crate::adapter_available() {
        println!("skipping GPU test: no GPU adapter");
        return None;
    }
    match Graphics::new_headless(size, settings) {
        Ok(g) => Some(g),
        Err(e) => {
            println!("skipping GPU test: {e}");
            None
        }
    }
}

pub(crate) fn headless(size: UVec2) -> Option<Graphics> {
    headless_with(size, GraphicsSettings::default())
}

/// Renders the queued frame and reads it back.
pub(crate) fn render(gfx: &mut Graphics) -> RgbaImage {
    gfx.request_capture();
    let result = gfx.end_frame().and_then(|_| {
        gfx.take_capture()
            .unwrap_or_else(|| Err(crate::GfxError::Readback("no capture".into())))
    });
    match result {
        Ok(img) => img,
        Err(e) => panic!("frame failed: {e}"),
    }
}

/// Asserts each channel of pixel `(x, y)` is within `tol` of `want`.
#[track_caller]
pub(crate) fn assert_px(img: &RgbaImage, x: u32, y: u32, want: [u8; 4], tol: u8) {
    let got = img.get_pixel(x, y).0;
    let ok = got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= tol);
    assert!(
        ok,
        "pixel {x},{y} is {got:?}, want {want:?} (tolerance {tol})"
    );
}

pub(crate) fn white(gfx: &Graphics) -> Texture {
    match Texture::from_pixels(gfx, 2, 2, &[255; 16], TextureOptions::default()) {
        Ok(t) => t,
        Err(e) => panic!("texture: {e}"),
    }
}

fn byte(linear: f32) -> u8 {
    (linear_to_srgb(linear.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8
}

#[test]
fn clear_colour_fills_the_frame() {
    let Some(mut gfx) = headless(uvec2(16, 8)) else {
        return;
    };
    gfx.set_clear_color(Color::rgb8(30, 60, 90));
    gfx.begin_frame();
    let img = render(&mut gfx);
    assert_px(&img, 0, 0, [30, 60, 90, 255], 1);
    assert_px(&img, 15, 7, [30, 60, 90, 255], 1);
}

#[test]
fn sprite_lands_where_placed_with_its_tint() {
    let Some(mut gfx) = headless(uvec2(32, 32)) else {
        return;
    };
    let tex = white(&gfx);
    gfx.draw_sprite(
        &tex,
        &Sprite::at(vec2(8.0, 8.0))
            .size(vec2(8.0, 8.0))
            .tint(Color::RED),
    );
    let img = render(&mut gfx);
    assert_px(&img, 8, 8, [255, 0, 0, 255], 0);
    assert_px(&img, 15, 15, [255, 0, 0, 255], 0);
    assert_px(&img, 7, 8, [0, 0, 0, 255], 0);
    assert_px(&img, 16, 16, [0, 0, 0, 255], 0);
}

#[test]
fn source_rect_and_flip_pick_texels() {
    let Some(mut gfx) = headless(uvec2(16, 8)) else {
        return;
    };
    let px = [255, 0, 0, 255, 0, 255, 0, 255];
    let Ok(tex) = Texture::from_pixels(&gfx, 2, 1, &px, TextureOptions::default()) else {
        panic!("texture");
    };
    gfx.draw_sprite(
        &tex,
        &Sprite::at(vec2(0.0, 0.0)).size(vec2(8.0, 8.0)).flip_x(true),
    );
    let src = Rect::new(1.0, 0.0, 1.0, 1.0);
    gfx.draw_sprite(
        &tex,
        &Sprite::at(vec2(8.0, 0.0)).size(vec2(8.0, 8.0)).source(src),
    );
    let img = render(&mut gfx);
    assert_px(&img, 1, 4, [0, 255, 0, 255], 0);
    assert_px(&img, 6, 4, [255, 0, 0, 255], 0);
    assert_px(&img, 12, 4, [0, 255, 0, 255], 0);
}

#[test]
fn blend_modes_combine_in_linear_light() {
    let Some(mut gfx) = headless(uvec2(70, 10)) else {
        return;
    };
    let grey = Color::rgb8(128, 128, 128);
    let d = srgb_to_linear(128.0 / 255.0);
    gfx.set_clear_color(grey);
    gfx.begin_frame();
    let tex = white(&gfx);
    let half = Color::WHITE.with_alpha(0.5);
    let cases = [
        (BlendMode::Alpha, half, [byte(d * 0.5 + 0.5); 3], 255),
        (BlendMode::Additive, half, [byte(d + 0.5); 3], 255),
        (BlendMode::Multiply, Color::RED, [byte(d), 0, 0], 255),
        (BlendMode::Screen, Color::WHITE, [255; 3], 255),
        // A result near black would test sRGB rounding, not subtraction:
        // the curve is steep there and drivers round differently.
        (
            BlendMode::Subtract,
            Color::rgb(0.25, 0.25, 0.25),
            [byte(d - srgb_to_linear(0.25)); 3],
            255,
        ),
        (BlendMode::Replace, Color::TRANSPARENT, [0; 3], 0),
        (
            BlendMode::PremultipliedAlpha,
            Color::rgba(1.0, 0.0, 0.0, 0.0),
            [byte(d + 1.0), byte(d), byte(d)],
            255,
        ),
    ];
    for (i, (mode, tint, _, _)) in cases.iter().enumerate() {
        gfx.with_blend(*mode, |g| {
            let at = vec2(i as f32 * 10.0, 0.0);
            g.draw_sprite(&tex, &Sprite::at(at).size(vec2(10.0, 10.0)).tint(*tint));
        });
    }
    let img = render(&mut gfx);
    for (i, (mode, _, rgb, a)) in cases.iter().enumerate() {
        let got = img.get_pixel(i as u32 * 10 + 5, 5).0;
        let want = [rgb[0], rgb[1], rgb[2], *a];
        let ok = got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 2);
        assert!(ok, "{mode:?}: got {got:?}, want {want:?}");
    }
}

#[test]
fn camera_moves_zooms_and_culls() {
    let Some(mut gfx) = headless(uvec2(32, 32)) else {
        return;
    };
    let tex = white(&gfx);
    let cam = Camera2D::new(vec2(100.0, 100.0)).with_zoom(2.0);
    gfx.with_camera(&cam, |g| {
        g.draw_sprite(
            &tex,
            &Sprite::at(vec2(100.0, 100.0))
                .size(vec2(4.0, 4.0))
                .centered(),
        );
        // Far outside the camera's view: culled before it is queued.
        g.draw_sprite(&tex, &Sprite::at(vec2(1000.0, 0.0)));
    });
    let img = render(&mut gfx);
    assert_px(&img, 16, 16, [255; 4], 0);
    assert_px(&img, 12, 12, [255; 4], 0);
    assert_px(&img, 11, 16, [0, 0, 0, 255], 0);
    assert_px(&img, 20, 16, [0, 0, 0, 255], 0);
    assert_eq!(gfx.stats().vertices, 4, "the culled sprite was dropped");
}

#[test]
fn layers_then_sort_keys_then_call_order() {
    let Some(mut gfx) = headless(uvec2(20, 10)) else {
        return;
    };
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    gfx.with_layer(1, |g| g.fill_rect(r, Color::RED));
    gfx.fill_rect(r, Color::GREEN);
    let r2 = Rect::new(10.0, 0.0, 10.0, 10.0);
    gfx.set_sort_key(2.0);
    gfx.fill_rect(r2, Color::RED);
    gfx.set_sort_key(1.0);
    gfx.fill_rect(r2, Color::GREEN);
    let img = render(&mut gfx);
    assert_px(&img, 5, 5, [255, 0, 0, 255], 0);
    assert_px(&img, 15, 5, [255, 0, 0, 255], 0);
}

#[test]
fn letterboxed_fixed_view_maps_into_the_window() {
    let settings = GraphicsSettings {
        view: crate::ViewSettings {
            fixed: Some(uvec2(10, 10)),
            scale: crate::ScaleMode::Letterbox,
        },
        clear_color: Color::RED,
        ..GraphicsSettings::default()
    };
    let Some(mut gfx) = headless_with(uvec2(40, 20), settings) else {
        return;
    };
    assert_eq!(gfx.view_size(), vec2(10.0, 10.0));
    assert_eq!(gfx.window_to_view(vec2(20.0, 10.0)), vec2(5.0, 5.0));
    gfx.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::GREEN);
    let img = render(&mut gfx);
    assert_px(&img, 2, 10, [0, 0, 0, 255], 0);
    assert_px(&img, 37, 10, [0, 0, 0, 255], 0);
    assert_px(&img, 25, 15, [255, 0, 0, 255], 0);
    assert_px(&img, 12, 2, [0, 255, 0, 255], 0);
}

#[test]
fn shapes_cover_their_area() {
    let Some(mut gfx) = headless(uvec2(40, 20)) else {
        return;
    };
    gfx.fill_circle(vec2(10.0, 10.0), 6.0, Color::BLUE);
    gfx.line(vec2(20.0, 10.0), vec2(38.0, 10.0), 4.0, Color::GREEN);
    gfx.stroke_rect(Rect::new(20.0, 0.0, 20.0, 6.0), 2.0, Color::RED);
    let img = render(&mut gfx);
    assert_px(&img, 10, 10, [0, 0, 255, 255], 0);
    assert_px(&img, 10, 5, [0, 0, 255, 255], 0);
    assert_px(&img, 4, 4, [0, 0, 0, 255], 0);
    assert_px(&img, 30, 10, [0, 255, 0, 255], 0);
    assert_px(&img, 30, 13, [0, 0, 0, 255], 0);
    assert_px(&img, 30, 0, [255, 0, 0, 255], 0);
    assert_px(&img, 30, 3, [0, 0, 0, 255], 0);
}

#[test]
fn nine_slice_keeps_corners_and_stretches_the_centre() {
    let Some(mut gfx) = headless(uvec2(40, 20)) else {
        return;
    };
    // A 3x3 texture: red corners, green edges, blue centre.
    let (r, g, b) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]);
    let grid = [r, g, r, g, b, g, r, g, r];
    let px: Vec<u8> = grid.iter().flatten().copied().collect();
    let Ok(tex) = Texture::from_pixels(&gfx, 3, 3, &px, TextureOptions::default()) else {
        panic!("texture");
    };
    let slice = crate::NineSlice::uniform(tex, 1.0);
    gfx.draw_nine_slice(&slice, Rect::new(0.0, 0.0, 40.0, 20.0), Color::WHITE);
    let img = render(&mut gfx);
    assert_px(&img, 0, 0, r, 0);
    assert_px(&img, 39, 19, r, 0);
    assert_px(&img, 20, 0, g, 0);
    assert_px(&img, 0, 10, g, 0);
    assert_px(&img, 20, 10, b, 0);
    assert_px(&img, 1, 1, b, 0);
}

#[test]
fn render_texture_draws_then_samples_in_one_frame() {
    let Some(mut gfx) = headless(uvec2(16, 16)) else {
        return;
    };
    let Ok(rt) = crate::RenderTexture::new(&gfx, 8, 8, Default::default()) else {
        panic!("render texture");
    };
    gfx.draw_to(&rt, Some(Color::BLUE), |g| {
        g.fill_rect(Rect::new(0.0, 0.0, 4.0, 8.0), Color::GREEN);
    });
    gfx.draw_texture_rect(rt.texture(), Rect::new(0.0, 0.0, 16.0, 16.0), Color::WHITE);
    let img = render(&mut gfx);
    assert_px(&img, 2, 8, [0, 255, 0, 255], 0);
    assert_px(&img, 12, 8, [0, 0, 255, 255], 0);
    match rt.read(&gfx) {
        Ok(back) => assert_px(&back, 6, 1, [0, 0, 255, 255], 0),
        Err(e) => panic!("read back: {e}"),
    }
}
