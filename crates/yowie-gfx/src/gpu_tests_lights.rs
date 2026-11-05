//! 2D lights: falloff, shadows, spot cones and mixing with unlit sprites.

use yowie_core::{Color, Rect, uvec2, vec2};

use crate::gpu_tests::{headless, render};
use crate::{Graphics, Light2D, Sprite, Spot2D, Texture, TextureOptions};

const W: u32 = 128;
const H: u32 = 64;

fn grey(gfx: &Graphics) -> Texture {
    match Texture::from_pixels(gfx, 1, 1, &[200, 200, 200, 255], TextureOptions::default()) {
        Ok(t) => t,
        Err(e) => panic!("texture: {e}"),
    }
}

/// Renders a lit grey floor under `lights` and `occluders`.
fn lit_floor(
    gfx: &mut Graphics,
    lights: &[Light2D],
    walls: &[Rect],
) -> image::RgbaImage {
    gfx.begin_frame();
    let floor = grey(gfx);
    gfx.set_lights_2d(Color::BLACK, lights);
    for w in walls {
        gfx.add_occluder_rect(*w);
    }
    gfx.draw_lit(
        &floor,
        None,
        &Sprite::at(vec2(0.0, 0.0)).size(vec2(W as f32, H as f32)),
    );
    render(gfx)
}

fn red(img: &image::RgbaImage, x: u32, y: u32) -> i32 {
    i32::from(img.get_pixel(x, y).0[0])
}

#[test]
fn light_brightens_near_pixels_and_dims_far_ones() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let light = Light2D::point(vec2(30.0, 32.0), 60.0, Color::WHITE);
    let img = lit_floor(&mut gfx, &[light], &[]);
    let (at, mid, far, beyond) = (
        red(&img, 30, 32),
        red(&img, 55, 32),
        red(&img, 80, 32),
        red(&img, 120, 32),
    );
    assert!(at > mid && mid > far, "falloff: {at} {mid} {far}");
    assert!(at > 100, "lit centre is bright: {at}");
    assert_eq!(beyond, 0, "outside the radius only ambient (black) remains");
}

#[test]
fn an_occluder_darkens_behind_and_not_in_front() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let light = Light2D::point(vec2(20.0, 32.0), 100.0, Color::WHITE).shadows(true);
    let open = lit_floor(&mut gfx, &[light], &[]);
    let wall = Rect::new(50.0, 12.0, 6.0, 40.0);
    let blocked = lit_floor(&mut gfx, &[light], &[wall]);
    let (front, behind) = ((40, 32), (80, 32));
    assert_eq!(
        red(&open, front.0, front.1),
        red(&blocked, front.0, front.1),
        "in front of the wall nothing changes"
    );
    let before = red(&open, behind.0, behind.1);
    let after = red(&blocked, behind.0, behind.1);
    assert!(before > 20, "open floor is lit behind the wall: {before}");
    assert!(after < 4, "shadowed floor is dark: {after}");
    // Past the wall's end the light still arrives.
    assert!(red(&blocked, 60, 62) > 8, "light passes below the wall");
}

#[test]
fn a_spot_light_lights_only_its_cone() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let cone = Spot2D {
        direction: 0.0,
        angle: 0.8,
        softness: 0.2,
    };
    let light = Light2D::spot(vec2(10.0, 32.0), 110.0, Color::WHITE, cone);
    let img = lit_floor(&mut gfx, &[light], &[]);
    assert!(red(&img, 60, 32) > 20, "inside the cone");
    assert_eq!(red(&img, 30, 60), 0, "outside the cone");
}

#[test]
fn lit_and_unlit_sprites_mix_in_one_frame() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    gfx.begin_frame();
    let floor = grey(&gfx);
    gfx.set_lights_2d(Color::BLACK, &[]);
    gfx.draw_lit(&floor, None, &Sprite::at(vec2(0.0, 0.0)).size(vec2(64.0, 64.0)));
    gfx.draw_sprite(&floor, &Sprite::at(vec2(64.0, 0.0)).size(vec2(64.0, 64.0)));
    let img = render(&mut gfx);
    assert_eq!(red(&img, 20, 20), 0, "lit sprite under no light is black");
    assert!(red(&img, 90, 20) > 150, "unlit sprite is drawn as it is");
}
