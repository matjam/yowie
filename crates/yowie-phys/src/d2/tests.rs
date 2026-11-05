use yowie_core::{Transform2D, Vec2};

use super::*;
use crate::{BodyId, CollisionLayers, Event, Limits, Motor, QueryFilter};

const DT: f32 = 1.0 / 60.0;

fn ground(world: &mut World2D) {
    let desc = ColliderDesc2D::new(Shape2D::cuboid(Vec2::new(50.0, 0.5)))
        .with_offset(Transform2D::from_position(Vec2::new(0.0, -0.5)));
    world.add_static_collider(&desc).unwrap();
}

fn run(world: &mut World2D, steps: usize) {
    for _ in 0..steps {
        world.step(DT);
    }
}

fn ball(world: &mut World2D, y: f32, radius: f32, restitution: f32) -> BodyId {
    let id = world.add_body(&BodyDesc2D::dynamic().at(Vec2::new(0.0, y)));
    let c = ColliderDesc2D::new(Shape2D::circle(radius)).with_restitution(restitution);
    world.add_collider(id, &c).unwrap();
    id
}

#[test]
fn box_comes_to_rest_on_ground() {
    let mut world = World2D::default();
    ground(&mut world);
    let id = world.add_body(&BodyDesc2D::dynamic().at(Vec2::new(0.0, 3.0)).rotated(0.3));
    let c = ColliderDesc2D::new(Shape2D::cuboid(Vec2::splat(0.5)));
    world.add_collider(id, &c).unwrap();
    run(&mut world, 360);
    let p = world.position(id).unwrap();
    let a = world.rotation(id).unwrap();
    assert!((p.y - 0.5).abs() < 0.03, "rest height {}", p.y);
    assert!(a.abs() < 0.05, "box should lie flat, angle {a}");
    assert!(world.is_sleeping(id));
}

#[test]
fn restitution_sets_bounce_height() {
    let cases = [(0.5_f32, 1.0_f32), (0.8, 2.56)];
    for (e, expected) in cases {
        let mut world = World2D::default();
        ground(&mut world);
        let id = ball(&mut world, 4.5, 0.5, e);
        let (mut bounced, mut peak, mut prev_vy) = (false, 0.0_f32, 0.0);
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
    let mut world = World2D::default();
    let anchor = world.add_body(&BodyDesc2D::fixed());
    let arm = world.add_body(&BodyDesc2D::dynamic().at(Vec2::new(1.0, 0.0)));
    world
        .add_collider(
            arm,
            &ColliderDesc2D::new(Shape2D::cuboid(Vec2::new(1.0, 0.1))),
        )
        .unwrap();
    let joint = JointDesc2D::hinge()
        .with_anchors(Vec2::ZERO, Vec2::new(-1.0, 0.0))
        .with_limits(Limits::new(-0.5, 0.5));
    world.add_joint(anchor, arm, &joint).unwrap();
    let mut lowest = 0.0_f32;
    for _ in 0..240 {
        world.step(DT);
        lowest = lowest.min(world.rotation(arm).unwrap());
    }
    assert!(lowest > -0.56, "angle went to {lowest}");
    assert!(lowest < -0.4, "arm should reach the limit, got {lowest}");
}

#[test]
fn prismatic_limits_and_wheel_motor() {
    let mut world = World2D::default();
    let rail = world.add_body(&BodyDesc2D::fixed());
    let slider = world.add_body(&BodyDesc2D::dynamic());
    world
        .add_collider(
            slider,
            &ColliderDesc2D::new(Shape2D::cuboid(Vec2::splat(0.2))),
        )
        .unwrap();
    let joint = JointDesc2D::new(JointKind2D::Prismatic {
        axis: Vec2::Y,
        limits: Some(Limits::new(-1.0, 1.0)),
        motor: None,
    });
    world.add_joint(rail, slider, &joint).unwrap();
    run(&mut world, 180);
    assert!((world.position(slider).unwrap().y + 1.0).abs() < 0.05);

    let mut world = World2D::new(Vec2::ZERO);
    let hub = world.add_body(&BodyDesc2D::fixed());
    let wheel = world.add_body(&BodyDesc2D::dynamic());
    world
        .add_collider(wheel, &ColliderDesc2D::new(Shape2D::circle(0.5)))
        .unwrap();
    let joint = JointDesc2D::new(JointKind2D::Wheel {
        axis: Vec2::Y,
        stiffness: 100.0,
        damping: 10.0,
        motor: None,
    })
    .with_motor(Motor::velocity(3.0, 1.0e4));
    world.add_joint(hub, wheel, &joint).unwrap();
    run(&mut world, 60);
    let w = world.angular_velocity(wheel).unwrap();
    assert!((w - 3.0).abs() < 0.3, "wheel spin {w}");
}

#[test]
fn ray_and_shape_queries_hit_expected_collider() {
    let mut world = World2D::default();
    ground(&mut world);
    let mut boxes = Vec::new();
    for x in [5.0, 9.0] {
        let b = world.add_body(&BodyDesc2D::fixed().at(Vec2::new(x, 1.0)));
        boxes.push(
            world
                .add_collider(b, &ColliderDesc2D::new(Shape2D::cuboid(Vec2::splat(0.5))))
                .unwrap(),
        );
    }
    world.refresh_queries();
    let f = QueryFilter::new();
    let hit = world
        .ray_cast(Vec2::new(0.0, 1.0), Vec2::X, 100.0, &f)
        .unwrap();
    assert_eq!(hit.collider, boxes[0]);
    assert!((hit.distance - 4.5).abs() < 1e-3);
    assert!((hit.normal + Vec2::X).length() < 1e-3);
    assert_eq!(
        world
            .ray_cast_all(Vec2::new(0.0, 1.0), Vec2::X, 100.0, &f)
            .len(),
        2
    );
    assert!(
        world
            .ray_cast(Vec2::new(0.0, 1.0), Vec2::X, 2.0, &f)
            .is_none()
    );

    assert_eq!(world.overlap_point(Vec2::new(9.1, 1.1), &f), vec![boxes[1]]);
    let at = Transform2D::from_position(Vec2::new(8.6, 1.0));
    assert_eq!(
        world.overlap_shape(&Shape2D::circle(0.3), &at, &f).unwrap(),
        vec![boxes[1]]
    );

    let start = Transform2D::from_position(Vec2::new(2.0, 1.0));
    let hit = world
        .shape_cast(&Shape2D::circle(0.25), &start, Vec2::X, 20.0, &f)
        .unwrap()
        .unwrap();
    assert_eq!(hit.collider, boxes[0]);
    assert!(
        (hit.distance - 2.25).abs() < 1e-2,
        "distance {}",
        hit.distance
    );
}

#[test]
fn signed_distance_inside_and_outside() {
    let square = Shape2D::cuboid(Vec2::splat(1.0));
    let cases = [
        (Vec2::new(3.0, 0.0), 2.0),
        (Vec2::new(0.5, 0.0), -0.5),
        (Vec2::new(2.0, 2.0), 2.0_f32.sqrt()),
    ];
    for (p, expected) in cases {
        let d = signed_distance_2d(&square, Vec2::ZERO, 0.0, p).unwrap();
        assert!(
            (d.distance - expected).abs() < 1e-4,
            "{p:?}: {}",
            d.distance
        );
    }
    let rotated = signed_distance_2d(
        &square,
        Vec2::ZERO,
        std::f32::consts::FRAC_PI_4,
        Vec2::new(2.0, 0.0),
    )
    .unwrap();
    assert!((rotated.distance - (2.0 - 2.0_f32.sqrt())).abs() < 1e-4);
}

#[test]
fn sensor_reports_enter_and_exit() {
    let mut world = World2D::default();
    let zone = world
        .add_static_collider(
            &ColliderDesc2D::new(Shape2D::cuboid(Vec2::new(2.0, 1.0)))
                .with_offset(Transform2D::from_position(Vec2::new(0.0, 3.0)))
                .as_sensor(),
        )
        .unwrap();
    let id = ball(&mut world, 6.0, 0.25, 0.0);
    let ball_collider = world.colliders_of(id)[0];
    let (mut entered, mut exited) = (0, 0);
    for _ in 0..180 {
        world.step(DT);
        for ev in world.events() {
            match *ev {
                Event::SensorEntered { sensor, other } => {
                    assert_eq!((sensor, other), (zone, ball_collider));
                    entered += 1;
                }
                Event::SensorExited { sensor, other } => {
                    assert_eq!((sensor, other), (zone, ball_collider));
                    exited += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!((entered, exited), (1, 1));
}

#[test]
fn layers_and_stale_ids() {
    let mut world = World2D::default();
    let floor = ColliderDesc2D::new(Shape2D::cuboid(Vec2::new(10.0, 0.5)))
        .with_layers(CollisionLayers::new(0b01, 0b01));
    world.add_static_collider(&floor).unwrap();
    let id = world.add_body(&BodyDesc2D::dynamic().at(Vec2::new(0.0, 2.0)));
    let c =
        ColliderDesc2D::new(Shape2D::circle(0.25)).with_layers(CollisionLayers::new(0b10, 0b10));
    world.add_collider(id, &c).unwrap();
    run(&mut world, 120);
    assert!(world.position(id).unwrap().y < -2.0);
    world.remove_body(id).unwrap();
    assert!(world.remove_body(id).is_err());
    assert!(world.add_collider(id, &c).is_err());
}

#[test]
fn character_climbs_step_and_stops_at_wall() {
    let mut world = World2D::new(Vec2::ZERO);
    ground(&mut world);
    let block = |x: f32, hx: f32, h: f32| {
        ColliderDesc2D::new(Shape2D::cuboid(Vec2::new(hx, h / 2.0)))
            .with_offset(Transform2D::from_position(Vec2::new(x, h / 2.0)))
    };
    world.add_static_collider(&block(3.0, 1.0, 0.2)).unwrap();
    world.add_static_collider(&block(8.0, 0.5, 4.0)).unwrap();
    let body = world.add_body(&BodyDesc2D::kinematic().at(Vec2::new(0.0, 0.5)));
    let shape = Shape2D::capsule(0.3, 0.2);
    world
        .add_collider(body, &ColliderDesc2D::new(shape.clone()))
        .unwrap();
    world.refresh_queries();
    let ctrl = CharacterController2D::new(shape);
    let mut on_step = false;
    for _ in 0..300 {
        let m = world
            .move_character_body(&ctrl, body, Vec2::new(2.0 * DT, -0.005), DT)
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
fn interpolation_blends_previous_and_current_pose() {
    let mut world = World2D::new(Vec2::ZERO);
    let id = world.add_body(&BodyDesc2D::dynamic().moving(Vec2::new(6.0, 0.0)));
    world
        .add_collider(id, &ColliderDesc2D::new(Shape2D::circle(0.5)))
        .unwrap();
    world.step(DT);
    let now = world.position(id).unwrap().x;
    let mid = world.interpolated_transform(id, 0.5).unwrap().position.x;
    assert!((mid - now / 2.0).abs() < 1e-3);
}
