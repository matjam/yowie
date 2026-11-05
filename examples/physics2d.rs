//! Rigid bodies in a pit: balls, boxes and triangles dropped by a timer
//! onto a ramp, a kinematic paddle sweeping the floor, a car driving on
//! sprung wheel joints, a trigger zone that counts what enters it, and a ray
//! cast from the corner that sweeps across the scene. The world is in
//! metres (40 pixels each). Click to drop a ball, D toggles the physics
//! debug lines, Escape quits.

use std::f32::consts::FRAC_PI_2;

use yowie::gfx::{Camera2D, Font, TextStyle};
use yowie::input::{Key, MouseButton};
use yowie::phys::{
    BodyDesc2D, BodyId, ColliderDesc2D, ColliderId, Event, JointDesc2D, JointKind2D, Motor,
    QueryFilter, Shape2D, World2D,
};
use yowie::rng::Rng;
use yowie::timer::Timers;
use yowie::{Color, Config, Context, Game, GameResult, Transform2D, Vec2, vec2};

const PPM: f32 = 40.0;
const WORLD: Vec2 = vec2(24.0, 13.5);

enum Look {
    Circle(f32),
    Polygon(Vec<Vec2>),
}

struct Item {
    body: BodyId,
    look: Look,
    color: Color,
}

struct Static {
    position: Vec2,
    rotation: f32,
    half: Vec2,
    color: Color,
}

struct Physics {
    world: World2D,
    items: Vec<Item>,
    statics: Vec<Static>,
    paddle: BodyId,
    chassis: BodyId,
    wheels: [BodyId; 2],
    zone: ColliderId,
    zone_rect: (Vec2, Vec2),
    entered: u32,
    inside: i32,
    timers: Timers<()>,
    rng: Rng,
    spawned: u32,
    clock: f32,
    debug: bool,
    ray: Option<(Vec2, f32)>,
    font: Font,
    mono: Font,
}

fn rotated(rot: f32, p: Vec2) -> Vec2 {
    Vec2::from_angle(rot).rotate(p)
}

impl Physics {
    fn spawn(&mut self, at: Vec2, kind: u32) {
        let palette = [
            Color::rgb8(240, 120, 90),
            Color::rgb8(250, 200, 80),
            Color::rgb8(110, 200, 140),
            Color::rgb8(110, 160, 240),
            Color::rgb8(200, 130, 230),
        ];
        let color = palette[kind as usize % palette.len()];
        let body = self.world.add_body(&BodyDesc2D::dynamic().at(at));
        let (shape, look) = match kind % 3 {
            0 => (Shape2D::circle(0.32), Look::Circle(0.32)),
            1 => {
                let half = vec2(0.32, 0.32);
                (
                    Shape2D::cuboid(half),
                    Look::Polygon(vec![
                        vec2(-half.x, -half.y),
                        vec2(half.x, -half.y),
                        vec2(half.x, half.y),
                        vec2(-half.x, half.y),
                    ]),
                )
            }
            _ => {
                let tri = vec![vec2(0.0, -0.42), vec2(0.4, 0.3), vec2(-0.4, 0.3)];
                (Shape2D::ConvexPolygon(tri.clone()), Look::Polygon(tri))
            }
        };
        let collider = ColliderDesc2D::new(shape)
            .with_restitution(if kind.is_multiple_of(3) { 0.45 } else { 0.1 })
            .with_friction(0.6);
        if self.world.add_collider(body, &collider).is_ok() {
            self.items.push(Item { body, look, color });
        }
    }
}

impl Game for Physics {
    fn init(ctx: &mut Context) -> GameResult<Self> {
        let mut world = World2D::new(vec2(0.0, 9.81));
        let mut statics = Vec::new();
        let rock = Color::rgb8(86, 92, 112);
        let mut add_static = |world: &mut World2D, position: Vec2, rotation: f32, half: Vec2| {
            let body = world.add_body(&BodyDesc2D::fixed().at(position).rotated(rotation));
            let _ = world.add_collider(
                body,
                &ColliderDesc2D::new(Shape2D::cuboid(half)).with_friction(0.7),
            );
            statics.push(Static {
                position,
                rotation,
                half,
                color: rock,
            });
        };
        add_static(&mut world, vec2(12.0, 13.0), 0.0, vec2(12.5, 0.5));
        add_static(&mut world, vec2(-0.5, 6.0), 0.0, vec2(0.5, 8.0));
        add_static(&mut world, vec2(24.5, 6.0), 0.0, vec2(0.5, 8.0));
        add_static(&mut world, vec2(5.0, 5.2), 0.32, vec2(3.4, 0.15));
        add_static(&mut world, vec2(13.5, 7.4), -0.28, vec2(2.6, 0.15));

        let paddle = world.add_body(&BodyDesc2D::kinematic().at(vec2(5.0, 12.0)));
        let _ = world.add_collider(
            paddle,
            &ColliderDesc2D::new(Shape2D::cuboid(vec2(1.3, 0.2))).with_friction(0.8),
        );

        // A car: a chassis and two wheels on spring suspensions. The motor
        // spins each wheel; its own friction does the driving.
        let chassis = world.add_body(&BodyDesc2D::dynamic().at(vec2(12.5, 11.3)));
        let _ = world.add_collider(
            chassis,
            &ColliderDesc2D::new(Shape2D::cuboid(vec2(1.1, 0.28))).with_density(2.0),
        );
        let mut wheels = [chassis; 2];
        for (i, dx) in [-0.85f32, 0.85].into_iter().enumerate() {
            let wheel = world.add_body(&BodyDesc2D::dynamic().at(vec2(12.5 + dx, 12.0)));
            let _ = world.add_collider(
                wheel,
                &ColliderDesc2D::new(Shape2D::circle(0.42))
                    .with_friction(1.4)
                    .with_density(1.0),
            );
            let joint = JointDesc2D::new(JointKind2D::Wheel {
                axis: Vec2::Y,
                stiffness: 900.0,
                damping: 60.0,
                motor: Some(Motor::velocity(3.0, 40.0)),
            })
            .with_anchors(vec2(dx, 0.55), Vec2::ZERO);
            let _ = world.add_joint(chassis, wheel, &joint);
            wheels[i] = wheel;
        }

        // A trigger zone on the floor: a sensor reports what enters it.
        let zone_rect = (vec2(20.6, 11.8), vec2(1.4, 0.7));
        let zone = world
            .add_static_collider(
                &ColliderDesc2D::new(Shape2D::cuboid(zone_rect.1))
                    .as_sensor()
                    .with_offset(Transform2D::from_position(zone_rect.0)),
            )
            .map_err(|e| e.to_string())?;

        let mut timers = Timers::new();
        timers.every(0.22, ());
        let game = Physics {
            world,
            items: Vec::new(),
            statics,
            paddle,
            chassis,
            wheels,
            zone,
            zone_rect,
            entered: 0,
            inside: 0,
            timers,
            rng: Rng::new(5),
            spawned: 0,
            clock: 0.0,
            debug: true,
            ray: None,
            font: ctx.gfx.default_font(),
            mono: ctx.gfx.default_mono_font(),
        };
        Ok(game)
    }

    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = ctx.time.delta();
        if ctx.input.key_pressed(Key::Escape) {
            ctx.quit();
        }
        if ctx.input.key_pressed(Key::D) {
            self.debug = !self.debug;
        }
        if ctx.input.mouse_pressed(MouseButton::Left) && ctx.input.mouse_inside() {
            let p = ctx.input.mouse() / PPM;
            self.spawn(p, 0);
        }
        self.clock += dt;
        for _ in self.timers.update(f64::from(dt)) {
            if self.spawned < 30 {
                let x = self.rng.between(1.5, 9.5);
                self.spawn(vec2(x, 0.8), self.spawned);
                self.spawned += 1;
            }
        }
        let paddle_x = 5.0 + 3.5 * (self.clock * 0.8).sin();
        let _ = self
            .world
            .set_kinematic_target(self.paddle, vec2(paddle_x, 12.0), 0.0);
        self.world.step(dt);
        for e in self.world.events() {
            match *e {
                Event::SensorEntered { sensor, .. } if sensor == self.zone => {
                    self.entered += 1;
                    self.inside += 1;
                }
                Event::SensorExited { sensor, .. } if sensor == self.zone => self.inside -= 1,
                _ => {}
            }
        }
        // A ray from the top-left corner that swings across the pit.
        let origin = vec2(1.0, 1.0);
        let angle = 0.35 + 0.55 * (self.clock * 0.6).sin() + FRAC_PI_2 * 0.5;
        let dir = Vec2::from_angle(angle);
        self.ray = self
            .world
            .ray_cast(origin, dir, 60.0, &QueryFilter::new())
            .map(|h| (h.point, h.distance));
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let cam = Camera2D::new(WORLD * 0.5).with_zoom(PPM);
        let line = 1.5 / PPM;
        let g = &mut ctx.gfx;
        g.with_camera(&cam, |g| {
            g.set_layer(0);
            // The zone first, so bodies draw over it.
            let (zc, zh) = self.zone_rect;
            let occupied = self.inside > 0;
            let fill = Color::rgba8(80, 230, 130, if occupied { 110 } else { 45 });
            g.fill_rect(yowie::Rect::from_center(zc, zh * 2.0), fill);
            for s in &self.statics {
                let corners = [
                    vec2(-s.half.x, -s.half.y),
                    vec2(s.half.x, -s.half.y),
                    vec2(s.half.x, s.half.y),
                    vec2(-s.half.x, s.half.y),
                ]
                .map(|p| s.position + rotated(s.rotation, p));
                g.fill_polygon(&corners, s.color);
            }
            g.set_layer(1);
            for item in &self.items {
                let Some(t) = self.world.transform(item.body) else {
                    continue;
                };
                match &item.look {
                    Look::Circle(r) => {
                        g.fill_circle(t.position, *r, item.color);
                        let tip = t.position + rotated(t.rotation, vec2(*r, 0.0));
                        g.line(t.position, tip, line, Color::rgb8(40, 40, 60));
                    }
                    Look::Polygon(pts) => {
                        let world: Vec<Vec2> = pts
                            .iter()
                            .map(|p| t.position + rotated(t.rotation, *p))
                            .collect();
                        g.fill_polygon(&world, item.color);
                    }
                }
            }
            // The paddle, the chassis and the wheels.
            let box_at = |g: &mut yowie::gfx::Graphics, body: BodyId, half: Vec2, c: Color| {
                if let Some(t) = self.world.transform(body) {
                    let corners = [
                        vec2(-half.x, -half.y),
                        vec2(half.x, -half.y),
                        vec2(half.x, half.y),
                        vec2(-half.x, half.y),
                    ]
                    .map(|p| t.position + rotated(t.rotation, p));
                    g.fill_polygon(&corners, c);
                }
            };
            box_at(g, self.paddle, vec2(1.3, 0.2), Color::rgb8(220, 90, 90));
            box_at(g, self.chassis, vec2(1.1, 0.28), Color::rgb8(70, 130, 220));
            for w in self.wheels {
                if let Some(t) = self.world.transform(w) {
                    g.fill_circle(t.position, 0.42, Color::rgb8(40, 44, 56));
                    let spoke = t.position + rotated(t.rotation, vec2(0.42, 0.0));
                    g.line(t.position, spoke, 2.5 / PPM, Color::rgb8(190, 196, 210));
                }
            }
            // The ray.
            let origin = vec2(1.0, 1.0);
            if let Some((hit, _)) = self.ray {
                g.line(origin, hit, line, Color::rgb8(255, 80, 80));
                g.fill_circle(hit, 0.1, Color::rgb8(255, 240, 120));
            }
            g.fill_circle(origin, 0.14, Color::rgb8(255, 80, 80));
            if self.debug {
                g.set_layer(2);
                self.world.debug_draw_to(g);
            }
        });

        g.set_layer(yowie::gfx::layers::UI);
        g.draw_text(
            &self.font,
            "Yowie physics2d",
            vec2(14.0, 10.0),
            &TextStyle::new(22.0).color(Color::rgb8(255, 220, 120)),
        );
        let ray = self
            .ray
            .map_or("none".to_string(), |(_, d)| format!("{d:.2} m"));
        let text = format!(
            "bodies {}  trigger: {} entered, {} inside\nray distance {}   debug lines {} (D)",
            self.world.body_count(),
            self.entered,
            self.inside,
            ray,
            if self.debug { "on" } else { "off" }
        );
        g.draw_text(
            &self.mono,
            &text,
            vec2(14.0, 40.0),
            &TextStyle::new(13.0).color(Color::rgb8(200, 230, 210)),
        );
        Ok(())
    }
}

fn main() -> Result<(), yowie::Error> {
    let config = Config {
        clear_color: Color::rgb8(22, 26, 40),
        ..Config::new("Yowie physics2d").with_size(960, 540)
    };
    yowie::run::<Physics>(config)
}
