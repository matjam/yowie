//! Renders scenes through the post chain headless and checks the pixels.
//! Every test skips with a printed message when no GPU adapter exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Quat, Vec3, uvec2};

use crate::Graphics;
use crate::gpu_tests::{headless, render};
use crate::scene::{Camera3D, Material, Mesh, MeshData, PostSettings, Scene, Tonemap};

fn mesh(gfx: &Graphics, data: &MeshData) -> Mesh {
    Mesh::new(gfx, data).expect("mesh")
}

fn srgb_encode(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn dump(img: &RgbaImage, name: &str) {
    if let Ok(dir) = std::env::var("YOWIE_TEST_DUMP") {
        let _ = img.save(format!("{dir}/{name}.png"));
    }
}

/// Linear luminance of a pixel, 0 to 1.
fn lum(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let p = img.get_pixel(x, y).0;
    0.2126 * srgb_decode(p[0]) + 0.7152 * srgb_decode(p[1]) + 0.0722 * srgb_decode(p[2])
}

fn unlit(c: Color) -> Material {
    Material {
        unlit: true,
        ..Material::color(c, 0.0, 1.0)
    }
}

/// No ambient light, no sun, a plain tone curve and the given settings.
fn dark(s: &mut Scene, post: &PostSettings) {
    s.set_ambient(Color::BLACK);
    s.set_post(PostSettings {
        tonemap: Tonemap::None,
        ..post.clone()
    });
}

fn front() -> Camera3D {
    Camera3D::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 0.8)
}

/// A white square (a cube's front face) turned a little about the view
/// axis on black, through `post`, over `frames` frames.
fn slanted_square(gfx: &mut Graphics, post: &PostSettings, frames: u32) -> RgbaImage {
    let cube = mesh(gfx, &MeshData::cube(2.0));
    let mut img = RgbaImage::new(1, 1);
    for _ in 0..frames {
        gfx.scene(&front(), |s| {
            dark(s, post);
            s.draw(&cube, &unlit(Color::WHITE), Mat4::from_rotation_z(0.2));
        });
        img = render(gfx);
    }
    img
}

/// Full-pixel jumps of the square's right edge from one row to the next
/// over the middle rows: each row's edge position is the sum of its
/// linear coverage across the edge, so an aliased edge jumps by a whole
/// pixel every few rows and a smoothed one moves a fraction each row.
fn edge_steps(img: &RgbaImage) -> u32 {
    let (w, h) = img.dimensions();
    let edge = |y: u32| -> f32 { (w / 2..w).map(|x| lum(img, x, y)).sum() };
    let mut steps = 0;
    for y in h * 3 / 8..h * 5 / 8 {
        if (edge(y + 1) - edge(y)).abs() > 0.75 {
            steps += 1;
        }
    }
    steps
}

#[test]
fn default_settings_reproduce_the_plain_resolve() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(2.0));
    let grey = Color::rgb(0.45, 0.45, 0.45);
    let linear = grey.to_linear_premultiplied()[0];
    for tonemap in [
        Tonemap::None,
        Tonemap::Aces,
        Tonemap::Agx,
        Tonemap::Reinhard,
        Tonemap::Neutral,
    ] {
        gfx.scene(&front(), |s| {
            s.set_post(PostSettings {
                exposure: 1.5,
                tonemap,
                ..PostSettings::default()
            });
            s.draw(&cube, &unlit(grey), Vec3::ZERO);
        });
        let img = render(&mut gfx);
        let want = tonemap.apply(Vec3::splat(linear * 1.5));
        let want = (srgb_encode(want.x.clamp(0.0, 1.0)) * 255.0).round() as i32;
        let got = i32::from(img.get_pixel(32, 32).0[0]);
        assert!(
            (got - want).abs() <= 1,
            "{tonemap:?}: got {got}, want {want}"
        );
    }
}

#[test]
fn bloom_spreads_a_bright_pixel() {
    let Some(mut gfx) = headless(uvec2(128, 128)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(0.3));
    let mut draw = |bloom: f32| {
        gfx.scene(&front(), |s| {
            dark(
                s,
                &PostSettings {
                    bloom,
                    ..PostSettings::default()
                },
            );
            s.draw(&cube, &Material::glowing(Color::WHITE, 40.0), Vec3::ZERO);
        });
        render(&mut gfx)
    };
    let (off, on) = (draw(0.0), draw(1.0));
    dump(&off, "bloom_off");
    dump(&on, "bloom_on");
    let (near_off, near_on) = (lum(&off, 64 + 14, 64), lum(&on, 64 + 14, 64));
    assert!(near_off < 0.01, "no glow without bloom: {near_off}");
    assert!(near_on > near_off + 0.02, "bloom reaches out: {near_on}");
    assert!(lum(&on, 2, 2) < near_on, "and fades with distance");
}

#[test]
fn ambient_occlusion_darkens_a_crease() {
    let Some(mut gfx) = headless(uvec2(160, 120)) else {
        return;
    };
    let floor = mesh(&gfx, &MeshData::plane(20.0, 1));
    let cube = mesh(&gfx, &MeshData::cube(2.0));
    let cam = Camera3D::perspective(Vec3::new(0.0, 2.5, 6.0), Vec3::new(0.0, 0.5, 0.0), 0.9);
    let mut draw = |ambient_occlusion: f32| {
        gfx.scene(&cam, |s| {
            s.set_ambient(Color::WHITE);
            s.set_post(PostSettings {
                tonemap: Tonemap::None,
                ambient_occlusion,
                occlusion_radius: 1.0,
                ..PostSettings::default()
            });
            let white = Material::color(Color::rgb(0.5, 0.5, 0.5), 0.0, 1.0);
            s.draw(&floor, &white, Vec3::ZERO);
            s.draw(&cube, &white, Vec3::new(0.0, 1.0, 0.0));
        });
        render(&mut gfx)
    };
    let (off, on) = (draw(0.0), draw(1.0));
    dump(&off, "ao_off");
    dump(&on, "ao_on");
    // Flat ambient light makes the cube's front and the floor one shade;
    // occlusion darkens the rows where they meet.
    let darkest = |img: &RgbaImage| (40..120).map(|y| lum(img, 80, y)).fold(1.0, f32::min);
    let (crease_off, crease_on) = (darkest(&off), darkest(&on));
    assert!(
        crease_on < crease_off * 0.8,
        "crease darkens: {crease_off} -> {crease_on}"
    );
    let (open_off, open_on) = (lum(&off, 4, 116), lum(&on, 4, 116));
    assert!(
        open_on > open_off * 0.9,
        "open floor keeps its light: {open_off} -> {open_on}"
    );
}

#[test]
fn reflections_show_a_bright_object_in_a_mirror_floor() {
    let Some(mut gfx) = headless(uvec2(160, 120)) else {
        return;
    };
    let floor = mesh(&gfx, &MeshData::plane(40.0, 1));
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let cam = Camera3D::perspective(Vec3::new(0.0, 1.5, 6.0), Vec3::new(0.0, 0.3, 0.0), 0.9);
    let mut draw = |reflections: f32| {
        gfx.scene(&cam, |s| {
            dark(
                s,
                &PostSettings {
                    reflections,
                    ..PostSettings::default()
                },
            );
            s.draw(
                &floor,
                &Material::color(Color::WHITE, 1.0, 0.05),
                Vec3::ZERO,
            );
            s.draw(
                &cube,
                &Material::glowing(Color::WHITE, 1.0),
                Vec3::new(0.0, 0.5, 0.0),
            );
        });
        render(&mut gfx)
    };
    let (off, on) = (draw(0.0), draw(1.0));
    // Below the cube's base in the middle column the floor is black
    // without reflections; with them the cube shows there.
    let base = (0..120)
        .rev()
        .find(|&y| lum(&off, 80, y) > 0.5)
        .expect("the cube");
    let below: Vec<u32> = (base + 2..120)
        .filter(|&y| lum(&off, 80, y) < 0.02)
        .collect();
    assert!(!below.is_empty(), "the floor shows below the cube");
    let best = below.iter().map(|&y| lum(&on, 80, y)).fold(0.0, f32::max);
    assert!(best > 0.3, "the mirror reflects the cube: {best}");
}

#[test]
fn fxaa_smooths_an_aliased_edge() {
    let Some(mut gfx) = headless(uvec2(128, 128)) else {
        return;
    };
    let plain = edge_steps(&slanted_square(&mut gfx, &PostSettings::default(), 1));
    let fxaa = PostSettings {
        fxaa: true,
        ..PostSettings::default()
    };
    let smoothed = edge_steps(&slanted_square(&mut gfx, &fxaa, 1));
    assert!(plain >= 4, "the plain edge is aliased: {plain} steps");
    assert!(smoothed < plain / 2, "fxaa: {plain} -> {smoothed} steps");
}

#[test]
fn temporal_aa_smooths_an_aliased_edge() {
    let Some(mut gfx) = headless(uvec2(128, 128)) else {
        return;
    };
    let plain = edge_steps(&slanted_square(&mut gfx, &PostSettings::default(), 1));
    let taa = PostSettings {
        temporal_aa: true,
        ..PostSettings::default()
    };
    let smoothed = edge_steps(&slanted_square(&mut gfx, &taa, 24));
    assert!(plain >= 4, "the plain edge is aliased: {plain} steps");
    assert!(smoothed < plain / 2, "taa: {plain} -> {smoothed} steps");
}

#[test]
fn msaa_smooths_an_aliased_edge() {
    let Some(mut gfx) = headless(uvec2(128, 128)) else {
        return;
    };
    let counts = crate::scene::targets::sample_counts(&gfx.shared.gpu.adapter, gfx.shared.device());
    if !counts.contains(&4) {
        println!("skipping: no 4x multisampling for the scene formats");
        return;
    }
    let plain = edge_steps(&slanted_square(&mut gfx, &PostSettings::default(), 1));
    let msaa = PostSettings {
        samples: 4,
        ..PostSettings::default()
    };
    let smoothed = edge_steps(&slanted_square(&mut gfx, &msaa, 1));
    assert!(plain >= 4, "the plain edge is aliased: {plain} steps");
    assert!(smoothed < plain / 2, "msaa: {plain} -> {smoothed} steps");
}

/// Pixels of a row between 10% and 90% of the way from `dark` to the
/// brightest pixel in `xs`.
fn transition_width(img: &RgbaImage, y: u32, xs: std::ops::Range<u32>) -> usize {
    let top = xs.clone().map(|x| lum(img, x, y)).fold(0.0, f32::max);
    xs.filter(|&x| {
        let l = lum(img, x, y) / top.max(1e-4);
        l > 0.1 && l < 0.9
    })
    .count()
}

#[test]
fn depth_of_field_blurs_only_what_is_out_of_focus() {
    let Some(mut gfx) = headless(uvec2(160, 120)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let cam = Camera3D::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 0.0), 0.9);
    let mut draw = |focus_distance: f32| {
        gfx.scene(&cam, |s| {
            dark(
                s,
                &PostSettings {
                    focus_distance,
                    focus_range: 1.0,
                    bokeh_radius: 16.0,
                    ..PostSettings::default()
                },
            );
            // In focus on the left; far behind on the right.
            s.draw(&cube, &unlit(Color::WHITE), Vec3::new(-1.2, 0.0, 0.0));
            s.draw(
                &cube,
                &unlit(Color::WHITE),
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(6.0),
                    Quat::IDENTITY,
                    Vec3::new(14.0, 0.0, -40.0),
                ),
            );
        });
        render(&mut gfx)
    };
    let (sharp, focused) = (draw(0.0), draw(5.0));
    let near = 0..80;
    let far = 80..160;
    let near_sharp = transition_width(&sharp, 60, near.clone());
    let near_focused = transition_width(&focused, 60, near);
    let far_sharp = transition_width(&sharp, 60, far.clone());
    let far_focused = transition_width(&focused, 60, far);
    assert!(
        near_focused <= near_sharp + 1,
        "in focus stays sharp: {near_sharp} -> {near_focused}"
    );
    assert!(
        far_focused >= far_sharp + 3,
        "out of focus blurs: {far_sharp} -> {far_focused}"
    );
}

#[test]
fn motion_blur_smears_a_moved_object() {
    let Some(mut gfx) = headless(uvec2(128, 96)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let mut draw = |motion_blur: f32| {
        gfx.scene(&front(), |s| {
            dark(
                s,
                &PostSettings {
                    motion_blur,
                    ..PostSettings::default()
                },
            );
            s.draw_moved(
                &cube,
                &unlit(Color::WHITE),
                Vec3::ZERO,
                Vec3::new(-1.0, 0.0, 0.0),
            );
        });
        render(&mut gfx)
    };
    let (still, blurred) = (draw(0.0), draw(1.0));
    // The cube's pixels average along its motion, so its edges fade into
    // the background; its middle, wider than the motion, stays white.
    let left = (0..64)
        .find(|&x| lum(&still, x, 48) > 0.5)
        .expect("the cube");
    let edge = lum(&blurred, left + 1, 48);
    assert!(lum(&still, left + 1, 48) > 0.9, "the still cube is solid");
    assert!(edge < 0.7, "the moving cube's edge smears: {edge}");
    assert!(lum(&blurred, 64, 48) > 0.9, "its middle stays solid");
}

#[test]
fn auto_exposure_brightens_a_dim_scene() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    let cube = mesh(&gfx, &MeshData::cube(4.0));
    let mut draw = |auto_exposure: bool| {
        gfx.scene(&front(), |s| {
            dark(
                s,
                &PostSettings {
                    auto_exposure,
                    ..PostSettings::default()
                },
            );
            s.draw(&cube, &unlit(Color::rgb(0.05, 0.05, 0.05)), Vec3::ZERO);
        });
        render(&mut gfx)
    };
    let (manual, auto) = (draw(false), draw(true));
    let (m, a) = (lum(&manual, 32, 32), lum(&auto, 32, 32));
    assert!(
        a > m * 5.0,
        "auto exposure lifts the scene to mid-grey: {m} -> {a}"
    );
    assert!((a - 0.18).abs() < 0.06, "close to mid-grey: {a}");
}

#[test]
fn gpu_timestamps_read_back() {
    let Some(mut gfx) = headless(uvec2(64, 64)) else {
        return;
    };
    if !gfx.gpu_timing_supported() {
        println!("skipping: no timestamp queries");
        return;
    }
    gfx.set_gpu_timing(true);
    let cube = mesh(&gfx, &MeshData::cube(1.0));
    let mut times = None;
    for _ in 0..8 {
        gfx.scene(&front(), |s| {
            s.draw(&cube, &Material::color(Color::WHITE, 0.0, 0.5), Vec3::ZERO);
        });
        render(&mut gfx);
        times = times.or(gfx.stats().gpu);
    }
    let t = times.expect("a frame's times came back");
    assert!(t.total_us() > 0, "{t:?}");
}
