//! Renders static groups headless: they must draw what the same draws
//! submitted one by one draw, and occlusion culling must skip only what is
//! hidden. Every test skips with a printed message when no GPU adapter
//! exists.

use image::RgbaImage;
use yowie_core::{Color, Mat4, Quat, Vec3, uvec2, vec2};

use crate::Graphics;
use crate::gpu_tests::{headless, render};
use crate::scene::*;

const W: u32 = 128;
const H: u32 = 96;

struct Props {
    cube: Mesh,
    sphere: Mesh,
    ground: Mesh,
    materials: [Material; 3],
}

impl Props {
    fn new(gfx: &Graphics) -> Self {
        let mesh = |d: &MeshData| Mesh::new(gfx, d).expect("mesh");
        Self {
            cube: mesh(&MeshData::cube(0.6)),
            sphere: mesh(&MeshData::sphere(0.4, 12, 24)),
            ground: mesh(&MeshData::plane(30.0, 1)),
            materials: [
                Material::color(Color::rgb8(200, 80, 60), 0.0, 0.6),
                Material::color(Color::rgb8(60, 140, 220), 0.3, 0.4),
                Material::color(Color::rgb8(230, 230, 230), 0.0, 0.9),
            ],
        }
    }

    /// A field of cubes and spheres in two materials, and the ground.
    fn items(&self) -> Vec<(&Mesh, &Material, Mat4)> {
        let mut v = Vec::new();
        for i in 0..60 {
            let p = Vec3::new(
                (i % 10) as f32 * 1.3 - 6.0,
                0.4,
                (i / 10) as f32 * -1.5 + 2.0,
            );
            let mesh = if i % 3 == 0 { &self.sphere } else { &self.cube };
            let rot = Quat::from_rotation_y(i as f32 * 0.4);
            v.push((
                mesh,
                &self.materials[i % 2],
                Mat4::from_rotation_translation(rot, p),
            ));
        }
        v.push((&self.ground, &self.materials[2], Mat4::IDENTITY));
        v
    }
}

fn camera() -> Camera3D {
    Camera3D::perspective(Vec3::new(0.0, 5.0, 9.0), Vec3::new(0.0, 0.0, -2.0), 0.9)
}

fn light(s: &mut Scene) {
    s.set_tonemap(Tonemap::None);
    s.set_sun(
        DirectionalLight::new(Vec3::new(-0.4, -1.0, -0.3), Color::WHITE, 2.5).with_shadows(30.0),
    );
}

fn draw_dynamic(gfx: &mut Graphics, props: &Props) -> RgbaImage {
    gfx.scene(&camera(), |s| {
        light(s);
        for (mesh, mat, model) in props.items() {
            s.draw(mesh, mat, model);
        }
    });
    render(gfx)
}

fn draw_group(gfx: &mut Graphics, group: &StaticGroup) -> RgbaImage {
    gfx.scene(&camera(), |s| {
        light(s);
        s.draw_static(group);
    });
    render(gfx)
}

fn assert_same(a: &RgbaImage, b: &RgbaImage) {
    let worst = a
        .pixels()
        .zip(b.pixels())
        .flat_map(|(p, q)| {
            p.0.iter()
                .zip(q.0)
                .map(|(x, y)| x.abs_diff(y))
                .collect::<Vec<_>>()
        })
        .max()
        .unwrap_or(0);
    assert!(worst <= 1, "pixels differ by up to {worst}");
}

#[test]
fn a_static_group_draws_the_same_pixels_as_dynamic_draws() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let props = Props::new(&gfx);
    let mut group = StaticGroup::new();
    for (mesh, mat, model) in props.items() {
        group.add(mesh, mat, model);
    }
    let want = draw_dynamic(&mut gfx, &props);
    let dynamic_calls = gfx.stats().draw_calls_3d;
    // The first frame draws everything late; later ones mostly early.
    for _ in 0..3 {
        let got = draw_group(&mut gfx, &group);
        assert_same(&want, &got);
    }
    let st = gfx.stats();
    // Cubes and spheres in two materials and the ground make five
    // batches, each drawn in both phases.
    assert_eq!(st.bundle_draws, 10, "{st:?}");
    assert_eq!(st.draws_3d, 61);
    // The five instanced opaque draws became ten indirect ones; shadow
    // maps batch the casters alike.
    assert_eq!(st.draw_calls_3d, dynamic_calls + 5, "{st:?}");
    let shadows = st.shadow_instances;
    draw_dynamic(&mut gfx, &props);
    assert_eq!(gfx.stats().shadow_instances, shadows, "same casters");
}

#[test]
fn without_gpu_culling_a_static_group_draws_as_ordinary_draws() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    gfx.scene_renderer().statics.disable_gpu_culling();
    let props = Props::new(&gfx);
    let mut group = StaticGroup::new();
    for (mesh, mat, model) in props.items() {
        group.add(mesh, mat, model);
    }
    let want = draw_dynamic(&mut gfx, &props);
    let got = draw_group(&mut gfx, &group);
    assert_same(&want, &got);
    assert_eq!(gfx.stats().bundle_draws, 0);
}

/// A wall, a box hidden behind it and a box beside it, all static.
fn walled(gfx: &Graphics) -> (StaticGroup, Mesh) {
    let cube = Mesh::new(gfx, &MeshData::cube(1.0)).expect("mesh");
    let mut g = StaticGroup::new();
    let wall =
        Mat4::from_scale_rotation_translation(Vec3::new(4.0, 3.0, 0.3), Quat::IDENTITY, Vec3::ZERO);
    g.add(
        &cube,
        &Material::color(Color::rgb8(120, 120, 120), 0.0, 0.8),
        wall,
    );
    g.add(
        &cube,
        &Material::glowing(Color::RED, 1.0),
        Vec3::new(0.0, 0.0, -3.0),
    );
    g.add(
        &cube,
        &Material::glowing(Color::GREEN, 1.0),
        Vec3::new(4.0, 0.0, 0.0),
    );
    (g, cube)
}

fn walled_frame(gfx: &mut Graphics, group: &StaticGroup, cam: &Camera3D) -> RgbaImage {
    gfx.scene(cam, |s| {
        s.set_ambient(Color::WHITE);
        s.set_tonemap(Tonemap::None);
        s.draw_static(group);
    });
    render(gfx)
}

#[test]
fn a_box_behind_a_wall_is_occluded_and_appears_the_frame_it_shows() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let (group, _cube) = walled(&gfx);
    let view = vec2(W as f32, H as f32);
    let front = Camera3D::perspective(Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, 0.9);
    let mut occluded = 0;
    for _ in 0..8 {
        walled_frame(&mut gfx, &group, &front);
        occluded = gfx.stats().occluded_3d;
        if occluded > 0 {
            break;
        }
    }
    assert_eq!(occluded, 1, "the hidden box, and only it, is occluded");
    let img = walled_frame(&mut gfx, &group, &front);
    let green = front
        .world_to_screen(Vec3::new(4.0, 0.0, 0.5), view)
        .expect("visible");
    let g = img.get_pixel(green.x as u32, green.y as u32).0;
    assert!(
        g[1] > 200 && g[0] < 40,
        "the box beside the wall draws: {g:?}"
    );
    // Step round the wall: the box it hid must draw in this very frame.
    let side = Camera3D::perspective(Vec3::new(-6.0, 0.0, 2.0), Vec3::new(0.0, 0.0, -3.0), 0.9);
    let img = walled_frame(&mut gfx, &group, &side);
    let red = side
        .world_to_screen(Vec3::new(0.0, 0.0, -3.0), view)
        .expect("visible");
    let r = img.get_pixel(red.x as u32, red.y as u32).0;
    assert!(
        r[0] > 200 && r[1] < 40,
        "the box behind the wall shows at once: {r:?}"
    );
}

/// Multisampling with occlusion and reflections on (so the HDR pass has
/// the surface images too), a probe captured every frame, and a static
/// group culled on the GPU against multisampled depth.
#[test]
fn static_groups_occlusion_and_probes_work_with_multisampling() {
    let Some(mut gfx) = headless(uvec2(W, H)) else {
        return;
    };
    let counts = super::super::targets::sample_counts(&gfx.shared.gpu.adapter, gfx.shared.device());
    if !counts.contains(&4) {
        println!("skipping: no 4x multisampling for the scene formats");
        return;
    }
    let (group, _cube) = walled(&gfx);
    let bounds = Aabb::new(Vec3::splat(-10.0), Vec3::splat(10.0));
    let probe = ReflectionProbe::new(Vec3::new(0.0, 2.0, 4.0), bounds)
        .with_update(ProbeUpdate::EveryFrames(1));
    let ball = Mesh::new(&gfx, &MeshData::sphere(0.5, 12, 24)).expect("mesh");
    let post = PostSettings {
        samples: 4,
        ambient_occlusion: 0.5,
        reflections: 0.5,
        ..PostSettings::default()
    };
    let frame = |gfx: &mut Graphics, cam: &Camera3D| {
        gfx.scene(cam, |s| {
            s.set_ambient(Color::WHITE);
            s.set_post(PostSettings {
                tonemap: Tonemap::None,
                ..post.clone()
            });
            s.add_reflection_probe(&probe);
            s.draw_static(&group);
            s.draw(
                &ball,
                &Material::color(Color::WHITE, 1.0, 0.1),
                Vec3::new(-3.0, 0.0, 1.0),
            );
        });
        render(gfx)
    };
    let view = vec2(W as f32, H as f32);
    let front = Camera3D::perspective(Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, 0.9);
    let mut occluded = 0;
    for _ in 0..8 {
        frame(&mut gfx, &front);
        occluded = gfx.stats().occluded_3d;
        if occluded > 0 {
            break;
        }
    }
    assert_eq!(
        occluded, 1,
        "the hidden box is occluded under multisampling"
    );
    assert!(gfx.stats().probe_faces > 0, "the probe captures");
    let img = frame(&mut gfx, &front);
    let green = front
        .world_to_screen(Vec3::new(4.0, 0.0, 0.5), view)
        .expect("visible");
    let g = img.get_pixel(green.x as u32, green.y as u32).0;
    assert!(g[1] > 200 && g[0] < 40, "the box beside the wall: {g:?}");
    let side = Camera3D::perspective(Vec3::new(-6.0, 0.0, 2.0), Vec3::new(0.0, 0.0, -3.0), 0.9);
    let img = frame(&mut gfx, &side);
    let red = side
        .world_to_screen(Vec3::new(0.0, 0.0, -3.0), view)
        .expect("visible");
    let r = img.get_pixel(red.x as u32, red.y as u32).0;
    assert!(r[0] > 200 && r[1] < 40, "the box behind the wall: {r:?}");
}
