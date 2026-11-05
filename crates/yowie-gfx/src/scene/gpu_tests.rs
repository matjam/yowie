//! Renders 3D scenes headless and checks the pixels. Every test skips
//! with a printed message when no GPU adapter exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Vec3, uvec2, vec2};

use super::*;
use crate::gpu_tests::{headless, render};
use crate::{Graphics, RenderTexture, RenderTextureOptions, Texture, TextureOptions};

fn luma(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let p = img.get_pixel(x, y).0;
    0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])
}

fn mesh(gfx: &Graphics, data: &MeshData) -> Mesh {
    Mesh::new(gfx, data).expect("mesh")
}

/// A scene with no ambient light and a plain tone curve, so pixels follow
/// the lights alone.
fn dark(s: &mut Scene) {
    s.set_ambient(Color::BLACK);
    s.set_tonemap(Tonemap::None);
}

fn front_camera() -> Camera3D {
    Camera3D::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 0.8)
}

#[test]
fn lit_sphere_falls_off_away_from_the_light() {
    let Some(mut gfx) = headless(uvec2(128, 128)) else {
        return;
    };
    let sphere = mesh(&gfx, &MeshData::sphere(1.5, 24, 48));
    gfx.scene(&front_camera(), |s| {
        dark(s);
        s.set_sun(DirectionalLight::new(
            Vec3::new(-1.0, 0.0, 0.0),
            Color::WHITE,
            2.0,
        ));
        s.draw(
            &sphere,
            &Material::color(Color::WHITE, 0.0, 0.8),
            Vec3::ZERO,
        );
    });
    let img = render(&mut gfx);
    let (right, centre, left) = (luma(&img, 100, 64), luma(&img, 64, 64), luma(&img, 30, 64));
    assert!(right > centre + 20.0, "right {right} centre {centre}");
    assert!(centre > left, "centre {centre} left {left}");
    assert!(left < 5.0, "the far side is dark: {left}");
    assert!(luma(&img, 2, 2) < 1.0, "background stays black");
}

#[test]
fn shadow_darkens_the_ground_behind_an_occluder() {
    let Some(mut gfx) = headless(uvec2(160, 120)) else {
        return;
    };
    let ground = mesh(&gfx, &MeshData::plane(20.0, 1));
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let cam = Camera3D::perspective(Vec3::new(0.0, 8.0, 8.0), Vec3::ZERO, 0.9);
    let draw = |gfx: &mut Graphics, shadows: bool| {
        gfx.scene(&cam, |s| {
            dark(s);
            let mut sun = DirectionalLight::new(Vec3::new(0.0, -1.0, 0.0), Color::WHITE, 2.0);
            sun.shadows = shadows;
            sun.shadow_distance = 30.0;
            s.set_sun(sun);
            let white = Material::color(Color::WHITE, 0.0, 1.0);
            s.draw(&ground, &white, Vec3::ZERO);
            s.draw(
                &cube,
                &white,
                Mat4::from_translation(Vec3::new(3.0, 2.0, 0.0)),
            );
        });
        render(gfx)
    };
    let lit = draw(&mut gfx, false);
    let shadowed = draw(&mut gfx, true);
    // The ground under the cube (x = 3) is dark with shadows on, and the
    // ground at x = -3 is lit in both.
    let cam_px = |p: Vec3| {
        let s = cam.world_to_screen(p, vec2(160.0, 120.0)).expect("visible");
        (s.x as u32, s.y as u32)
    };
    let (ux, uy) = cam_px(Vec3::new(3.0, 0.0, 0.0));
    let (ox, oy) = cam_px(Vec3::new(-3.0, 0.0, 0.0));
    assert!(
        luma(&lit, ux, uy) > 100.0,
        "lit ground {}",
        luma(&lit, ux, uy)
    );
    assert!(
        luma(&shadowed, ux, uy) < 10.0,
        "shadowed ground {}",
        luma(&shadowed, ux, uy)
    );
    assert!((luma(&shadowed, ox, oy) - luma(&lit, ox, oy)).abs() < 3.0);
    assert!(gfx.stats().shadow_instances >= 2);
}

#[test]
fn point_light_falls_off_with_distance() {
    let Some(mut gfx) = headless(uvec2(160, 120)) else {
        return;
    };
    let ground = mesh(&gfx, &MeshData::plane(20.0, 1));
    let cam = Camera3D::perspective(Vec3::new(0.0, 10.0, 0.01), Vec3::ZERO, 1.2);
    gfx.scene(&cam, |s| {
        dark(s);
        s.add_point_light(PointLight::new(
            Vec3::new(0.0, 1.0, 0.0),
            Color::WHITE,
            3.0,
            6.0,
        ));
        s.draw(
            &ground,
            &Material::color(Color::WHITE, 0.0, 1.0),
            Vec3::ZERO,
        );
    });
    let img = render(&mut gfx);
    let at = |x: f32| {
        let s = cam
            .world_to_screen(Vec3::new(x, 0.0, 0.0), vec2(160.0, 120.0))
            .expect("visible");
        luma(&img, s.x as u32, s.y as u32)
    };
    let (a, b, c, d) = (at(0.0), at(1.5), at(3.0), at(7.0));
    assert!(a > b && b > c, "falloff {a} {b} {c}");
    assert!(a > 60.0, "{a}");
    assert!(d < 1.0, "beyond the range is dark: {d}");
    assert_eq!(gfx.stats().lights, 1);
}

#[test]
fn alpha_mask_cuts_out() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    // Left half transparent, right half opaque white.
    let mut px = Vec::new();
    for _y in 0..4 {
        for x in 0..4 {
            px.extend_from_slice(&[255, 255, 255, if x < 2 { 0 } else { 255 }]);
        }
    }
    let tex = Texture::from_pixels(&gfx, 4, 4, &px, TextureOptions::default()).expect("texture");
    let quad = mesh(&gfx, &MeshData::plane(4.0, 1));
    let mat = Material {
        base_color: Color::RED,
        base_color_texture: Some(tex),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        double_sided: true,
        ..Material::default()
    };
    // The clear colour applies from the next frame on.
    gfx.set_clear_color(Color::BLUE);
    gfx.begin_frame();
    let cam = Camera3D::orthographic(Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, 2.0)
        .with_depth_range(0.1, 20.0);
    let cam = Camera3D {
        up: Vec3::NEG_Z,
        ..cam
    };
    gfx.scene(&cam, |s| {
        dark(s);
        s.draw(&quad, &mat, Vec3::ZERO);
    });
    let img = render(&mut gfx);
    let left = img.get_pixel(12, 32).0;
    let right = img.get_pixel(52, 32).0;
    assert!(
        left[2] > 200 && left[0] < 30,
        "cut out shows the clear colour: {left:?}"
    );
    assert!(
        right[0] > 200 && right[2] < 30,
        "kept half is red: {right:?}"
    );
}

#[test]
fn skinning_moves_vertices() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let mut data = MeshData::cube(1.0);
    for v in &mut data.vertices {
        v.joints = [0, 1, 0, 0];
        v.weights = [0.0, 1.0, 0.0, 0.0];
    }
    let skinned = mesh(&gfx, &data);
    assert!(skinned.is_skinned());
    let cam = Camera3D::orthographic(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, 4.0);
    let joints = [
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
    ];
    gfx.scene(&cam, |s| {
        dark(s);
        s.draw_skinned(
            &skinned,
            &Material::glowing(Color::WHITE, 1.0),
            Vec3::ZERO,
            &joints,
        );
    });
    let img = render(&mut gfx);
    // The view spans 8 units over 64 pixels: x = 2 is pixel 48.
    assert!(luma(&img, 48, 32) > 200.0, "moved cube drawn");
    assert!(luma(&img, 32, 32) < 5.0, "rest position empty");
}

#[test]
fn identical_draws_become_one_instanced_call() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(0.2));
    let red = Material::color(Color::RED, 0.0, 0.5);
    let blue = Material::color(Color::BLUE, 0.0, 0.5);
    let cam = Camera3D::perspective(Vec3::new(0.0, 0.0, 12.0), Vec3::ZERO, 1.0);
    let models: Vec<Mat4> = (0..50)
        .map(|i| {
            Mat4::from_translation(Vec3::new((i % 10) as f32 - 4.5, (i / 10) as f32 - 2.0, 0.0))
        })
        .collect();
    gfx.scene(&cam, |s| {
        for m in &models {
            s.draw(&cube, &red, *m);
        }
        s.draw_instanced(&cube, &blue, &models);
    });
    gfx.end_frame().expect("frame");
    let st = gfx.stats();
    assert_eq!(st.draws_3d, 100);
    // Two batches (red and blue) and the resolve.
    assert_eq!(st.draw_calls_3d, 3, "{st:?}");
    assert_eq!(st.culled_3d, 0);
}

#[test]
fn frustum_culling_counts_what_is_skipped() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(0.5));
    let mat = Material::default();
    gfx.scene(&front_camera(), |s| {
        for i in 0..10 {
            s.draw(&cube, &mat, Vec3::new(i as f32 * 0.1, 0.0, 0.0));
            s.draw(&cube, &mat, Vec3::new(0.0, 0.0, 20.0 + i as f32));
            s.draw(&cube, &mat, Vec3::new(500.0, 0.0, 0.0));
        }
    });
    gfx.end_frame().expect("frame");
    let st = gfx.stats();
    assert_eq!(st.draws_3d, 30);
    assert_eq!(st.culled_3d, 20, "{st:?}");
}

#[test]
fn picking_returns_the_nearest_object() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let mat = Material::default();
    let cam = front_camera();
    let view = gfx.view_size();
    let (hit, miss, behind) = gfx.scene(&cam, |s| {
        let _left = s.draw(&cube, &mat, Vec3::new(-2.0, 0.0, 0.0));
        let far = s.draw(&cube, &mat, Vec3::new(1.0, 0.0, -3.0));
        let near = s.draw(&cube, &mat, Vec3::new(1.0, 0.0, 0.0));
        let screen = cam
            .world_to_screen(Vec3::new(1.0, 0.0, 0.0), view)
            .expect("visible");
        let hit = s
            .pick(&s.screen_ray(screen))
            .map(|p| (p.draw, p.hit.distance));
        let miss = s.pick(&s.screen_ray(vec2(1.0, 1.0)));
        (hit.map(|h| (h, near)), miss, far)
    });
    let ((id, dist), near) = hit.expect("hit");
    assert_eq!(id, near, "the nearer of the two cubes on the ray");
    assert_ne!(id, behind);
    assert!(dist > 3.0 && dist < 5.0, "{dist}");
    assert!(miss.is_none());
    gfx.end_frame().expect("frame");
}

#[test]
fn a_scene_renders_into_a_render_texture() {
    let Some(mut gfx) = headless(uvec2(32, 32)) else {
        return;
    };
    let rt = RenderTexture::new(&gfx, 48, 48, RenderTextureOptions::default()).expect("rt");
    let sphere = mesh(&gfx, &MeshData::sphere(1.0, 16, 32));
    gfx.draw_to(&rt, Some(Color::BLACK), |g| {
        g.scene(&front_camera(), |s| {
            dark(s);
            s.draw(&sphere, &Material::glowing(Color::GREEN, 1.0), Vec3::ZERO);
        });
    });
    gfx.end_frame().expect("frame");
    let img = rt.read(&gfx).expect("read");
    let centre = img.get_pixel(24, 24).0;
    let corner = img.get_pixel(1, 1).0;
    assert!(centre[1] > 200 && centre[0] < 20, "{centre:?}");
    assert!(corner[1] < 10, "{corner:?}");
    assert_eq!(gfx.stats().outputs, 2);
}

#[test]
fn sky_and_environment_light_the_scene() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let sphere = mesh(&gfx, &MeshData::sphere(1.5, 16, 32));
    // A panorama bright above and dark below lights the top of a sphere.
    let mut px = Vec::new();
    for y in 0..32 {
        for _x in 0..64 {
            px.push(if y < 16 {
                [2.0, 2.0, 2.0]
            } else {
                [0.0, 0.0, 0.0]
            });
        }
    }
    let pano = Panorama::from_pixels(64, 32, px).expect("panorama");
    let env = Environment::new(
        &gfx,
        &pano,
        EnvironmentOptions {
            size: 32,
            intensity: 1.0,
        },
    )
    .expect("environment");
    gfx.scene(&front_camera(), |s| {
        dark(s);
        s.set_environment(Some(&env));
        s.set_background(false);
        s.draw(
            &sphere,
            &Material::color(Color::WHITE, 0.0, 1.0),
            Vec3::ZERO,
        );
    });
    let img = render(&mut gfx);
    let (top, bottom) = (luma(&img, 32, 14), luma(&img, 32, 50));
    assert!(top > bottom + 40.0, "top {top} bottom {bottom}");
    // A procedural sky draws behind the scene and is blue overhead.
    let up = Camera3D::perspective(Vec3::ZERO, Vec3::new(0.0, 1.0, -0.3), 1.0);
    gfx.scene(&up, |s| {
        s.set_sun(DirectionalLight::new(
            Vec3::new(0.3, -1.0, 0.2),
            Color::WHITE,
            3.0,
        ));
        s.set_sky(Sky::atmospheric(
            Atmosphere::new(3000.0),
            Color::rgb8(70, 70, 60),
        ));
    });
    let sky = render(&mut gfx);
    let p = sky.get_pixel(32, 20).0;
    assert!(p[2] > p[0] && p[2] > 60, "blue sky: {p:?}");
}
