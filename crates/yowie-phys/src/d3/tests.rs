use yowie_core::{Quat, Transform, Vec3};

use super::*;
use crate::{BodyKind, CollisionLayers, Event, Limits, Motor, QueryFilter};

const DT: f32 = 1.0 / 60.0;

fn ground(world: &mut World3D) {
    let desc = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(50.0, 0.5, 50.0)))
        .with_offset(Transform::from_xyz(0.0, -0.5, 0.0))
        .with_restitution(0.0);
    world.add_static_collider(&desc).unwrap();
}

fn run(world: &mut World3D, steps: usize) {
    for _ in 0..steps {
        world.step(DT);
    }
}

fn ball(world: &mut World3D, y: f32, radius: f32, restitution: f32) -> crate::BodyId {
    let id = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, y, 0.0)));
    let c = ColliderDesc3D::new(Shape3D::sphere(radius)).with_restitution(restitution);
    world.add_collider(id, &c).unwrap();
    id
}

#[test]
fn box_comes_to_rest_on_ground() {
    let mut world = World3D::default();
    ground(&mut world);
    let id = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, 3.0, 0.0)));
    let c = ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5)));
    world.add_collider(id, &c).unwrap();
    run(&mut world, 300);
    let p = world.position(id).unwrap();
    assert!((p.y - 0.5).abs() < 0.02, "rest height {}", p.y);
    assert!(world.linear_velocity(id).unwrap().length() < 0.05);
    assert!(world.is_sleeping(id), "box should fall asleep");
    assert_eq!(world.kind(id), Some(BodyKind::Dynamic));
}

#[test]
fn restitution_sets_bounce_height() {
    let cases = [(0.5_f32, 1.0_f32), (0.8, 2.56)];
    for (e, expected) in cases {
        let mut world = World3D::default();
        ground(&mut world);
        let id = ball(&mut world, 4.5, 0.5, e);
        let mut bounced = false;
        let mut peak = 0.0_f32;
        let mut prev_vy = 0.0;
        for _ in 0..400 {
            world.step(DT);
            let vy = world.linear_velocity(id).unwrap().y;
            if prev_vy < -1.0 && vy > 0.0 {
                bounced = true;
            }
            if bounced {
                peak = peak.max(world.position(id).unwrap().y - 0.5);
            }
            prev_vy = vy;
            if bounced && vy < 0.0 {
                break;
            }
        }
        assert!(bounced);
        assert!(
            (peak - expected).abs() < 0.25 * expected,
            "e={e} peak={peak}"
        );
    }
}

#[test]
fn hinge_limits_hold() {
    let mut world = World3D::default();
    let anchor = world.add_body(&BodyDesc3D::fixed());
    let arm = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(1.0, 0.0, 0.0)));
    world
        .add_collider(
            arm,
            &ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(1.0, 0.1, 0.1))),
        )
        .unwrap();
    let joint = JointDesc3D::hinge(Vec3::Z)
        .with_anchors(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0))
        .with_limits(Limits::new(-0.5, 0.5));
    world.add_joint(anchor, arm, &joint).unwrap();
    let mut lowest = 0.0_f32;
    for _ in 0..240 {
        world.step(DT);
        let q = world.rotation(arm).unwrap();
        lowest = lowest.min(2.0 * q.z.atan2(q.w));
    }
    assert!(lowest > -0.56, "angle went to {lowest}");
    assert!(lowest < -0.4, "arm should reach the limit, got {lowest}");
}

#[test]
fn hinge_motor_spins_body() {
    let mut world = World3D::new(Vec3::ZERO);
    let anchor = world.add_body(&BodyDesc3D::fixed());
    let wheel = world.add_body(&BodyDesc3D::dynamic());
    world
        .add_collider(wheel, &ColliderDesc3D::new(Shape3D::sphere(0.5)))
        .unwrap();
    let joint = JointDesc3D::hinge(Vec3::Z).with_motor(Motor::velocity(2.0, 1.0e4));
    world.add_joint(anchor, wheel, &joint).unwrap();
    run(&mut world, 60);
    let w = world.angular_velocity(wheel).unwrap();
    assert!((w.z - 2.0).abs() < 0.2, "angular velocity {w:?}");
}

#[test]
fn ray_and_shape_queries_hit_expected_collider() {
    let mut world = World3D::default();
    ground(&mut world);
    let near = world.add_body(&BodyDesc3D::fixed().at(Vec3::new(5.0, 1.0, 0.0)));
    let far = world.add_body(&BodyDesc3D::fixed().at(Vec3::new(9.0, 1.0, 0.0)));
    let cn = world
        .add_collider(
            near,
            &ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5))),
        )
        .unwrap();
    let cf = world
        .add_collider(far, &ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5))))
        .unwrap();
    world.refresh_queries();
    let f = QueryFilter::new();

    let hit = world
        .ray_cast(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 100.0, &f)
        .unwrap();
    assert_eq!(hit.collider, cn);
    assert_eq!(hit.body, Some(near));
    assert!((hit.distance - 4.5).abs() < 1e-3);
    assert!((hit.normal - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-3);

    let skip = QueryFilter::new().without_body(near);
    let hit = world
        .ray_cast(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 100.0, &skip)
        .unwrap();
    assert_eq!(hit.collider, cf);
    assert_eq!(
        world
            .ray_cast_all(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 100.0, &f)
            .len(),
        2
    );
    assert!(
        world
            .ray_cast(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 2.0, &f)
            .is_none()
    );
    assert!(world.ray_cast(Vec3::ZERO, Vec3::ZERO, 2.0, &f).is_none());

    assert_eq!(world.overlap_point(Vec3::new(5.1, 1.1, 0.0), &f), vec![cn]);
    let at = Transform::from_xyz(8.6, 1.0, 0.0);
    let found = world.overlap_shape(&Shape3D::sphere(0.3), &at, &f).unwrap();
    assert_eq!(found, vec![cf]);

    let start = Transform::from_xyz(2.0, 1.0, 0.0);
    let hit = world
        .shape_cast(&Shape3D::sphere(0.25), &start, Vec3::X, 20.0, &f)
        .unwrap()
        .unwrap();
    assert_eq!(hit.collider, cn);
    assert!(
        (hit.distance - 2.25).abs() < 1e-2,
        "distance {}",
        hit.distance
    );
    assert!((hit.normal - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-2);
}

#[test]
fn signed_distance_inside_and_outside() {
    let cube = Shape3D::cuboid(Vec3::splat(1.0));
    let cases = [
        (Vec3::new(3.0, 0.0, 0.0), 2.0),
        (Vec3::new(0.5, 0.0, 0.0), -0.5),
        (Vec3::new(0.0, 1.0, 0.0), 0.0),
    ];
    for (p, expected) in cases {
        let d = signed_distance_3d(&cube, Vec3::ZERO, Quat::IDENTITY, p).unwrap();
        assert!(
            (d.distance - expected).abs() < 1e-4,
            "{p:?}: {}",
            d.distance
        );
    }
    let d =
        signed_distance_3d(&cube, Vec3::ZERO, Quat::IDENTITY, Vec3::new(3.0, 0.0, 0.0)).unwrap();
    assert!((d.normal - Vec3::X).length() < 1e-4);
}

#[test]
fn sensor_reports_enter_and_exit() {
    let mut world = World3D::default();
    let zone = world
        .add_static_collider(
            &ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(2.0, 1.0, 2.0)))
                .with_offset(Transform::from_xyz(0.0, 3.0, 0.0))
                .as_sensor(),
        )
        .unwrap();
    let id = ball(&mut world, 6.0, 0.25, 0.0);
    let body_collider = world.colliders_of(id)[0];
    let (mut entered, mut exited) = (0, 0);
    for _ in 0..180 {
        world.step(DT);
        for ev in world.events() {
            match *ev {
                Event::SensorEntered { sensor, other } => {
                    assert_eq!((sensor, other), (zone, body_collider));
                    entered += 1;
                }
                Event::SensorExited { sensor, other } => {
                    assert_eq!((sensor, other), (zone, body_collider));
                    exited += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!((entered, exited), (1, 1));
}

#[test]
fn contact_events_carry_impulse() {
    let mut world = World3D::default();
    ground(&mut world);
    let id = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, 2.0, 0.0)));
    let c = ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5))).with_contact_events();
    world.add_collider(id, &c).unwrap();
    let (mut started, mut impulse) = (false, 0.0_f32);
    for _ in 0..90 {
        world.step(DT);
        for ev in world.events() {
            match ev {
                Event::ContactStarted { .. } => started = true,
                Event::ContactImpulse { total, .. } => impulse = impulse.max(*total),
                _ => {}
            }
        }
    }
    assert!(started);
    assert!(impulse > 1.0, "impulse {impulse}");
}

#[test]
fn layers_let_bodies_pass_through() {
    let mut world = World3D::default();
    let floor = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(10.0, 0.5, 10.0)))
        .with_layers(CollisionLayers::new(0b01, 0b01));
    world.add_static_collider(&floor).unwrap();
    let id = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, 2.0, 0.0)));
    let c =
        ColliderDesc3D::new(Shape3D::sphere(0.25)).with_layers(CollisionLayers::new(0b10, 0b10));
    world.add_collider(id, &c).unwrap();
    run(&mut world, 120);
    assert!(world.position(id).unwrap().y < -2.0);
}

#[test]
fn interpolation_blends_previous_and_current_pose() {
    let mut world = World3D::new(Vec3::ZERO);
    let id = world.add_body(&BodyDesc3D::dynamic().moving(Vec3::new(6.0, 0.0, 0.0)));
    world
        .add_collider(id, &ColliderDesc3D::new(Shape3D::sphere(0.5)))
        .unwrap();
    world.step(DT);
    let now = world.position(id).unwrap().x;
    let mid = world.interpolated_transform(id, 0.5).unwrap().translation.x;
    assert!((mid - now / 2.0).abs() < 1e-3);
    assert!((world.interpolated_transform(id, 1.0).unwrap().translation.x - now).abs() < 1e-4);
}

#[test]
fn stale_ids_are_errors() {
    let mut world = World3D::default();
    let id = world.add_body(&BodyDesc3D::dynamic());
    world.remove_body(id).unwrap();
    assert!(world.remove_body(id).is_err());
    assert!(world.position(id).is_none());
    let c = ColliderDesc3D::new(Shape3D::sphere(1.0));
    assert!(world.add_collider(id, &c).is_err());
    let bad = ColliderDesc3D::new(Shape3D::sphere(-1.0));
    let ok = world.add_body(&BodyDesc3D::dynamic());
    assert!(world.add_collider(ok, &bad).is_err());
}

#[test]
fn character_climbs_step_and_stops_at_wall() {
    let mut world = World3D::new(Vec3::ZERO);
    ground(&mut world);
    let boxed = |x: f32, hx: f32, h: f32| {
        ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(hx, h / 2.0, 5.0)))
            .with_offset(Transform::from_xyz(x, h / 2.0, 0.0))
    };
    world.add_static_collider(&boxed(3.0, 1.0, 0.2)).unwrap();
    world.add_static_collider(&boxed(8.0, 0.5, 4.0)).unwrap();
    let body = world.add_body(&BodyDesc3D::kinematic().at(Vec3::new(0.0, 0.5, 0.0)));
    let shape = Shape3D::capsule(0.3, 0.2);
    world
        .add_collider(body, &ColliderDesc3D::new(shape.clone()))
        .unwrap();
    world.refresh_queries();
    let ctrl = CharacterController3D::new(shape);
    let mut on_step = false;
    for _ in 0..300 {
        let m = world
            .move_character_body(&ctrl, body, Vec3::new(2.0 * DT, -0.005, 0.0), DT)
            .unwrap();
        world.step(DT);
        let p = world.position(body).unwrap();
        if (2.2..3.8).contains(&p.x) && m.grounded {
            on_step = on_step || p.y > 0.65;
        }
    }
    let p = world.position(body).unwrap();
    assert!(on_step, "character never stood on the step");
    assert!(p.x < 7.5 && p.x > 6.5, "stopped at x={}", p.x);
}

#[test]
fn ragdoll_falls_and_stays_connected() {
    let mut world = World3D::default();
    ground(&mut world);
    let spec = RagdollSpec {
        position: Vec3::new(0.0, 0.5, 0.0),
        ..RagdollSpec::default()
    };
    let doll = Ragdoll3D::spawn(&mut world, &spec).unwrap();
    assert_eq!(doll.parts.len(), 11);
    assert_eq!(doll.joints.len(), 10);
    run(&mut world, 240);
    let head = world.position(doll.parts["head"]).unwrap();
    let pelvis = world.position(doll.parts["pelvis"]).unwrap();
    assert!(head.is_finite() && pelvis.is_finite());
    assert!(head.y > -0.05 && head.y < 1.0, "head at {head:?}");
    assert!(head.distance(pelvis) < 1.2);
    doll.despawn(&mut world);
    assert_eq!(world.body_count(), 0);
}

#[test]
fn debug_draw_emits_lines() {
    struct Count(usize);
    impl crate::DebugDraw for Count {
        fn line(&mut self, _: Vec3, _: Vec3, _: yowie_core::Color) {
            self.0 += 1;
        }
        fn point(&mut self, _: Vec3, _: yowie_core::Color) {}
    }
    let mut world = World3D::default();
    ground(&mut world);
    ball(&mut world, 2.0, 0.5, 0.0);
    world.step(DT);
    let mut c = Count(0);
    world.debug_draw(&mut c);
    assert!(c.0 > 20);
}

#[test]
fn joint_motor_can_be_changed_while_running() {
    let mut world = World3D::new(Vec3::ZERO);
    let anchor = world.add_body(&BodyDesc3D::fixed());
    let wheel = world.add_body(&BodyDesc3D::dynamic());
    world
        .add_collider(wheel, &ColliderDesc3D::new(Shape3D::sphere(0.5)))
        .unwrap();
    let joint = JointDesc3D::hinge(Vec3::Z).with_motor(Motor::velocity(1.0, 1.0e4));
    let id = world.add_joint(anchor, wheel, &joint).unwrap();
    run(&mut world, 60);
    assert!((world.angular_velocity(wheel).unwrap().z - 1.0).abs() < 0.2);
    world
        .set_joint_motor(id, Motor::velocity(-3.0, 1.0e4))
        .unwrap();
    run(&mut world, 90);
    let w = world.angular_velocity(wheel).unwrap();
    assert!((w.z + 3.0).abs() < 0.3, "angular velocity {w:?}");
}

#[test]
fn debug_contacts_draw_extra_lines() {
    struct Count(usize);
    impl crate::DebugDraw for Count {
        fn line(&mut self, _: Vec3, _: Vec3, _: yowie_core::Color) {
            self.0 += 1;
        }
        fn point(&mut self, _: Vec3, _: yowie_core::Color) {}
    }
    let mut world = World3D::default();
    ground(&mut world);
    let b = world.add_body(&BodyDesc3D::dynamic().at(Vec3::new(0.0, 0.5, 0.0)));
    world
        .add_collider(b, &ColliderDesc3D::new(Shape3D::cuboid(Vec3::splat(0.5))))
        .unwrap();
    run(&mut world, 30);
    let mut off = Count(0);
    world.debug_draw(&mut off);
    world.set_debug_contacts(true);
    let mut on = Count(0);
    world.debug_draw(&mut on);
    assert!(
        on.0 > off.0,
        "{} lines with contacts, {} without",
        on.0,
        off.0
    );
}
