//! Replacing textures and shaders in place, HDR and compressed uploads.

use yowie_core::{Color, uvec2, vec2};
use yowie_tools::encode::{EncodeOptions, encode_image};
use yowie_tools::format::BlockFormat;

use crate::gpu_tests::{assert_px, headless, render};
use crate::{Graphics, ImageData, Sprite, Texture, TextureOptions};

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> ImageData {
    ImageData::Rgba8 {
        width: w,
        height: h,
        pixels: rgba.repeat((w * h) as usize),
    }
}

fn fill(gfx: &mut Graphics, tex: &Texture, size: f32) -> image::RgbaImage {
    gfx.begin_frame();
    gfx.draw_sprite(tex, &Sprite::at(vec2(0.0, 0.0)).size(vec2(size, size)));
    render(gfx)
}

#[test]
fn replaced_pixels_show_through_existing_clones() {
    let Some(mut gfx) = headless(uvec2(16, 16)) else {
        return;
    };
    let tex = Texture::from_data(
        &gfx,
        &solid(2, 2, [255, 0, 0, 255]),
        TextureOptions::default(),
    )
    .unwrap_or_else(|e| panic!("upload: {e}"));
    let held = tex.clone();
    assert_px(&fill(&mut gfx, &held, 16.0), 8, 8, [255, 0, 0, 255], 0);

    // Same size, then a different size: the texture is the same object.
    for (w, h, rgba) in [(2, 2, [0, 255, 0, 255]), (4, 2, [0, 0, 255, 255])] {
        held.replace_data(&gfx, &solid(w, h, rgba))
            .unwrap_or_else(|e| panic!("replace: {e}"));
        assert_eq!(tex.size(), uvec2(w, h));
        assert_eq!(tex, held);
        assert_px(&fill(&mut gfx, &tex, 16.0), 8, 8, rgba, 0);
    }
}

#[test]
fn failed_replace_keeps_the_old_image() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let tex = Texture::from_data(
        &gfx,
        &solid(1, 1, [9, 99, 199, 255]),
        TextureOptions::default(),
    )
    .unwrap_or_else(|e| panic!("upload: {e}"));
    let bad = ImageData::Rgba8 {
        width: 2,
        height: 2,
        pixels: vec![0; 3],
    };
    assert!(tex.replace_data(&gfx, &bad).is_err());
    assert_px(&fill(&mut gfx, &tex, 8.0), 4, 4, [9, 99, 199, 255], 0);
}

#[test]
fn hdr_data_is_stored_linear() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let data = ImageData::Rgba32F {
        width: 1,
        height: 1,
        pixels: vec![0.5, 0.25, 1.0, 1.0],
    };
    let tex = Texture::from_data(&gfx, &data, TextureOptions::default())
        .unwrap_or_else(|e| panic!("upload: {e}"));
    // Linear 0.5 / 0.25 / 1.0 written to an sRGB target.
    assert_px(&fill(&mut gfx, &tex, 8.0), 4, 4, [188, 137, 255, 255], 2);
}

#[test]
fn bc7_upload_matches_the_source_colour() {
    let Some(mut gfx) = headless(uvec2(16, 16)) else {
        return;
    };
    if !gfx.supports_bc() {
        println!("skipping BC7 test: the adapter has no BC support");
        return;
    }
    let src = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 120, 40, 255]));
    let mut opts = EncodeOptions::new(BlockFormat::Bc7);
    opts.fast = true;
    let file = encode_image(&src, &opts).unwrap_or_else(|e| panic!("encode: {e}"));
    assert!(file.levels.len() > 1);
    let tex = Texture::from_data(
        &gfx,
        &ImageData::Compressed(file),
        TextureOptions::default(),
    )
    .unwrap_or_else(|e| panic!("upload: {e}"));
    assert_eq!(tex.size(), uvec2(16, 16));
    assert_px(&fill(&mut gfx, &tex, 16.0), 8, 8, [200, 120, 40, 255], 3);
    // A size that is not a multiple of the block size is refused.
    let odd =
        encode_image(&image::RgbaImage::new(6, 6), &opts).unwrap_or_else(|e| panic!("encode: {e}"));
    assert!(
        Texture::from_data(&gfx, &ImageData::Compressed(odd), TextureOptions::default()).is_err()
    );
}

#[test]
fn shaders_reload_in_place() {
    let Some(mut gfx) = headless(uvec2(8, 8)) else {
        return;
    };
    let src = |c: &str| format!("fn fragment(input: SpriteInput) -> vec4<f32> {{ return {c}; }}");
    let shader = gfx
        .compile_shader(&src("vec4<f32>(1.0, 0.0, 0.0, 1.0)"))
        .unwrap_or_else(|e| panic!("compile: {e}"));
    let tex = Texture::from_data(&gfx, &solid(1, 1, [255; 4]), TextureOptions::default())
        .unwrap_or_else(|e| panic!("upload: {e}"));
    let draw = |gfx: &mut Graphics| {
        gfx.begin_frame();
        gfx.set_shader(Some(&shader));
        gfx.draw_sprite(&tex, &Sprite::at(vec2(0.0, 0.0)).size(vec2(8.0, 8.0)));
        gfx.set_shader(None);
        render(gfx)
    };
    assert_px(&draw(&mut gfx), 4, 4, [255, 0, 0, 255], 1);
    shader
        .reload_source(&gfx, &src("vec4<f32>(0.0, 1.0, 0.0, 1.0)"))
        .unwrap_or_else(|e| panic!("reload: {e}"));
    assert_px(&draw(&mut gfx), 4, 4, [0, 255, 0, 255], 1);
    // A broken edit keeps the working program.
    assert!(shader.reload_source(&gfx, "fn fragment(").is_err());
    assert_px(&draw(&mut gfx), 4, 4, [0, 255, 0, 255], 1);
    let _ = Color::WHITE;
}
