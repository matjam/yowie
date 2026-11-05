use yowie_core::{Rect, Transform, Transform2D, Vec2, Vec3};

use super::*;
use crate::{BodyDesc2D, ColliderDesc2D, ColliderDesc3D, Shape2D, Shape3D, World2D, World3D};

const DT: f32 = 1.0 / 60.0;

fn flag(cols: usize, rows: usize) -> ClothDesc {
    ClothDesc {
        cols,
        rows,
        spacing: 0.1,
        mass: 0.5,
        origin: Vec3::new(0.0, 3.0, 0.0),
        pinned: vec![0, cols - 1],
        ..ClothDesc::default()
    }
}

#[test]
fn cloth_stays_attached_at_pins_and_keeps_edge_lengths() {
    let mut cloth = Cloth::new(&flag(12, 12)).unwrap();
    let start = (cloth.positions()[0], cloth.positions()[11]);
    let settings = SoftSettings::default();
    for _ in 0..240 {
        cloth.step(DT, &settings, &());
    }
    assert_eq!((cloth.positions()[0], cloth.positions()[11]), start);
    assert!(cloth.is_pinned(0) && !cloth.is_pinned(5));
    assert!(cloth.max_strain() < 0.1, "strain {}", cloth.max_strain());
    let lowest = cloth
        .positions()
        .iter()
        .map(|p| p.y)
        .fold(f32::MAX, f32::min);
    assert!(lowest < 3.0 - 0.5, "cloth should sag, lowest {lowest}");
    assert!(cloth.positions().iter().all(|p| p.is_finite()));
}

#[test]
fn cloth_in_wind_stays_finite_and_billows() {
    let mut desc = flag(10, 10);
    desc.wind = Wind {
        velocity: Vec3::new(0.0, 0.0, 6.0),
        drag: 0.5,
    };
    let mut cloth = Cloth::new(&desc).unwrap();
    let settings = SoftSettings::default();
    for _ in 0..180 {
        cloth.step(DT, &settings, &());
    }
    let z: f32 = cloth.positions().iter().map(|p| p.z).sum::<f32>() / 100.0;
    assert!(z > 0.1, "wind should push the cloth along +Z, mean z {z}");
    assert!(cloth.max_strain() < 0.15);
}

#[test]
fn cloth_drapes_over_box_collider() {
    let mut world = World3D::default();
    let desc = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(0.5, 0.25, 0.5)))
        .with_offset(Transform::from_xyz(0.5, 1.0, 0.5));
    world.add_static_collider(&desc).unwrap();
    world.step(DT);
    let mut cloth = Cloth::new(&ClothDesc {
        cols: 16,
        rows: 16,
        spacing: 0.08,
        origin: Vec3::new(-0.1, 1.6, -0.1),
        u: Vec3::X,
        v: Vec3::Z,
        ..ClothDesc::default()
    })
    .unwrap();
    let settings = SoftSettings::default();
    for _ in 0..240 {
        cloth.step(DT, &settings, &world);
    }
    let top = 1.25;
    let inside = cloth
        .positions()
        .iter()
        .filter(|p| (0.0..1.0).contains(&p.x) && (0.0..1.0).contains(&p.z) && p.y < top - 0.01);
    assert_eq!(inside.count(), 0, "particles sank into the box");
    let resting = cloth
        .positions()
        .iter()
        .any(|p| (p.y - top).abs() < 0.05 && p.x > 0.3 && p.x < 0.7);
    assert!(resting, "cloth should lie on top of the box");
}

#[test]
fn soft_cube_keeps_its_volume_when_dropped() {
    let mut body = SoftBody::lattice(
        Vec3::new(0.0, 1.5, 0.0),
        Vec3::splat(1.0),
        [4, 4, 4],
        &SoftBodyDesc {
            mass: 2.0,
            ..SoftBodyDesc::default()
        },
    )
    .unwrap();
    assert!((body.rest_volume() - 1.0).abs() < 1e-4);
    let settings = SoftSettings {
        ground: Some(Ground::default()),
        ..SoftSettings::default()
    };
    let mut worst = 0.0_f32;
    for i in 0..300 {
        body.step(DT, &settings, &());
        if i > 30 {
            worst = worst.max((body.volume() / body.rest_volume() - 1.0).abs());
        }
    }
    assert!(worst < 0.08, "volume drifted by {worst}");
    let lowest = body
        .positions()
        .iter()
        .map(|p| p.y)
        .fold(f32::MAX, f32::min);
    assert!(lowest > -0.01, "body sank through the ground: {lowest}");
    let top = body
        .positions()
        .iter()
        .map(|p| p.y)
        .fold(f32::MIN, f32::max);
    assert!(top > 0.7, "body collapsed, top at {top}");
}

#[test]
fn soft_body_rests_on_world_collider_and_exports_mesh() {
    let mut world = World3D::default();
    let floor = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(5.0, 0.5, 5.0)))
        .with_offset(Transform::from_xyz(0.0, -0.5, 0.0));
    world.add_static_collider(&floor).unwrap();
    world.step(DT);
    let desc = SoftBodyDesc {
        shape_matching: 0.2,
        ..SoftBodyDesc::default()
    };
    let mut body =
        SoftBody::lattice(Vec3::new(0.0, 1.0, 0.0), Vec3::splat(0.6), [3, 3, 3], &desc).unwrap();
    let mut mesh = body.mesh();
    for _ in 0..240 {
        body.step(DT, &SoftSettings::default(), &world);
    }
    body.update_mesh(&mut mesh);
    let lowest = mesh.positions.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(lowest > -0.05 && lowest < 0.1, "lowest {lowest}");
    assert_eq!(mesh.normals.len(), body.positions().len());
    assert!(mesh.normals.iter().all(|n| (n.length() - 1.0).abs() < 1e-3));
    assert_eq!(mesh.indices.len(), body.surface().len() * 3);
    // 6 faces of 3x3 cells, two triangles each.
    assert_eq!(body.surface().len(), 6 * 9 * 2);
}

#[test]
fn invalid_descriptions_are_rejected() {
    assert!(
        Cloth::new(&ClothDesc {
            cols: 1,
            ..ClothDesc::default()
        })
        .is_err()
    );
    assert!(
        Cloth::new(&ClothDesc {
            pinned: vec![1000],
            ..ClothDesc::default()
        })
        .is_err()
    );
    assert!(SoftBody::lattice(Vec3::ZERO, Vec3::ONE, [0, 1, 1], &SoftBodyDesc::default()).is_err());
    assert!(
        Fluid2D::new(&FluidDesc2D {
            spacing: 0.0,
            ..FluidDesc2D::default()
        })
        .is_err()
    );
}

#[test]
fn flat_cloth_mesh_faces_the_front_normal() {
    let cloth = Cloth::new(&ClothDesc::default()).unwrap();
    let mesh = cloth.mesh();
    assert_eq!(mesh.indices.len(), 9 * 9 * 2 * 3);
    for n in &mesh.normals {
        assert!((*n - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4, "{n:?}");
    }
}

#[test]
fn fluid_stays_inside_container_and_settles() {
    let bounds = Rect::new(0.0, 0.0, 1.5, 2.0);
    let mut fluid = Fluid2D::new(&FluidDesc2D {
        bounds: Some(bounds),
        spacing: 0.06,
        ..FluidDesc2D::default()
    })
    .unwrap();
    fluid.fill(Rect::new(0.1, 0.1, 0.9, 1.5));
    let n = fluid.count();
    assert!(n > 300);
    for _ in 0..300 {
        fluid.step(DT, Vec2::new(0.0, -9.81), &());
    }
    assert_eq!(fluid.count(), n);
    for p in fluid.positions() {
        assert!(p.is_finite());
        assert!(bounds.inflate(0.001).contains(*p), "escaped to {p:?}");
    }
    let mean_y = fluid.positions().iter().map(|p| p.y).sum::<f32>() / n as f32;
    assert!(
        mean_y < 0.7,
        "fluid should sink and spread, mean y {mean_y}"
    );
    let speed = fluid.velocities().iter().map(|v| v.length()).sum::<f32>() / n as f32;
    assert!(speed < 1.5, "fluid should calm down, mean speed {speed}");
    let dense = (0..n)
        .map(|i| fluid.density(i) / fluid.rest_density())
        .fold(0.0, f32::max);
    assert!(dense < 2.0, "density ratio {dense}");
}

#[test]
fn fluid_collides_with_world_colliders() {
    let mut world = World2D::default();
    for (x, y, hx, hy) in [
        (0.0, -0.1, 1.0, 0.1),
        (-1.0, 0.5, 0.1, 0.6),
        (1.0, 0.5, 0.1, 0.6),
    ] {
        let desc = ColliderDesc2D::new(Shape2D::cuboid(Vec2::new(hx, hy)))
            .with_offset(Transform2D::from_position(Vec2::new(x, y)));
        world.add_static_collider(&desc).unwrap();
    }
    let moving = world.add_body(&BodyDesc2D::dynamic().at(Vec2::new(5.0, 5.0)));
    world
        .add_collider(moving, &ColliderDesc2D::new(Shape2D::circle(0.1)))
        .unwrap();
    world.step(DT);
    let mut fluid = Fluid2D::new(&FluidDesc2D {
        spacing: 0.06,
        ..FluidDesc2D::default()
    })
    .unwrap();
    fluid.fill(Rect::new(-0.5, 0.3, 1.0, 0.6));
    for _ in 0..360 {
        fluid.step(DT, Vec2::new(0.0, -9.81), &world);
    }
    for p in fluid.positions() {
        assert!(p.y > -0.01, "fell through the floor: {p:?}");
        assert!(p.x > -0.95 && p.x < 0.95, "left the tank: {p:?}");
    }
}
