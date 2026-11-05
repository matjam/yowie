//! Renders the layered material features headless and checks the pixels:
//! clearcoat, sheen, glass, order-independent transparency and decals.
//! Every test skips with a printed message when no GPU adapter exists.

use std::f32::consts::FRAC_PI_2;

use image::RgbaImage;
use yowie_core::{Color, Mat4, Vec3, uvec2, vec2};

use super::*;
use crate::gpu_tests::{headless, render};
use crate::{Graphics, Texture, TextureOptions};

pub(super) const SIZE: u32 = 128;

pub(super) fn luma(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let p = img.get_pixel(x, y).0;
    0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])
}

pub(super) fn rgb(img: &RgbaImage, x: u32, y: u32) -> [f32; 3] {
    let p = img.get_pixel(x, y).0;
    [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
}

pub(super) fn mesh(gfx: &Graphics, data: &MeshData) -> Mesh {
    Mesh::new(gfx, data).expect("mesh")
}

/// No ambient light and a plain tone curve, so pixels follow the lights.
pub(super) fn dark(s: &mut Scene) {
    s.set_ambient(Color::BLACK);
    s.set_tonemap(Tonemap::None);
}

pub(super) fn front_camera() -> Camera3D {
    Camera3D::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 0.8)
}

/// Where `p` lands on a `SIZE` square frame.
pub(super) fn px(cam: &Camera3D, p: Vec3) -> (u32, u32) {
    let s = cam
        .world_to_screen(p, vec2(SIZE as f32, SIZE as f32))
        .expect("visible");
    (s.x as u32, s.y as u32)
}

/// A square of `size` facing the camera (+Z) at `at`.
pub(super) fn facing(at: Vec3) -> Mat4 {
    Mat4::from_translation(at) * Mat4::from_rotation_x(FRAC_PI_2)
}

pub(super) fn unlit(color: Color) -> Material {
    Material {
        unlit: true,
        ..Material::color(color, 0.0, 1.0)
    }
}

/// The brightest luma in the 5 by 5 block around `(x, y)`.
fn peak(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let mut best: f32 = 0.0;
    for dy in 0..5 {
        for dx in 0..5 {
            best = best.max(luma(img, x + dx - 2, y + dy - 2));
        }
    }
    best
}

#[test]
fn clearcoat_adds_a_sharp_second_highlight() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let sphere = mesh(&gfx, &MeshData::sphere(1.5, 32, 64));
    let cam = front_camera();
    let to_light = Vec3::new(0.5, 0.5, 1.0).normalize();
    let mut draw = |coat: bool| {
        gfx.scene(&cam, |s| {
            dark(s);
            s.set_sun(DirectionalLight::new(-to_light, Color::WHITE, 2.0));
            let mut m = Material::color(Color::rgb(0.3, 0.3, 0.3), 0.0, 0.7);
            if coat {
                m.clearcoat = Some(Clearcoat::new(1.0, 0.05));
            }
            s.draw(&sphere, &m, Vec3::ZERO);
        });
        render(&mut gfx)
    };
    let plain = draw(false);
    let coated = draw(true);
    // The mirror point: the normal halfway between the light and the eye.
    let mut n = Vec3::Z;
    for _ in 0..8 {
        n = (to_light + (cam.position - n * 1.5).normalize()).normalize();
    }
    let (hx, hy) = px(&cam, n * 1.5);
    let (a, b) = (peak(&plain, hx, hy), peak(&coated, hx, hy));
    assert!(b > a + 40.0, "coat highlight {b} over base {a}");
    // A little way off the highlight the coat adds almost nothing.
    let (ox, oy) = (hx - 14, hy + 14);
    let (c, d) = (luma(&plain, ox, oy), luma(&coated, ox, oy));
    assert!((d - c).abs() < 12.0, "off highlight {d} vs {c}");
}

#[test]
fn sheen_brightens_grazing_angles() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let sphere = mesh(&gfx, &MeshData::sphere(1.5, 32, 64));
    let cam = front_camera();
    let mut draw = |sheen: bool| {
        gfx.scene(&cam, |s| {
            dark(s);
            s.set_sun(DirectionalLight::new(-Vec3::Z, Color::WHITE, 2.0));
            let mut m = Material::color(Color::rgb(0.08, 0.08, 0.08), 0.0, 0.8);
            if sheen {
                m.sheen = Some(Sheen::new(Color::WHITE, 0.5));
            }
            s.draw(&sphere, &m, Vec3::ZERO);
        });
        render(&mut gfx)
    };
    let plain = draw(false);
    let velvet = draw(true);
    let edge_n = Vec3::new(72f32.to_radians().sin(), 0.0, 72f32.to_radians().cos());
    let (ex, ey) = px(&cam, edge_n * 1.5);
    let (cx, cy) = px(&cam, Vec3::new(0.0, 0.0, 1.5));
    let edge = luma(&velvet, ex, ey) - luma(&plain, ex, ey);
    let centre = luma(&velvet, cx, cy) - luma(&plain, cx, cy);
    assert!(edge > 15.0, "sheen adds {edge} at the edge");
    assert!(edge > centre + 10.0, "edge {edge} centre {centre}");
}

#[test]
fn glass_shows_the_background_tinted_by_absorption() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let back = mesh(&gfx, &MeshData::plane(12.0, 1));
    let cube = mesh(&gfx, &MeshData::cube(1.5));
    let cam = front_camera();
    let mut draw = |tinted: bool| {
        gfx.scene(&cam, |s| {
            dark(s);
            s.draw(
                &back,
                &unlit(Color::WHITE),
                facing(Vec3::new(0.0, 0.0, -3.0)),
            );
            let mut glass = Material::color(Color::WHITE, 0.0, 0.05);
            let mut t = Transmission::glass(1.5);
            if tinted {
                t.attenuation_color = Color::rgb(1.0, 0.3, 0.3);
                t.attenuation_distance = 0.75;
            }
            glass.transmission = Some(t);
            s.draw(&cube, &glass, Vec3::ZERO);
        });
        render(&mut gfx)
    };
    let clear = draw(false);
    let tinted = draw(true);
    let (cx, cy) = (SIZE / 2, SIZE / 2);
    let c = rgb(&clear, cx, cy);
    assert!(
        c[1] > 170.0 && c[2] > 170.0,
        "clear glass shows white: {c:?}"
    );
    let t = rgb(&tinted, cx, cy);
    assert!(t[0] > 120.0, "tinted glass passes red: {t:?}");
    assert!(t[1] < t[0] * 0.5, "and absorbs green: {t:?}");
    let o = rgb(&tinted, 4, cy);
    assert!(o.iter().all(|v| *v > 240.0), "outside stays white: {o:?}");
}

#[test]
fn order_independent_transparency_ignores_submission_order() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let quad = mesh(&gfx, &MeshData::plane(2.0, 1));
    let cam = front_camera();
    let panes = [
        (Color::rgba(1.0, 0.0, 0.0, 0.5), Vec3::new(-0.4, 0.3, 0.0)),
        (Color::rgba(0.0, 1.0, 0.0, 0.5), Vec3::new(0.4, 0.3, -0.5)),
        (Color::rgba(0.0, 0.0, 1.0, 0.5), Vec3::new(0.0, -0.4, -1.0)),
    ];
    let mut draw = |reverse: bool| {
        gfx.scene(&cam, |s| {
            dark(s);
            let mut order: Vec<_> = panes.iter().collect();
            if reverse {
                order.reverse();
            }
            for (color, at) in order {
                let m = Material {
                    alpha_mode: AlphaMode::Blend,
                    ..unlit(*color)
                };
                s.draw(&quad, &m, facing(*at));
            }
        });
        render(&mut gfx)
    };
    let a = draw(false);
    let b = draw(true);
    let worst = a
        .pixels()
        .zip(b.pixels())
        .flat_map(|(p, q)| (0..3).map(move |i| p.0[i].abs_diff(q.0[i])))
        .max()
        .unwrap_or(0);
    assert!(worst <= 2, "orders differ by {worst}");
    // The three panes overlap at the centre, and all show there.
    let c = rgb(&a, SIZE / 2, SIZE / 2);
    assert!(
        c.iter().all(|v| *v > 20.0),
        "overlap mixes all three: {c:?}"
    );
}

#[test]
fn water_joins_order_independent_transparency_by_depth() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let quad = mesh(&gfx, &MeshData::plane(2.0, 1));
    let floor = mesh(&gfx, &MeshData::plane(8.0, 1));
    let water = Water::new(
        &gfx,
        WaterOptions {
            size: yowie_core::Vec2::splat(8.0),
            waves: 0.0,
            foam: 0.0,
            ..WaterOptions::default()
        },
    );
    // From straight above: a red pane at height `y` over water at 0 and
    // a white floor at -2.
    let cam = Camera3D::orthographic(Vec3::new(0.0, 3.0, 0.01), Vec3::ZERO, 3.0);
    let mut draw = |y: f32| {
        gfx.scene(&cam, |s| {
            dark(s);
            s.set_ambient(Color::WHITE);
            s.draw(&floor, &unlit(Color::WHITE), Vec3::new(0.0, -2.0, 0.0));
            let pane = Material {
                alpha_mode: AlphaMode::Blend,
                ..unlit(Color::rgba(1.0, 0.0, 0.0, 0.7))
            };
            s.draw(&quad, &pane, Vec3::new(0.0, y, 0.0));
            s.draw_water(&water);
        });
        render(&mut gfx)
    };
    let above = rgb(&draw(1.0), SIZE / 2, SIZE / 2);
    let below = rgb(&draw(-1.0), SIZE / 2, SIZE / 2);
    // The water accumulates with the pane, so the nearer one weighs more.
    assert!(
        above[0] > below[0] + 30.0,
        "pane above {above:?}, below {below:?}"
    );
}

#[test]
fn decal_colours_only_inside_its_box() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let ground = mesh(&gfx, &MeshData::plane(10.0, 1));
    let red = Texture::from_pixels(
        &gfx,
        4,
        4,
        &[255, 0, 0, 255].repeat(16),
        TextureOptions::default(),
    )
    .expect("texture");
    let decal = Decal::new(red);
    let cam = Camera3D::perspective(Vec3::new(0.0, 6.0, 0.01), Vec3::ZERO, 0.9);
    gfx.scene(&cam, |s| {
        dark(s);
        s.set_sun(DirectionalLight::new(-Vec3::Y, Color::WHITE, 2.0));
        s.draw(
            &ground,
            &Material::color(Color::WHITE, 0.0, 1.0),
            Vec3::ZERO,
        );
        s.draw_decal(&decal, Mat4::from_scale(Vec3::new(2.0, 1.0, 2.0)));
    });
    let img = render(&mut gfx);
    let (ix, iy) = px(&cam, Vec3::new(0.3, 0.0, 0.3));
    let inside = rgb(&img, ix, iy);
    assert!(
        inside[0] > 100.0 && inside[1] < inside[0] * 0.3,
        "{inside:?}"
    );
    for p in [Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -2.0)] {
        let (x, y) = px(&cam, p);
        let out = rgb(&img, x, y);
        assert!(out[0] > 100.0, "outside is lit: {out:?}");
        assert!((out[0] - out[1]).abs() < 8.0, "and not red: {out:?}");
    }
}
