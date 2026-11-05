//! Draws the large-scene classes (terrain, water, cut-out and blended
//! billboards, a model cross-fading with its impostor) under every HDR
//! pass layout the post chain asks for. GPU validation errors fail the
//! tests; every test skips with a printed message when no GPU adapter
//! exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Quat, UVec2, Vec2, Vec3, uvec2};

use crate::Graphics;
use crate::gpu_tests::render;
use crate::scene::*;

const W: u32 = 160;
const H: u32 = 120;

fn headless(size: UVec2) -> Option<Graphics> {
    let gfx = crate::gpu_tests::headless(size)?;
    gfx.device()
        .on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            panic!("GPU error: {e}");
        }));
    Some(gfx)
}

fn dump(img: &RgbaImage, name: &str) {
    if let Ok(dir) = std::env::var("YOWIE_TEST_DUMP") {
        let _ = img.save(format!("{dir}/{name}.png"));
    }
}

/// The mean absolute channel difference of the rows `rows` of two
/// images, 0 to 255.
fn mean_diff(a: &RgbaImage, b: &RgbaImage, rows: std::ops::Range<u32>) -> f32 {
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in rows {
        for x in 0..a.width() {
            let (p, q) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
            for c in 0..3 {
                sum += u64::from(p[c].abs_diff(q[c]));
                n += 1;
            }
        }
    }
    sum as f32 / n.max(1) as f32
}

fn mean_luma(img: &RgbaImage, rows: std::ops::Range<u32>) -> f32 {
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in rows {
        for x in 0..img.width() {
            let p = img.get_pixel(x, y).0;
            sum += u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]);
            n += 3;
        }
    }
    sum as f32 / n.max(1) as f32
}

fn pine(gfx: &Graphics) -> Model {
    let trunk = Mesh::new(gfx, &MeshData::cylinder(0.3, 2.4, 8)).expect("mesh");
    let crown = Mesh::new(gfx, &MeshData::sphere(1.6, 10, 16)).expect("mesh");
    Model::from_parts(&[
        (
            trunk,
            Material::color(Color::rgb8(160, 120, 80), 0.0, 0.9),
            Mat4::from_translation(Vec3::new(0.0, 1.2, 0.0)),
        ),
        (
            crown,
            Material::color(Color::rgb8(120, 220, 120), 0.0, 0.9),
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.0, 1.6, 1.0),
                Quat::IDENTITY,
                Vec3::new(0.0, 3.8, 0.0),
            ),
        ),
    ])
    .expect("model")
}

struct World {
    terrain: Terrain,
    water: Water,
    model: ModelInstance,
    impostor: Impostor,
}

impl World {
    fn new(gfx: &mut Graphics) -> Self {
        // A lumpy bowl with water in the middle.
        let map = HeightMap::from_fn(65, 65, |x, z| {
            let (dx, dz) = (x as f32 - 32.0, z as f32 - 32.0);
            (dx * dx + dz * dz).sqrt() * 0.2 - 3.0
                + 1.5 * (x as f32 * 0.9).sin() * (z as f32 * 0.7).cos()
        });
        let terrain = Terrain::new(gfx, map, TerrainOptions::default()).expect("terrain");
        let water = Water::over_terrain(
            gfx,
            &terrain,
            WaterOptions {
                size: Vec2::splat(64.0),
                ..WaterOptions::default()
            },
        );
        let model = pine(gfx).instance();
        let options = ImpostorOptions {
            views: 8,
            resolution: 64,
            ..ImpostorOptions::default()
        };
        let mut impostor = Impostor::bake(gfx, &model, options).expect("bake");
        // Halfway through the cross-fade at the camera's distance, so the
        // dithered model and the impostor both draw.
        impostor.distance = 26.0;
        impostor.fade = 20.0;
        Self {
            terrain,
            water,
            model,
            impostor,
        }
    }

    fn draw(&self, s: &mut Scene, post: &PostSettings) {
        // Mostly ambient light, which is what occlusion scales.
        s.set_sun(
            DirectionalLight::new(Vec3::new(-0.4, -1.0, -0.3), Color::WHITE, 1.0)
                .with_shadows(60.0),
        );
        s.set_ambient(Color::WHITE);
        s.set_post(post.clone());
        s.draw_terrain(&self.terrain);
        s.draw_water(&self.water);
        let cutout = BillboardStyle::default();
        let blended = BillboardStyle {
            alpha: AlphaMode::Blend,
            ..BillboardStyle::default()
        };
        let mut board = Billboard::upright(Vec3::new(-7.0, 2.0, 6.0), Vec2::new(2.0, 3.0));
        board.color = Color::rgb8(230, 90, 60);
        s.draw_billboard(&cutout, board);
        board.position.x = 7.0;
        board.color = Color::rgb8(60, 120, 230).with_alpha(0.6);
        s.draw_billboard(&blended, board);
        s.draw_model_or_impostor(
            &self.model,
            &self.impostor,
            Mat4::from_translation(Vec3::new(0.0, 1.0, 8.0)),
        );
    }

    fn frame(&self, gfx: &mut Graphics, post: &PostSettings) -> RgbaImage {
        let cam = Camera3D::perspective(Vec3::new(0.0, 16.0, 30.0), Vec3::new(0.0, 0.0, 4.0), 0.9);
        gfx.scene(&cam, |s| self.draw(s, post));
        render(gfx)
    }
}

#[test]
fn large_scene_classes_draw_in_every_post_layout() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let world = World::new(&mut gfx);
    let plain = world.frame(&mut gfx, &PostSettings::default());
    dump(&plain, "large_plain");
    let st = gfx.stats();
    assert!(st.terrain_chunks > 0, "{st:?}");
    assert!(st.impostors > 0, "the impostor draws: {st:?}");
    // The ground below the trees: terrain alone.
    let ground = H * 3 / 4..H;
    let layouts = [
        (
            "occlusion",
            PostSettings {
                ambient_occlusion: 1.0,
                occlusion_radius: 2.0,
                ..PostSettings::default()
            },
        ),
        (
            "reflections",
            PostSettings {
                reflections: 1.0,
                ..PostSettings::default()
            },
        ),
        (
            "msaa",
            PostSettings {
                samples: 4,
                ..PostSettings::default()
            },
        ),
        (
            "everything",
            PostSettings {
                samples: 4,
                ambient_occlusion: 1.0,
                occlusion_radius: 2.0,
                reflections: 1.0,
                temporal_aa: true,
                motion_blur: 0.5,
                ..PostSettings::default()
            },
        ),
    ];
    for (name, post) in &layouts {
        let mut img = world.frame(&mut gfx, post);
        // Temporal anti-aliasing settles over a few frames.
        for _ in 0..4 {
            img = world.frame(&mut gfx, post);
        }
        dump(&img, &format!("large_{name}"));
        let d = mean_diff(&plain, &img, 0..H);
        assert!(d < 12.0, "{name}: the picture stays the same: {d}");
        if post.ambient_occlusion > 0.0 {
            // Terrain writes the surface image, so occlusion darkens the
            // creases of the ground.
            let (lit, dark) = (
                mean_luma(&plain, ground.clone()),
                mean_luma(&img, ground.clone()),
            );
            assert!(
                dark < lit - 0.8,
                "{name}: occlusion darkens the ground: {dark} vs {lit}"
            );
        }
    }
}

#[test]
fn impostor_bakes_ignore_the_post_layout_of_earlier_frames() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let world = World::new(&mut gfx);
    let heavy = PostSettings {
        samples: 4,
        ambient_occlusion: 1.0,
        reflections: 1.0,
        temporal_aa: true,
        ..PostSettings::default()
    };
    for _ in 0..3 {
        world.frame(&mut gfx, &heavy);
    }
    let options = ImpostorOptions {
        views: 8,
        resolution: 64,
        ..ImpostorOptions::default()
    };
    let again = Impostor::bake(&mut gfx, &world.model, options).expect("bake");
    let cam = Camera3D::perspective(Vec3::new(0.0, 2.0, 12.0), Vec3::new(0.0, 2.5, 0.0), 0.9);
    let mut shots = Vec::new();
    for imp in [&world.impostor, &again] {
        gfx.scene(&cam, |s| {
            s.set_ambient(Color::WHITE);
            s.draw_impostor(imp, Mat4::IDENTITY);
        });
        shots.push(render(&mut gfx));
    }
    dump(&shots[0], "impostor_first");
    dump(&shots[1], "impostor_after_post");
    let d = mean_diff(&shots[0], &shots[1], 0..H);
    assert!(d < 0.01, "the bakes match: {d}");
}
