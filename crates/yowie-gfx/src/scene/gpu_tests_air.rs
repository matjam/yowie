//! Renders aerial perspective and valley fog headless and checks the
//! pixels. Every test skips with a printed message when no GPU adapter
//! exists.

use yowie_core::{Color, Vec3, uvec2};

use super::*;
use crate::gpu_tests::{headless, render};

fn sun() -> DirectionalLight {
    DirectionalLight::new(Vec3::new(0.3, -1.0, 0.2), Color::WHITE, 3.0)
}

#[test]
fn valley_fog_fills_low_ground_and_spares_hills() {
    let Some(mut gfx) = headless(uvec2(128, 64)) else {
        return;
    };
    let cube = Mesh::new(&gfx, &MeshData::cube(2.0)).expect("mesh");
    let black = Material::color(Color::BLACK, 0.0, 1.0);
    let fog_color = Color::rgb(0.8, 0.8, 0.8);
    // Two black cubes the same distance from the camera: the left one in
    // the valley, the right one on a hill above the mist.
    let cam = Camera3D::perspective(Vec3::new(0.0, 6.0, 20.0), Vec3::new(0.0, 4.0, 0.0), 0.8);
    gfx.scene(&cam, |s| {
        s.set_sun(sun());
        s.set_ambient(Color::BLACK);
        s.set_fog(Fog::valley(fog_color, 0.3, 1.0, 1.5));
        s.draw(&cube, &black, Vec3::new(-4.0, 0.0, 0.0));
        s.draw(&cube, &black, Vec3::new(4.0, 8.0, 0.0));
    });
    let img = render(&mut gfx);
    let at = |w: Vec3| {
        let p = cam
            .world_to_screen(w, yowie_core::vec2(128.0, 64.0))
            .expect("in view");
        img.get_pixel(p.x as u32, p.y as u32).0[0]
    };
    let (low, high) = (at(Vec3::new(-4.0, 0.0, 1.0)), at(Vec3::new(4.0, 8.0, 1.0)));
    assert!(low > 120, "the valley cube is in the mist: {low}");
    assert!(high < 30, "the hill cube stays clear: {high}");
}

#[test]
fn aerial_perspective_fades_distant_ground_towards_the_sky() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let cube = Mesh::new(&gfx, &MeshData::cube(1.0)).expect("mesh");
    let black = Material::color(Color::BLACK, 0.0, 1.0);
    let cam = Camera3D::perspective(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, -10.0), 0.8);
    let mut blue = [0u8; 2];
    for (i, aerial) in [1.0, 40.0].into_iter().enumerate() {
        gfx.scene(&cam, |s| {
            s.set_sun(sun());
            s.set_ambient(Color::BLACK);
            s.set_background(false);
            s.set_sky(Sky::atmospheric(
                Atmosphere::new(3000.0).with_aerial(aerial),
                Color::rgb8(70, 70, 60),
            ));
            s.draw(
                &cube,
                &black,
                yowie_core::Mat4::from_scale_rotation_translation(
                    Vec3::splat(40.0),
                    yowie_core::Quat::IDENTITY,
                    Vec3::new(0.0, 1.0, -120.0),
                ),
            );
        });
        blue[i] = render(&mut gfx).get_pixel(32, 32).0[2];
    }
    assert!(blue[1] > blue[0] + 40, "aerial scale hazes it: {blue:?}");
}

#[test]
fn aerial_perspective_is_smooth_over_ground_below_the_camera() {
    let Some(mut gfx) = headless(uvec2(96, 64)) else {
        return;
    };
    let ground = Mesh::new(&gfx, &MeshData::plane(1.0, 1)).expect("mesh");
    let grey = Material::color(Color::GRAY, 0.0, 1.0);
    // Looking down at flat ground from above: every pixel's ray leaves
    // the camera downwards, which once cut some rays short at random.
    let cam = Camera3D::perspective(Vec3::new(0.0, 30.0, 0.0), Vec3::new(0.0, 0.0, -200.0), 0.6);
    gfx.scene(&cam, |s| {
        s.set_sun(sun());
        s.set_sky(Sky::atmospheric(
            Atmosphere::new(3000.0).with_aerial(8.0),
            Color::rgb8(70, 70, 60),
        ));
        s.draw(
            &ground,
            &grey,
            yowie_core::Mat4::from_scale(Vec3::new(2000.0, 1.0, 2000.0)),
        );
    });
    let img = render(&mut gfx);
    let mut worst = 0;
    for y in 40..64 {
        for x in 1..96 {
            let (a, b) = (img.get_pixel(x - 1, y).0, img.get_pixel(x, y).0);
            let d = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
            worst = worst.max(d);
        }
    }
    assert!(worst < 12, "neighbouring pixels differ by {worst}");
}
