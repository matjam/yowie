//! GPU tests for text and game shaders.

use bytemuck::{Pod, Zeroable};
use image::RgbaImage;
use yowie_core::{Color, Rect, uvec2, vec2};

use crate::gpu_tests::{assert_px, headless, render};
use crate::{TextAlign, TextStyle};

fn lit(img: &RgbaImage, area: Rect) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for y in area.y as u32..(area.y + area.h) as u32 {
        for x in area.x as u32..(area.x + area.w) as u32 {
            let p = img.get_pixel(x, y).0;
            if p[0].max(p[1]).max(p[2]) > 128 {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn text_covers_pixels_in_its_colour() {
    let Some(mut gfx) = headless(uvec2(64, 32)) else {
        return;
    };
    let font = gfx.default_font();
    let style = TextStyle::new(24.0).color(Color::RED);
    let size = gfx.measure_text(&font, "Wm", &style);
    assert!(size.x > 20.0 && size.y >= 24.0, "measured {size:?}");
    gfx.draw_text(&font, "Wm", vec2(2.0, 2.0), &style);
    let img = render(&mut gfx);
    let on = lit(&img, Rect::new(0.0, 0.0, 64.0, 32.0));
    assert!(on.len() > 40, "only {} lit pixels", on.len());
    assert!(on.iter().all(|p| p[1] < 40 && p[2] < 40), "text is red");
    let right = lit(
        &img,
        Rect::new(2.0 + size.x + 2.0, 0.0, 64.0 - size.x - 4.0, 32.0),
    );
    assert!(right.is_empty(), "nothing past the measured width");
}

#[test]
fn wrapping_and_alignment() {
    let Some(mut gfx) = headless(uvec2(100, 60)) else {
        return;
    };
    let font = gfx.default_mono_font();
    let wrapped = TextStyle::new(10.0).width(30.0);
    let one = gfx.measure_text(&font, "aaa bbb ccc", &TextStyle::new(10.0));
    let many = gfx.measure_text(&font, "aaa bbb ccc", &wrapped);
    assert!(
        many.y >= one.y * 2.5,
        "wrapped into three lines: {one:?} {many:?}"
    );
    gfx.draw_text(
        &font,
        "ii",
        vec2(0.0, 0.0),
        &TextStyle::new(16.0).width(100.0).align(TextAlign::Right),
    );
    let img = render(&mut gfx);
    assert!(
        lit(&img, Rect::new(0.0, 0.0, 50.0, 20.0)).is_empty(),
        "right-aligned text leaves the left empty"
    );
    assert!(!lit(&img, Rect::new(80.0, 0.0, 20.0, 20.0)).is_empty());
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    color: [f32; 4],
}

const SOLID: &str = "
struct Params { color: vec4<f32> };
@group(2) @binding(0) var<uniform> params: Params;

fn fragment(input: SpriteInput) -> vec4<f32> {
    return params.color * sample_texture(input.uv).a;
}
";

#[test]
fn shader_uniforms_snapshot_per_draw() {
    let Some(mut gfx) = headless(uvec2(20, 10)) else {
        return;
    };
    let shader = match gfx.compile_shader_with_uniforms(
        SOLID,
        &Params {
            color: [0.0, 1.0, 0.0, 1.0],
        },
    ) {
        Ok(s) => s,
        Err(e) => panic!("{e}"),
    };
    gfx.with_shader(&shader, |g| {
        g.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::WHITE)
    });
    assert!(
        shader
            .set_uniforms(&Params {
                color: [0.0, 0.0, 1.0, 1.0]
            })
            .is_ok()
    );
    gfx.with_shader(&shader, |g| {
        g.fill_rect(Rect::new(10.0, 0.0, 10.0, 10.0), Color::WHITE)
    });
    assert!(
        shader.set_uniforms(&[0u8; 4]).is_err(),
        "wrong size is refused"
    );
    let img = render(&mut gfx);
    assert_px(&img, 5, 5, [0, 255, 0, 255], 0);
    assert_px(&img, 15, 5, [0, 0, 255, 255], 0);
}

#[test]
fn shader_errors_name_the_game_line_and_keep_the_old_program() {
    let Some(mut gfx) = headless(uvec2(10, 10)) else {
        return;
    };
    let bad = "fn fragment(input: SpriteInput) -> vec4<f32> {\n    return undefined_thing;\n}\n";
    match gfx.compile_shader(bad) {
        Ok(_) => panic!("bad source compiled"),
        Err(e) => {
            let text = e.to_string();
            assert!(text.contains("line 2"), "{text}");
        }
    }
    let red =
        "fn fragment(input: SpriteInput) -> vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }";
    let Ok(shader) = gfx.compile_shader(red) else {
        panic!("good source failed");
    };
    let wrong_interface = "fn fragment(input: SpriteInput) -> f32 { return 1.0; }";
    assert!(shader.reload_source(&gfx, wrong_interface).is_err());
    gfx.with_shader(&shader, |g| {
        g.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::WHITE)
    });
    let img = render(&mut gfx);
    assert_px(&img, 5, 5, [255, 0, 0, 255], 0);
    let green =
        "fn fragment(input: SpriteInput) -> vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }";
    assert!(shader.reload_source(&gfx, green).is_ok());
    gfx.with_shader(&shader, |g| {
        g.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Color::WHITE)
    });
    let img = render(&mut gfx);
    assert_px(&img, 5, 5, [0, 255, 0, 255], 0);
}
