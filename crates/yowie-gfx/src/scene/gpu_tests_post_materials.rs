//! Draws the material features (clearcoat, glass, order-independent
//! transparency, a decal, an outline, a stencil mask, fur, x-ray and a
//! surface shader) with 4x multisampling, ambient occlusion, reflections
//! and temporal anti-aliasing on, and compares the picture with the one
//! the default settings give. GPU validation errors fail the test; it
//! skips with a printed message when no GPU adapter exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Vec3, uvec2, vec2};

use super::gpu_tests_materials::{facing, mesh, rgb, unlit};
use super::*;
use crate::gpu_tests::{headless, render};
use crate::{Graphics, Texture, TextureOptions};

const W: u32 = 320;
const H: u32 = 160;
const TOP: f32 = 1.6;
const BOTTOM: f32 = -1.6;
const COLUMNS: [f32; 4] = [-4.2, -1.4, 1.4, 4.2];

fn camera() -> Camera3D {
    Camera3D::perspective(Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, 0.8)
}

/// Where `p` lands on the frame.
fn px(p: Vec3) -> (u32, u32) {
    let s = camera()
        .world_to_screen(p, vec2(W as f32, H as f32))
        .expect("visible");
    (s.x as u32, s.y as u32)
}

fn dump(img: &RgbaImage, name: &str) {
    if let Ok(dir) = std::env::var("YOWIE_TEST_DUMP") {
        let _ = img.save(format!("{dir}/{name}.png"));
    }
}

/// The meshes, decal and shader of a white backdrop with one feature in
/// each of eight cells.
struct World {
    sphere: Mesh,
    cube: Mesh,
    quad: Mesh,
    back: Mesh,
    decal: Decal,
    shader: SurfaceShader,
}

impl World {
    fn new(gfx: &mut Graphics) -> Self {
        let red = Texture::from_pixels(
            gfx,
            4,
            4,
            &[255, 0, 0, 255].repeat(16),
            TextureOptions::default(),
        )
        .expect("texture");
        let shader = gfx
            .compile_surface_shader(
                "fn material(s: Surface) -> Surface {\n    var o = s;\n    o.base = vec3<f32>(0.0, 1.0, 0.0);\n    return o;\n}\n",
            )
            .expect("shader");
        Self {
            sphere: mesh(gfx, &MeshData::sphere(1.0, 24, 48)),
            cube: mesh(gfx, &MeshData::cube(1.0)),
            quad: mesh(gfx, &MeshData::plane(1.0, 1)),
            back: mesh(gfx, &MeshData::plane(20.0, 1)),
            decal: Decal::new(red),
            shader,
        }
    }

    fn draw(&self, s: &mut Scene, post: &PostSettings) {
        s.set_ambient(Color::BLACK);
        s.set_post(PostSettings {
            tonemap: Tonemap::None,
            ..post.clone()
        });
        let sun = Vec3::new(-0.3, -0.3, -1.0).normalize();
        s.set_sun(DirectionalLight::new(sun, Color::WHITE, 2.0));
        let backdrop = facing(Vec3::new(0.0, 0.0, -3.0));
        s.draw(&self.back, &unlit(Color::WHITE), backdrop);
        let at = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let scaled = |p: Vec3, k: f32| Mat4::from_translation(p) * Mat4::from_scale(Vec3::splat(k));
        // Top row: clearcoat, tinted glass, an outline, fur.
        let mut coat = Material::color(Color::rgb(0.5, 0.1, 0.1), 0.0, 0.6);
        coat.clearcoat = Some(Clearcoat::new(1.0, 0.05));
        s.draw(&self.sphere, &coat, scaled(at(COLUMNS[0], TOP, 0.0), 0.8));
        let mut glass = Material::color(Color::WHITE, 0.0, 0.05);
        let mut t = Transmission::glass(1.5);
        t.attenuation_color = Color::rgb(1.0, 0.3, 0.3);
        t.attenuation_distance = 0.6;
        glass.transmission = Some(t);
        s.draw(&self.cube, &glass, scaled(at(COLUMNS[1], TOP, 0.0), 1.2));
        let ringed = Material {
            outline: Some(Outline::new(4.0, Color::GREEN)),
            ..unlit(Color::RED)
        };
        s.draw(&self.sphere, &ringed, scaled(at(COLUMNS[2], TOP, 0.0), 0.8));
        let mut fur = Material::color(Color::rgb(0.6, 0.5, 0.3), 0.0, 0.8);
        fur.fur = Some(Fur::new(8, 0.1));
        s.draw(&self.sphere, &fur, scaled(at(COLUMNS[3], TOP, 0.0), 0.7));
        // Bottom row: a decal, blended panes, a stencil mask, a surface
        // shader seen through a wall with x-ray.
        let card = Material::color(Color::WHITE, 0.0, 1.0);
        let c = at(COLUMNS[0], BOTTOM, 0.0);
        s.draw(
            &self.quad,
            &card,
            facing(c) * Mat4::from_scale(Vec3::splat(1.8)),
        );
        let decal_box = facing(c) * Mat4::from_scale(Vec3::new(0.8, 1.0, 0.8));
        s.draw_decal(&self.decal, decal_box);
        let panes = [
            (Color::rgba(1.0, 0.0, 0.0, 0.5), at(-0.3, 0.25, 0.0)),
            (Color::rgba(0.0, 1.0, 0.0, 0.5), at(0.3, 0.25, -0.4)),
            (Color::rgba(0.0, 0.0, 1.0, 0.5), at(0.0, -0.3, -0.8)),
        ];
        for (color, offset) in panes {
            let m = Material {
                alpha_mode: AlphaMode::Blend,
                ..unlit(color)
            };
            let p = at(COLUMNS[1], BOTTOM, 0.0) + offset;
            s.draw(
                &self.quad,
                &m,
                facing(p) * Mat4::from_scale(Vec3::splat(1.2)),
            );
        }
        let marker = Material {
            stencil: Stencil::mark(1),
            ..unlit(Color::GREEN)
        };
        let m = at(COLUMNS[2], BOTTOM, 0.0);
        s.draw(
            &self.quad,
            &marker,
            facing(m) * Mat4::from_scale(Vec3::splat(0.8)),
        );
        let masked = Material {
            stencil: Stencil::inside(1),
            ..unlit(Color::RED)
        };
        let front = m + Vec3::Z * 0.5;
        s.draw(
            &self.quad,
            &masked,
            facing(front) * Mat4::from_scale(Vec3::splat(1.6)),
        );
        let shaded = Material {
            shader: Some(self.shader.clone()),
            xray: Some(Color::CYAN),
            ..Material::color(Color::WHITE, 0.0, 0.8)
        };
        let x = at(COLUMNS[3], BOTTOM, 0.0);
        s.draw(&self.sphere, &shaded, scaled(x, 0.7));
        let wall = Mat4::from_translation(x + Vec3::new(0.0, -0.4, 1.0))
            * Mat4::from_scale(Vec3::new(1.6, 0.5, 0.1));
        s.draw(&self.cube, &unlit(Color::GRAY), wall);
    }

    /// One frame through `post`, failing on any GPU validation error.
    fn frame(&self, gfx: &mut Graphics, post: &PostSettings) -> RgbaImage {
        let scope = gfx.device().push_error_scope(wgpu::ErrorFilter::Validation);
        gfx.scene(&camera(), |s| self.draw(s, post));
        let img = render(gfx);
        let error = pollster::block_on(scope.pop());
        assert!(error.is_none(), "GPU validation error: {error:?}");
        img
    }
}

/// The mean absolute channel difference of two images, 0 to 255.
fn mean_diff(a: &RgbaImage, b: &RgbaImage) -> f32 {
    let sum: u64 = a
        .pixels()
        .zip(b.pixels())
        .flat_map(|(p, q)| (0..3).map(move |i| u64::from(p.0[i].abs_diff(q.0[i]))))
        .sum();
    sum as f32 / (a.width() * a.height() * 3) as f32
}

#[test]
fn material_features_draw_under_multisampling_and_post_effects() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    gfx.device()
        .on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            panic!("GPU error: {e}");
        }));
    let counts = super::targets::sample_counts(&gfx.shared.gpu.adapter, gfx.shared.device());
    if !counts.contains(&4) {
        println!("skipping: no 4x multisampling for the scene formats");
        return;
    }
    let world = World::new(&mut gfx);
    let plain = world.frame(&mut gfx, &PostSettings::default());
    let heavy = PostSettings {
        samples: 4,
        ambient_occlusion: 1.0,
        occlusion_radius: 1.0,
        reflections: 1.0,
        temporal_aa: true,
        ..PostSettings::default()
    };
    let mut img = world.frame(&mut gfx, &heavy);
    // Temporal anti-aliasing settles over a few frames.
    for _ in 0..4 {
        img = world.frame(&mut gfx, &heavy);
    }
    dump(&plain, "post_materials_plain");
    dump(&img, "post_materials_heavy");
    let d = mean_diff(&plain, &img);
    assert!(d < 6.0, "the picture stays the same: {d}");

    // Glass refracts the white backdrop through its red absorption.
    let (gx, gy) = px(Vec3::new(COLUMNS[1], TOP, 0.0));
    let g = rgb(&img, gx, gy);
    assert!(g[0] > 120.0 && g[1] < g[0] * 0.5, "glass: {g:?}");
    // The blended panes cover the backdrop where they overlap.
    let (ox, oy) = px(Vec3::new(COLUMNS[1], BOTTOM + 0.05, 0.0));
    let o = rgb(&img, ox, oy);
    assert!(o.iter().any(|v| *v < 200.0), "panes: {o:?}");
    // The decal colours the card inside its box only.
    let (dx, dy) = px(Vec3::new(COLUMNS[0] + 0.1, BOTTOM + 0.1, 0.0));
    let inside = rgb(&img, dx, dy);
    assert!(
        inside[0] > 80.0 && inside[1] < inside[0] * 0.4,
        "decal: {inside:?}"
    );
    let (ex, ey) = px(Vec3::new(COLUMNS[0] + 0.7, BOTTOM, 0.0));
    let outside = rgb(&img, ex, ey);
    assert!(
        outside[0] > 60.0 && (outside[0] - outside[1]).abs() < 12.0,
        "card beyond the decal: {outside:?}"
    );
    // The stencil keeps the red quad inside the marker.
    let front = Vec3::new(COLUMNS[2], BOTTOM, 0.5);
    let (sx, sy) = px(front);
    let s = rgb(&img, sx, sy);
    assert!(s[0] > 200.0 && s[1] < 30.0, "inside the mask: {s:?}");
    let (tx, ty) = px(front + Vec3::X * 0.65);
    let t = rgb(&img, tx, ty);
    assert!(t[1] > 200.0, "outside the mask: {t:?}");
    // A green line rings the outlined sphere.
    let (cx, cy) = px(Vec3::new(COLUMNS[2], TOP, 0.0));
    let ring = (cx..cx + 30).any(|x| {
        let p = rgb(&img, x, cy);
        p[1] > 150.0 && p[0] < 60.0
    });
    assert!(ring, "outline");
    // Each feature matches the default settings' picture.
    for (x, y) in [(gx, gy), (ox, oy), (dx, dy), (ex, ey), (sx, sy), (tx, ty)] {
        let (p, q) = (rgb(&plain, x, y), rgb(&img, x, y));
        let worst = (0..3).map(|i| (p[i] - q[i]).abs()).fold(0.0, f32::max);
        assert!(worst < 24.0, "({x}, {y}): {p:?} vs {q:?}");
    }
}
