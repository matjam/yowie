//! Renders rooms headless and checks that probes carry the room's colours
//! into reflections and diffuse light. Every test skips with a printed
//! message when no GPU adapter exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Quat, UVec3, Vec3, uvec2};

use crate::Graphics;
use crate::gpu_tests::{headless, render};
use crate::scene::*;

const SIZE: u32 = 96;

fn mesh(gfx: &Graphics, data: &MeshData) -> Mesh {
    Mesh::new(gfx, data).expect("mesh")
}

fn px(img: &RgbaImage, x: u32, y: u32) -> [i32; 3] {
    let p = img.get_pixel(x, y).0;
    [i32::from(p[0]), i32::from(p[1]), i32::from(p[2])]
}

/// A blue environment: what a reflection shows where no probe covers it.
fn blue_sky(gfx: &Graphics) -> Environment {
    let pano = Panorama::from_pixels(16, 8, vec![[0.05, 0.1, 1.5]; 128]).expect("panorama");
    let options = EnvironmentOptions {
        size: 16,
        intensity: 1.0,
    };
    Environment::new(gfx, &pano, options).expect("environment")
}

/// A closed room 10 units across: glowing walls, red at -X, green at +X,
/// a dim red-grey everywhere else.
struct Room {
    cube: Mesh,
    sphere: Mesh,
}

impl Room {
    fn new(gfx: &Graphics) -> Self {
        Self {
            cube: mesh(gfx, &MeshData::cube(1.0)),
            sphere: mesh(gfx, &MeshData::sphere(1.0, 24, 48)),
        }
    }

    /// Each wall's material and placement.
    fn pieces(side_glow: f32) -> Vec<(Material, Mat4)> {
        let wall = |c: Color| Material {
            double_sided: true,
            ..Material::glowing(c, side_glow)
        };
        let other = Material {
            double_sided: true,
            ..Material::glowing(Color::rgb(0.5, 0.5, 0.5), 0.6)
        };
        let slab = |p: Vec3, sc: Vec3| Mat4::from_scale_rotation_translation(sc, Quat::IDENTITY, p);
        let mut out = vec![
            (
                wall(Color::RED),
                slab(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(0.2, 10.0, 10.0)),
            ),
            (
                wall(Color::GREEN),
                slab(Vec3::new(5.0, 0.0, 0.0), Vec3::new(0.2, 10.0, 10.0)),
            ),
        ];
        for (p, sc) in [
            (Vec3::new(0.0, 5.0, 0.0), Vec3::new(10.0, 0.2, 10.0)),
            (Vec3::new(0.0, -5.0, 0.0), Vec3::new(10.0, 0.2, 10.0)),
            (Vec3::new(0.0, 0.0, -5.0), Vec3::new(10.0, 10.0, 0.2)),
            (Vec3::new(0.0, 0.0, 5.0), Vec3::new(10.0, 10.0, 0.2)),
        ] {
            out.push((other.clone(), slab(p, sc)));
        }
        out
    }

    fn walls(&self, s: &mut Scene, side_glow: f32) {
        for (m, at) in Self::pieces(side_glow) {
            s.draw(&self.cube, &m, at);
        }
    }

    /// The walls as one static group.
    fn static_walls(&self, side_glow: f32) -> StaticGroup {
        let mut g = StaticGroup::new();
        for (m, at) in Self::pieces(side_glow) {
            g.add(&self.cube, &m, at);
        }
        g
    }
}

fn camera() -> Camera3D {
    Camera3D::perspective(Vec3::new(0.0, 0.0, 3.6), Vec3::ZERO, 0.9)
}

fn probe() -> ReflectionProbe {
    let mut p = ReflectionProbe::new(Vec3::ZERO, Aabb::new(Vec3::splat(-5.0), Vec3::splat(5.0)));
    p.resolution = 64;
    p
}

/// Draws the room with a chrome ball at the centre, lit by `env`, with
/// `probe` shown when given.
fn chrome_scene(
    gfx: &mut Graphics,
    room: &Room,
    env: &Environment,
    probe: Option<&ReflectionProbe>,
) -> RgbaImage {
    gfx.scene(&camera(), |s| {
        s.set_ambient(Color::BLACK);
        s.set_tonemap(Tonemap::None);
        s.set_environment(Some(env));
        s.set_background(false);
        if let Some(p) = probe {
            s.add_reflection_probe(p);
        }
        room.walls(s, 1.0);
        let ball = s.draw(
            &room.sphere,
            &Material::color(Color::WHITE, 1.0, 0.05),
            Vec3::ZERO,
        );
        s.hide_in_probes(ball);
    });
    render(gfx)
}

#[test]
fn chrome_ball_reflects_the_room_through_its_probe() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let room = Room::new(&gfx);
    let env = blue_sky(&gfx);
    let p = probe();
    assert!(!p.is_baked());
    gfx.bake_reflection_probe(&p, |s| {
        s.set_ambient(Color::BLACK);
        s.set_environment(Some(&env));
        room.walls(s, 1.0);
    });
    assert!(p.is_baked());
    let with = chrome_scene(&mut gfx, &room, &env, Some(&p));
    let without = chrome_scene(&mut gfx, &room, &env, None);
    let c = SIZE / 2;
    let (w, wo) = (px(&with, c, c), px(&without, c, c));
    // The ball's middle reflects the grey wall behind the camera, never
    // the blue sky outside the room.
    assert!(w[0] > 40 && (w[2] - w[0]).abs() < 15, "with probe {w:?}");
    assert!(wo[2] > wo[0] + 40, "without probe {wo:?}");
    // Its left rim reflects the red wall at -X, its right rim the green
    // wall at +X: the capture is the right way round.
    let r = (SIZE as f32 * 0.205) as u32;
    let (left, right) = (px(&with, c - r, c), px(&with, c + r, c));
    assert!(left[0] > left[1] + 40, "left rim {left:?}");
    assert!(right[1] > right[0] + 40, "right rim {right:?}");
}

#[test]
fn an_unbaked_probe_is_captured_from_the_scene_that_shows_it() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let room = Room::new(&gfx);
    let env = blue_sky(&gfx);
    let p = probe();
    let first = chrome_scene(&mut gfx, &room, &env, Some(&p));
    assert_eq!(gfx.stats().probe_faces, 6);
    assert!(p.is_baked());
    let c = SIZE / 2;
    let w = px(&first, c, c);
    assert!(w[0] > 40 && (w[2] - w[0]).abs() < 15, "{w:?}");
    chrome_scene(&mut gfx, &room, &env, Some(&p));
    assert_eq!(gfx.stats().probe_faces, 0, "captured once");
    p.request_bake();
    chrome_scene(&mut gfx, &room, &env, Some(&p));
    assert_eq!(gfx.stats().probe_faces, 6, "captured again on request");
}

/// The room around a 3 by 1 by 1 grid, and two matte balls near its ends.
fn grid_scene(gfx: &mut Graphics, room: &Room, grid: Option<&LightProbeGrid>) -> RgbaImage {
    let cam = Camera3D::perspective(Vec3::new(0.0, 0.0, 4.5), Vec3::ZERO, 1.3);
    gfx.scene(&cam, |s| {
        s.set_ambient(Color::BLACK);
        s.set_tonemap(Tonemap::None);
        s.set_light_probes(grid);
        room.walls(s, 1.0);
        let matte = Material::color(Color::WHITE, 0.0, 1.0);
        let at = |x: f32| {
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.8),
                Quat::IDENTITY,
                Vec3::new(x, 0.0, 0.0),
            )
        };
        s.draw(&room.sphere, &matte, at(-2.5));
        s.draw(&room.sphere, &matte, at(2.5));
    });
    render(gfx)
}

fn ball_pixels(img: &RgbaImage) -> ([i32; 3], [i32; 3]) {
    let cam = Camera3D::perspective(Vec3::new(0.0, 0.0, 4.5), Vec3::ZERO, 1.3);
    let view = yowie_core::vec2(SIZE as f32, SIZE as f32);
    let at = |x: f32| {
        // A point on each ball facing the camera.
        let s = cam
            .world_to_screen(Vec3::new(x, 0.0, 0.8), view)
            .expect("visible");
        px(img, s.x as u32, s.y as u32)
    };
    (at(-2.5), at(2.5))
}

#[test]
fn matte_balls_pick_up_the_wall_beside_them_from_the_grid() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let room = Room::new(&gfx);
    let mut grid = LightProbeGrid::new(
        Vec3::new(-3.0, 0.0, 0.0),
        Vec3::splat(3.0),
        UVec3::new(3, 1, 1),
    );
    gfx.bake_light_probes(&mut grid, |s| {
        s.set_ambient(Color::BLACK);
        room.walls(s, 1.0);
    })
    .expect("bake");
    assert!(grid.is_baked());
    let lit = grid_scene(&mut gfx, &room, Some(&grid));
    let (left, right) = ball_pixels(&lit);
    assert!(left[0] > left[1] + 30, "ball by the red wall {left:?}");
    assert!(right[1] > right[0] + 30, "ball by the green wall {right:?}");
    let dark = grid_scene(&mut gfx, &room, None);
    let (l0, _) = ball_pixels(&dark);
    assert!(l0.iter().all(|c| *c < 5), "no grid, no light: {l0:?}");
    // The processor lookup agrees: red at the left end, green at the right.
    let e = grid
        .irradiance(Vec3::new(-3.0, 0.0, 0.0), Vec3::NEG_X)
        .expect("baked");
    assert!(e.x > e.y * 2.0, "{e}");
}

#[test]
fn probes_capture_static_groups_like_ordinary_draws() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let room = Room::new(&gfx);
    let walls = room.static_walls(1.0);
    let new_grid = || {
        LightProbeGrid::new(
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::splat(3.0),
            UVec3::new(3, 1, 1),
        )
    };
    let (mut drawn, mut grouped) = (new_grid(), new_grid());
    gfx.bake_light_probes(&mut drawn, |s| {
        s.set_ambient(Color::BLACK);
        room.walls(s, 1.0);
    })
    .expect("bake");
    gfx.bake_light_probes(&mut grouped, |s| {
        s.set_ambient(Color::BLACK);
        s.draw_static(&walls);
    })
    .expect("bake");
    for x in 0..3 {
        let (a, b) = (drawn.coefficients(x, 0, 0), grouped.coefficients(x, 0, 0));
        let (a, b) = (a.expect("baked"), b.expect("baked"));
        for (ca, cb) in a.iter().zip(&b) {
            assert!(
                (*ca - *cb).abs().max_element() < 1e-3,
                "cell {x}: {a:?} {b:?}"
            );
        }
    }
    let env = blue_sky(&gfx);
    let p = probe();
    gfx.bake_reflection_probe(&p, |s| {
        s.set_ambient(Color::BLACK);
        s.set_environment(Some(&env));
        s.draw_static(&walls);
    });
    let with = chrome_scene(&mut gfx, &room, &env, Some(&p));
    let c = SIZE / 2;
    let w = px(&with, c, c);
    assert!(w[0] > 40 && (w[2] - w[0]).abs() < 15, "with probe {w:?}");
}

#[test]
fn saved_light_probes_load_back_and_light_the_same() {
    let Some(mut gfx) = headless(uvec2(SIZE, SIZE)) else {
        return;
    };
    let room = Room::new(&gfx);
    let mut grid = LightProbeGrid::new(
        Vec3::new(-3.0, 0.0, 0.0),
        Vec3::splat(3.0),
        UVec3::new(3, 1, 1),
    );
    gfx.bake_light_probes(&mut grid, |s| {
        s.set_ambient(Color::BLACK);
        room.walls(s, 1.0);
    })
    .expect("bake");
    let bytes = grid.to_bytes().expect("baked");
    let loaded = LightProbeGrid::from_bytes(&bytes).expect("load");
    for x in 0..3 {
        assert_eq!(grid.coefficients(x, 0, 0), loaded.coefficients(x, 0, 0));
    }
    let a = grid_scene(&mut gfx, &room, Some(&grid));
    let b = grid_scene(&mut gfx, &room, Some(&loaded));
    assert_eq!(a.as_raw(), b.as_raw(), "the loaded grid lights identically");
}
