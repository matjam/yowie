//! Renders terrain, water, billboards, impostors and labels headless and
//! checks the pixels. Every test skips with a printed message when no GPU
//! adapter exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Vec2, Vec3, uvec2, vec2};

use super::*;
use crate::gpu_tests::render;
use crate::{Graphics, TextStyle};

/// A headless renderer whose GPU validation errors fail the test.
fn headless(size: yowie_core::UVec2) -> Option<Graphics> {
    let gfx = crate::gpu_tests::headless(size)?;
    gfx.device()
        .on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            panic!("GPU error: {e}");
        }));
    Some(gfx)
}

/// Renders the frame and, when `YOWIE_TEST_DUMP` names a directory, saves
/// it there as `<name>.png` for inspection.
fn shot(gfx: &mut Graphics, name: &str) -> RgbaImage {
    let img = render(gfx);
    if let Ok(dir) = std::env::var("YOWIE_TEST_DUMP") {
        let _ = img.save(std::path::Path::new(&dir).join(format!("{name}.png")));
    }
    img
}

const MAGENTA: Color = Color::rgb(1.0, 0.0, 1.0);

fn is_magenta(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] > 200
}

fn lumpy(cols: u32) -> HeightMap {
    HeightMap::from_fn(cols, cols, |x, z| {
        let (x, z) = (x as f32, z as f32);
        3.0 * (x * 0.31).sin() * (z * 0.23).cos() + 1.5 * (x * 0.77 + z * 0.53).sin()
    })
}

fn lit_scene(s: &mut Scene) {
    s.set_sun(DirectionalLight::new(
        Vec3::new(-0.4, -1.0, -0.3),
        Color::WHITE,
        3.0,
    ));
    s.set_ambient(Color::rgb(0.5, 0.5, 0.5));
    s.set_background(false);
}

#[test]
fn terrain_levels_follow_distance() {
    let Some(mut gfx) = headless(uvec2(160, 100)) else {
        return;
    };
    let options = TerrainOptions {
        lod_distance: 20.0,
        ..TerrainOptions::default()
    };
    let terrain = Terrain::new(&gfx, lumpy(129), options).expect("terrain");
    assert_eq!(terrain.chunk_count(), 16);
    // The camera stands over the -X, -Z corner chunk looking across the
    // terrain towards the far corner.
    let cam = Camera3D::perspective(
        Vec3::new(-56.0, 12.0, -56.0),
        Vec3::new(30.0, 0.0, 30.0),
        1.0,
    );
    let rocks = Lod::new(Mesh::new(&gfx, &MeshData::sphere(1.0, 12, 24)).expect("mesh"))
        .then(Mesh::new(&gfx, &MeshData::cube(1.5)).expect("mesh"), 0.05);
    gfx.scene(&cam, |s| {
        lit_scene(s);
        s.draw_terrain(&terrain);
        let grey = Material::color(Color::GRAY, 0.0, 0.8);
        s.draw_lod(
            &rocks,
            &grey,
            Vec3::new(-52.0, 6.0, -52.0),
            &mut LodState::default(),
        );
        s.draw_lod(
            &rocks,
            &grey,
            Vec3::new(50.0, 4.0, 50.0),
            &mut LodState::default(),
        );
    });
    let img = shot(&mut gfx, "levels");
    let stats = gfx.stats();
    let near = terrain.chunk_level(0).expect("chunk");
    let far = terrain.chunk_level(15).expect("chunk");
    assert_eq!(near, 0, "the chunk under the camera is the finest");
    assert_eq!(far, 3, "the far corner chunk is the coarsest");
    let mid = terrain.chunk_level(5).expect("chunk");
    assert!(mid > near && mid < far, "a middle chunk is between: {mid}");
    assert!(stats.terrain_chunks > 0, "{stats:?}");
    assert_eq!(stats.terrain_chunks + stats.terrain_chunks_culled, 16);
    assert_eq!(stats.lod_levels[0] + stats.lod_levels[1], 2, "{stats:?}");
    assert_eq!(stats.lod_levels[1], 1, "the far rock is coarse: {stats:?}");
    // The lower half of the view is lit ground.
    let p = img.get_pixel(80, 90).0;
    assert!(p[0] > 20 || p[1] > 20, "ground at the bottom: {p:?}");
}

#[test]
fn no_background_shows_through_seams_between_levels() {
    let Some(mut gfx) = headless(uvec2(256, 160)) else {
        return;
    };
    gfx.set_clear_color(MAGENTA);
    gfx.begin_frame();
    // Steep, busy ground with levels changing every few units makes the
    // cracks between levels as wide as they get.
    let map = HeightMap::from_fn(129, 129, |x, z| {
        let (x, z) = (x as f32, z as f32);
        1.5 * (x * 0.9).sin() * (z * 0.7).cos() + 1.2 * (x * 0.37 + z * 1.3).sin()
    });
    let options = TerrainOptions {
        lod_distance: 8.0,
        hysteresis: 0.0,
        chunk_size: 16,
        ..TerrainOptions::default()
    };
    let terrain = Terrain::new(&gfx, map, options).expect("terrain");
    // Each camera looks down steeply enough that the terrain fills the
    // view, from the finest level underneath to the coarsest far away.
    for (i, (eye, target)) in [
        (Vec3::new(0.0, 10.0, 0.0), Vec3::new(10.0, -4.0, 10.0)),
        (Vec3::new(20.0, 9.0, -10.0), Vec3::new(12.0, -4.0, -2.0)),
        (Vec3::new(-30.0, 11.0, 30.0), Vec3::new(-20.0, -4.0, 20.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let cam = Camera3D::perspective(eye, target, 0.9);
        gfx.scene(&cam, |s| {
            lit_scene(s);
            s.draw_terrain(&terrain);
        });
        let img = shot(&mut gfx, &format!("seams{i}"));
        let levels: std::collections::HashSet<u32> = (0..terrain.chunk_count())
            .filter_map(|i| terrain.chunk_level(i))
            .collect();
        assert!(levels.len() >= 3, "the view mixes levels: {levels:?}");
        // Every pixel is ground: the camera looks down into the middle of
        // the terrain, so any magenta pixel is a crack.
        let holes = img.pixels().filter(|p| is_magenta(p.0)).count();
        assert_eq!(holes, 0, "background through seams from {eye:?}");
    }
}

#[test]
fn digging_lowers_the_height_and_rebuilds_the_chunk() {
    let Some(mut gfx) = headless(uvec2(96, 96)) else {
        return;
    };
    let options = TerrainOptions {
        rules: None,
        ..TerrainOptions::default()
    };
    let terrain = Terrain::new(&gfx, HeightMap::flat(65, 65, 1.0), options).expect("terrain");
    let at = Vec3::new(4.0, 0.0, -3.0);
    assert!((terrain.height(at.x, at.z) - 1.0).abs() < 1e-5);
    let cam = Camera3D::orthographic(Vec3::new(4.0, 20.0, -2.0), at, 6.0);
    let draw = |gfx: &mut Graphics| {
        gfx.scene(&cam, |s| {
            lit_scene(s);
            s.draw_terrain(&terrain);
        });
        shot(gfx, "dig")
    };
    let before = draw(&mut gfx);
    let chunk_min = terrain.bounds().min.y;
    assert!(terrain.apply_brush(&gfx, &Brush::dig(at, 4.0, 3.0)));
    assert!(
        terrain.height(at.x, at.z) < -1.5,
        "{}",
        terrain.height(at.x, at.z)
    );
    assert!(
        (terrain.height(at.x + 10.0, at.z) - 1.0).abs() < 1e-5,
        "outside the brush"
    );
    assert!(
        terrain.bounds().min.y < chunk_min,
        "the chunk box grew down"
    );
    let n = terrain.normal(at.x + 2.0, at.z);
    assert!(n.x < -0.2, "the crater wall leans in: {n:?}");
    let ray = Ray::new(Vec3::new(at.x, 10.0, at.z), Vec3::NEG_Y);
    let hit = terrain.raycast(&ray, 100.0).expect("hit");
    assert!(hit.point.y < -1.5, "{hit:?}");
    let after = draw(&mut gfx);
    // The crater's walls face away from flat ground, so they shade
    // differently from what was there.
    let s = cam
        .world_to_screen(at + Vec3::new(2.0, 0.0, 0.0), vec2(96.0, 96.0))
        .expect("in view");
    let (x, y) = (s.x as u32, s.y as u32);
    let (a, b) = (before.get_pixel(x, y).0, after.get_pixel(x, y).0);
    let diff: i32 = (0..3)
        .map(|c| (i32::from(a[c]) - i32::from(b[c])).abs())
        .sum();
    assert!(diff > 20, "the crater shows: {a:?} {b:?}");
}

/// The bounding box of pixels matching `f`, and how many matched.
fn bbox(img: &RgbaImage, f: impl Fn([u8; 4]) -> bool) -> Option<(u32, u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (u32::MAX, u32::MAX, 0, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if f(p.0) {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            n += 1;
        }
    }
    (n > 0).then_some((x0, y0, x1, y1, n))
}

fn white(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] > 200 && p[2] > 200
}

#[test]
fn billboard_faces_the_camera() {
    let Some(mut gfx) = headless(uvec2(200, 200)) else {
        return;
    };
    let unlit = BillboardStyle {
        lit: false,
        alpha: AlphaMode::Opaque,
        ..BillboardStyle::default()
    };
    // From an oblique corner, a camera-facing quad is still a square
    // filling its box.
    let cam = Camera3D::perspective(Vec3::new(6.0, 4.0, 5.0), Vec3::ZERO, 0.8);
    gfx.scene(&cam, |s| {
        s.set_background(false);
        s.draw_billboard(&unlit, Billboard::new(Vec3::ZERO, Vec2::splat(2.0)));
    });
    let img = shot(&mut gfx, "billboard_camera");
    let (x0, y0, x1, y1, n) = bbox(&img, white).expect("drawn");
    let (w, h) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
    let d = Vec3::new(6.0, 4.0, 5.0).length();
    let want = 2.0 / (d * (0.4f32).tan()) * 100.0;
    assert!(
        (w - want).abs() < 3.0 && (h - want).abs() < 3.0,
        "{w}x{h}, want {want}"
    );
    assert!(
        n as f32 > w * h * 0.95,
        "the quad fills its box: {n} of {}",
        w * h
    );
    // Seen from 45 degrees above through an orthographic lens, an upright
    // quad keeps its full width and vertical sides but its height shrinks
    // by cos 45; a camera-facing quad stays square.
    let cam = Camera3D::orthographic(Vec3::new(1.0, 8.0, 8.0), Vec3::new(1.0, 0.0, 0.0), 3.0);
    for (facing, aspect) in [
        (Facing::Upright, std::f32::consts::FRAC_1_SQRT_2),
        (Facing::Camera, 1.0),
    ] {
        gfx.scene(&cam, |s| {
            s.set_background(false);
            let mut b = Billboard::upright(Vec3::new(1.0, -0.7, 0.0), Vec2::splat(2.0));
            b.facing = facing;
            s.draw_billboard(&unlit, b);
        });
        let img = shot(&mut gfx, &format!("billboard_{facing:?}"));
        let (x0, y0, x1, y1, n) = bbox(&img, white).expect("drawn");
        let (w, h) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        assert!((w - 200.0 / 3.0).abs() < 2.0, "{facing:?}: width {w}");
        assert!((h / w - aspect).abs() < 0.04, "{facing:?}: {w}x{h}");
        assert!(n as f32 > w * h * 0.95, "{facing:?}: sides stay vertical");
    }
}

fn pine(gfx: &Graphics) -> Model {
    let trunk = Mesh::new(gfx, &MeshData::cylinder(0.15, 1.2, 8)).expect("mesh");
    let crown = Mesh::new(gfx, &MeshData::sphere(0.8, 10, 16)).expect("mesh");
    let bark = Material::color(Color::rgb8(160, 120, 80), 0.0, 0.9);
    let leaves = Material::color(Color::rgb8(120, 220, 120), 0.0, 0.9);
    Model::from_parts(&[
        (
            trunk,
            bark,
            Mat4::from_translation(Vec3::new(0.0, 0.6, 0.0)),
        ),
        (
            crown,
            leaves,
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.0, 1.6, 0.6),
                yowie_core::Quat::IDENTITY,
                Vec3::new(0.0, 1.9, 0.0),
            ),
        ),
    ])
    .expect("model")
}

fn mask(img: &RgbaImage) -> Vec<bool> {
    img.pixels()
        .map(|p| p.0[0] > 25 || p.0[1] > 25 || p.0[2] > 25)
        .collect()
}

fn iou(a: &[bool], b: &[bool]) -> f32 {
    let inter = a.iter().zip(b).filter(|(x, y)| **x && **y).count();
    let union = a.iter().zip(b).filter(|(x, y)| **x || **y).count();
    inter as f32 / union.max(1) as f32
}

#[test]
fn impostor_silhouette_matches_the_model() {
    let Some(mut gfx) = headless(uvec2(160, 160)) else {
        return;
    };
    let model = pine(&gfx);
    let inst = model.instance();
    let options = ImpostorOptions {
        views: 8,
        resolution: 128,
        ..ImpostorOptions::default()
    };
    let mut imp = Impostor::bake(&mut gfx, &inst, options).expect("bake");
    assert_eq!(imp.views(), 8);
    // Seen along a baked view (from +X turned 90 degrees, at the bake's
    // pitch) from far away with a narrow lens, as the bake saw it.
    let place = Mat4::from_rotation_y(0.3);
    let dir = Vec3::new(
        (std::f32::consts::FRAC_PI_2 + 0.3).sin() * options.pitch.cos(),
        options.pitch.sin(),
        (std::f32::consts::FRAC_PI_2 + 0.3).cos() * options.pitch.cos(),
    );
    let center = Vec3::new(0.0, 1.5, 0.0);
    let cam = Camera3D::perspective(center + dir * 40.0, center, 0.1);
    let draw = |gfx: &mut Graphics, what: u32, imp: &Impostor| {
        gfx.scene(&cam, |s| {
            lit_scene(s);
            s.set_ambient(Color::WHITE);
            match what {
                0 => {
                    s.draw_model(&inst, place);
                }
                1 => s.draw_impostor(imp, place),
                _ => s.draw_model_or_impostor(&inst, imp, place),
            }
        });
        shot(gfx, &format!("impostor{what}"))
    };
    let model_img = draw(&mut gfx, 0, &imp);
    let imp_img = draw(&mut gfx, 1, &imp);
    let (m, i) = (mask(&model_img), mask(&imp_img));
    let covered = m.iter().filter(|v| **v).count();
    assert!(covered > 1500, "the model covers the view: {covered}");
    let score = iou(&m, &i);
    assert!(score > 0.85, "silhouettes overlap: {score}");
    assert_eq!(gfx.stats().impostors, 1);
    // Halfway through the cross-fade the model and the impostor share
    // the pixels between them without holes.
    imp.distance = 40.0;
    imp.fade = 10.0;
    let mixed = draw(&mut gfx, 2, &imp);
    let score = iou(&m, &mask(&mixed));
    assert!(score > 0.85, "cross-fade covers the model: {score}");
}

#[test]
fn label_projects_to_the_right_pixel() {
    let Some(mut gfx) = headless(uvec2(240, 160)) else {
        return;
    };
    let font = gfx.default_font();
    let green = Color::rgb(0.0, 1.0, 0.0);
    let style = LabelStyle {
        text: TextStyle::new(14.0).color(green),
        background: Some(green),
        ..LabelStyle::default()
    };
    let cam = Camera3D::perspective(Vec3::new(3.0, 4.0, 9.0), Vec3::ZERO, 0.9);
    let point = Vec3::new(-1.5, 0.5, 1.0);
    gfx.scene(&cam, |s| {
        s.set_background(false);
        s.label(&font, point, "Camp", &style);
        // Behind the camera: not drawn.
        s.label(&font, Vec3::new(6.0, 8.0, 18.0), "Behind", &style);
    });
    let img = shot(&mut gfx, "label");
    let want = cam
        .world_to_screen(point, vec2(240.0, 160.0))
        .expect("in view");
    let (x0, _, x1, y1, _) = bbox(&img, |p| p[1] > 200 && p[0] < 60).expect("drawn");
    let centre = (x0 + x1) as f32 * 0.5;
    assert!(
        (centre - want.x).abs() <= 2.0,
        "centred: {centre} vs {}",
        want.x
    );
    assert!(
        (y1 as f32 - want.y).abs() <= 2.0,
        "bottom on the point: {y1} vs {}",
        want.y
    );
    assert!(x1 - x0 < 100, "only one label: {x0}..{x1}");
}

#[test]
fn water_is_clear_at_the_shore_and_dark_in_the_middle() {
    let Some(mut gfx) = headless(uvec2(128, 128)) else {
        return;
    };
    // A bowl: deep in the middle, rising past the water level at the rim.
    let map = HeightMap::from_fn(65, 65, |x, z| {
        let (dx, dz) = (x as f32 - 32.0, z as f32 - 32.0);
        (dx * dx + dz * dz).sqrt() * 0.25 - 6.0
    });
    let options = TerrainOptions {
        rules: None,
        layers: std::array::from_fn(|_| TerrainLayer::color(Color::rgb8(230, 210, 160), 0.9)),
        ..TerrainOptions::default()
    };
    let terrain = Terrain::new(&gfx, map, options).expect("terrain");
    let water = Water::over_terrain(
        &gfx,
        &terrain,
        WaterOptions {
            size: Vec2::splat(64.0),
            waves: 0.0,
            foam: 0.0,
            ..WaterOptions::default()
        },
    );
    let cam = Camera3D::orthographic(Vec3::new(0.0, 30.0, 0.1), Vec3::ZERO, 20.0);
    gfx.scene(&cam, |s| {
        lit_scene(s);
        s.draw_terrain(&terrain);
        s.draw_water(&water);
    });
    let img = shot(&mut gfx, "water");
    let luma = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
    };
    let (middle, shore) = (luma(64, 64), luma(64 + 44, 64));
    assert!(
        middle + 100 < shore,
        "deep water is darker: {middle} vs {shore}"
    );
    let p = img.get_pixel(64, 64).0;
    assert!(p[2] > p[0], "deep water is blue: {p:?}");
}
