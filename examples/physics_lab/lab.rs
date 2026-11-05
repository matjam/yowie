//! The lab's simulation: terrain, tumbling debris, the hinge chain, the
//! paddle wheel, the ragdoll and the character on the stairs.

use yowie::phys::{
    BodyDesc3D, BodyId, CharacterController3D, ColliderDesc3D, JointDesc3D, JointId, Motor,
    Ragdoll3D, RagdollSpec, Shape3D, World3D,
};
use yowie::rng::Rng;
use yowie::{Color, GameResult, Quat, Transform, Vec3};

use crate::meshes::{Terrain, octahedron_points, wedge_points};

/// Where the paddle wheel turns.
pub const WHEEL_AT: Vec3 = Vec3::new(-2.0, 2.4, 8.0);
/// Top of the slab the stairs stand on.
pub const SLAB_TOP: f32 = 2.0;
/// The stairs: first step's left edge, depth, rise per step and count.
pub const STAIRS_X: f32 = 6.0;
pub const STEP_DEPTH: f32 = 1.2;
pub const STEP_RISE: f32 = 0.3;
pub const STEP_COUNT: usize = 6;
/// Centre of the stair slab and its half extents.
pub const SLAB: (Vec3, Vec3) = (Vec3::new(8.0, -0.25, -8.0), Vec3::new(8.0, 2.25, 3.0));
/// The character's capsule.
pub const HERO_HALF: f32 = 0.45;
pub const HERO_RADIUS: f32 = 0.35;
const DEBRIS: usize = 48;
/// The most debris the lab keeps; `spawn` stops here.
pub const DEBRIS_LIMIT: usize = 400;

const PALETTE: [Color; 5] = [
    Color::rgb(0.94, 0.55, 0.12),
    Color::rgb(0.24, 0.75, 0.31),
    Color::rgb(0.2, 0.43, 0.94),
    Color::rgb(0.94, 0.39, 0.7),
    Color::rgb(0.94, 0.86, 0.24),
];

/// The shapes the lab throws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Capsule,
    Octahedron,
    Wedge,
    Box,
    Sphere,
}

impl Kind {
    /// All kinds, in the order the lab cycles through them.
    pub const ALL: [Kind; 5] = [
        Kind::Capsule,
        Kind::Octahedron,
        Kind::Wedge,
        Kind::Box,
        Kind::Sphere,
    ];

    /// The name the `spawn` command accepts.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Capsule => "capsule",
            Kind::Octahedron => "hull",
            Kind::Wedge => "wedge",
            Kind::Box => "box",
            Kind::Sphere => "sphere",
        }
    }

    fn shape(self) -> Shape3D {
        match self {
            Kind::Capsule => Shape3D::capsule(0.45, 0.3),
            Kind::Octahedron => Shape3D::ConvexHull(octahedron_points(0.7)),
            Kind::Wedge => Shape3D::ConvexHull(wedge_points(0.6, 0.35, 0.5)),
            Kind::Box => Shape3D::cuboid(Vec3::splat(0.4)),
            Kind::Sphere => Shape3D::sphere(0.4),
        }
    }
}

/// One thrown body.
pub struct Debris {
    pub body: BodyId,
    pub kind: Kind,
    pub color: Color,
}

/// The character that walks the stairs up and down, turning at each end.
pub struct Hero {
    pub body: BodyId,
    pub controller: CharacterController3D,
    pub direction: f32,
    pub grounded: bool,
}

/// Everything the lab simulates.
pub struct Lab {
    pub world: World3D,
    pub debris: Vec<Debris>,
    pub links: Vec<BodyId>,
    pub ball: BodyId,
    pub wheel: BodyId,
    wheel_joint: JointId,
    pub ragdoll: Option<Ragdoll3D>,
    pub hero: Hero,
    rng: Rng,
}

impl Lab {
    /// Builds the static scenery, the chain, the wheel and the character,
    /// then drops the debris.
    pub fn new(terrain: &Terrain) -> GameResult<Self> {
        let mut world = World3D::new(Vec3::new(0.0, -9.8, 0.0));
        world.add_static_collider(&ColliderDesc3D::new(Shape3D::TriMesh {
            vertices: terrain.positions.clone(),
            indices: terrain.triangles.clone(),
        }))?;
        build_stairs(&mut world)?;
        let (links, ball) = build_chain(&mut world)?;
        let (wheel, wheel_joint) = build_wheel(&mut world)?;
        let hero_body = world.add_body(&BodyDesc3D::kinematic().at(Vec3::new(
            2.0,
            SLAB_TOP + HERO_HALF + HERO_RADIUS + 0.05,
            -8.0,
        )));
        world.add_collider(
            hero_body,
            &ColliderDesc3D::new(Shape3D::capsule(HERO_HALF, HERO_RADIUS)),
        )?;
        let mut controller = CharacterController3D::new(Shape3D::capsule(HERO_HALF, HERO_RADIUS));
        controller.step_height = Some(0.45);
        controller.snap_to_ground = Some(0.4);
        controller.max_climb_angle = 50.0_f32.to_radians();
        let mut lab = Lab {
            world,
            debris: Vec::new(),
            links,
            ball,
            wheel,
            wheel_joint,
            ragdoll: None,
            hero: Hero {
                body: hero_body,
                controller,
                direction: 1.0,
                grounded: false,
            },
            rng: Rng::new(5),
        };
        // The character moves before the first update's step, so one step
        // now puts the scenery into the query structures.
        lab.world.step(1.0 / 60.0);
        lab.reset()?;
        Ok(lab)
    }

    /// Removes the debris and the ragdoll and throws a fresh set.
    pub fn reset(&mut self) -> GameResult {
        for d in self.debris.drain(..) {
            self.world.remove_body(d.body)?;
        }
        for i in 0..DEBRIS {
            let x = self.rng.between(-12.0, 4.0);
            let z = self.rng.between(-4.0, 12.0);
            let y = 6.0 + (i % 6) as f32 * 2.0 + self.rng.between(0.0, 1.0);
            self.throw(
                Kind::ALL[i % 5],
                Vec3::new(x, y, z),
                PALETTE[i % PALETTE.len()],
            )?;
        }
        self.drop_ragdoll()
    }

    /// Throws `count` bodies of `kind` (or a mix) from above the terrain.
    pub fn spawn(&mut self, count: usize, kind: Option<Kind>) -> GameResult<usize> {
        let room = DEBRIS_LIMIT.saturating_sub(self.debris.len());
        let count = count.min(room);
        for i in 0..count {
            let at = Vec3::new(
                self.rng.between(-10.0, 6.0),
                9.0 + self.rng.between(0.0, 4.0),
                self.rng.between(-3.0, 11.0),
            );
            let kind = kind.unwrap_or(Kind::ALL[self.rng.below(5) as usize]);
            self.throw(kind, at, PALETTE[(self.debris.len() + i) % PALETTE.len()])?;
        }
        Ok(count)
    }

    fn throw(&mut self, kind: Kind, at: Vec3, color: Color) -> GameResult {
        let r = &mut self.rng;
        let axis = Vec3::new(r.float(), r.float(), r.float()).normalize_or(Vec3::Y);
        let mut desc = BodyDesc3D::dynamic()
            .at(at)
            .rotated(Quat::from_axis_angle(axis, r.float() * 3.0));
        desc.angular_velocity = Vec3::new(
            r.between(-2.0, 2.0),
            r.between(-2.0, 2.0),
            r.between(-2.0, 2.0),
        );
        if kind == Kind::Sphere {
            desc = desc.with_ccd();
        }
        let body = self.world.add_body(&desc);
        let collider = ColliderDesc3D::new(kind.shape())
            .with_friction(0.6)
            .with_restitution(0.1);
        self.world.add_collider(body, &collider)?;
        self.debris.push(Debris { body, kind, color });
        Ok(())
    }

    /// Drops a ragdoll twice life size, tipped over so it lands in a heap.
    pub fn drop_ragdoll(&mut self) -> GameResult {
        if let Some(old) = self.ragdoll.take() {
            old.despawn(&mut self.world);
        }
        let tilt = Quat::from_rotation_x(0.7) * Quat::from_rotation_z(0.4);
        let spec = RagdollSpec {
            position: Vec3::new(5.0, 6.0, 10.0),
            rotation: tilt,
            height: 3.6,
            ..RagdollSpec::default()
        };
        self.ragdoll = Some(Ragdoll3D::spawn(&mut self.world, &spec)?);
        Ok(())
    }

    /// Throws every loose body upward by `speed` units per second.
    pub fn kick(&mut self, speed: f32) {
        let ids: Vec<BodyId> = self.debris.iter().map(|d| d.body).collect();
        for id in ids {
            let spin = Vec3::new(0.0, 0.0, 2.0);
            let _ = self
                .world
                .set_linear_velocity(id, Vec3::new(0.0, speed, 0.0));
            let _ = self.world.set_angular_velocity(id, spin);
        }
    }

    /// Sets the wheel motor's target speed (radians per second) and torque.
    pub fn set_wheel(&mut self, speed: f32, torque: f32) {
        let _ = self
            .world
            .set_joint_motor(self.wheel_joint, Motor::velocity(speed, torque));
    }

    /// Walks the character up the stairs and back, turning at the slab's
    /// start and at the landing, then steps the world.
    pub fn step(&mut self, dt: f32, paused: bool) {
        if paused {
            return;
        }
        let h = &mut self.hero;
        let x = self.world.position(h.body).map_or(0.0, |p| p.x);
        if x > STAIRS_X + STEP_DEPTH * (STEP_COUNT - 1) as f32 + 1.5 {
            h.direction = -1.0;
        } else if x < 2.0 {
            h.direction = 1.0;
        }
        // A small downward pull; the controller snaps to the ground.
        let want = Vec3::new(2.5 * h.direction * dt, -0.02, 0.0);
        if let Ok(m) = self
            .world
            .move_character_body(&h.controller, h.body, want, dt)
        {
            h.grounded = m.grounded;
        }
        self.world.step(dt);
    }

    /// The number of dynamic bodies that are asleep, out of the total.
    pub fn sleeping(&self) -> (usize, usize) {
        let ids = self
            .debris
            .iter()
            .map(|d| d.body)
            .chain(self.links.iter().copied())
            .chain([self.ball, self.wheel])
            .chain(self.ragdoll.iter().flat_map(|r| r.parts.values().copied()));
        let (mut asleep, mut total) = (0, 0);
        for id in ids {
            total += 1;
            asleep += usize::from(self.world.is_sleeping(id));
        }
        (asleep, total)
    }
}

fn static_box(world: &mut World3D, centre: Vec3, half: Vec3) -> GameResult {
    let desc =
        ColliderDesc3D::new(Shape3D::cuboid(half)).with_offset(Transform::from_translation(centre));
    world.add_static_collider(&desc)?;
    Ok(())
}

/// The slab, the six steps and a landing at the top.
fn build_stairs(world: &mut World3D) -> GameResult {
    static_box(world, SLAB.0, SLAB.1)?;
    for i in 0..STEP_COUNT {
        let (c, h) = step_box(i);
        static_box(world, c, h)?;
    }
    Ok(())
}

/// Centre and half extents of step `i`; the last one is a long landing.
pub fn step_box(i: usize) -> (Vec3, Vec3) {
    let height = STEP_RISE * (i + 1) as f32;
    let left = STAIRS_X + STEP_DEPTH * i as f32;
    let depth = if i + 1 == STEP_COUNT { 4.0 } else { STEP_DEPTH };
    (
        Vec3::new(left + depth * 0.5, SLAB_TOP + height * 0.5, -8.0),
        Vec3::new(depth * 0.5, height * 0.5, 3.0),
    )
}

/// Seven hinged links hanging from a point in the sky with a ball on the end.
fn build_chain(world: &mut World3D) -> GameResult<(Vec<BodyId>, BodyId)> {
    let anchor_at = Vec3::new(-9.0, 12.0, 4.0);
    let anchor = world.add_body(&BodyDesc3D::fixed().at(anchor_at));
    let hinge = JointDesc3D::hinge(Vec3::Z);
    let mut prev = anchor;
    let mut prev_anchor = Vec3::ZERO;
    let mut links = Vec::new();
    for i in 0..7 {
        let body = world.add_body(
            &BodyDesc3D::dynamic()
                .at(anchor_at + Vec3::new(0.5 + i as f32, 0.0, 0.0))
                .damped(0.5, 0.5),
        );
        let c = ColliderDesc3D::new(Shape3D::cuboid(Vec3::new(0.5, 0.12, 0.12)));
        world.add_collider(body, &c)?;
        let joint = hinge
            .clone()
            .with_anchors(prev_anchor, Vec3::new(-0.5, 0.0, 0.0));
        world.add_joint(prev, body, &joint)?;
        prev = body;
        prev_anchor = Vec3::new(0.5, 0.0, 0.0);
        links.push(body);
    }
    let ball = world.add_body(
        &BodyDesc3D::dynamic()
            .at(anchor_at + Vec3::new(7.6, 0.0, 0.0))
            .damped(0.2, 0.5),
    );
    world.add_collider(
        ball,
        &ColliderDesc3D::new(Shape3D::sphere(0.6)).with_density(3.0),
    )?;
    let joint = hinge.with_anchors(prev_anchor, Vec3::new(-0.6, 0.0, 0.0));
    world.add_joint(prev, ball, &joint)?;
    Ok((links, ball))
}

/// A cross of two paddles on a world hinge, turned by the hinge's motor.
fn build_wheel(world: &mut World3D) -> GameResult<(BodyId, JointId)> {
    let anchor = world.add_body(&BodyDesc3D::fixed().at(WHEEL_AT));
    let wheel = world.add_body(&BodyDesc3D::dynamic().at(WHEEL_AT).damped(0.0, 0.2));
    let shape = Shape3D::Compound(vec![
        (
            Transform::IDENTITY,
            Shape3D::cuboid(Vec3::new(1.6, 0.12, 0.5)),
        ),
        (
            Transform::IDENTITY,
            Shape3D::cuboid(Vec3::new(0.12, 1.6, 0.5)),
        ),
    ]);
    world.add_collider(wheel, &ColliderDesc3D::new(shape).with_friction(0.6))?;
    let joint = JointDesc3D::hinge(Vec3::Z).with_motor(Motor::velocity(1.5, 400.0));
    let id = world.add_joint(anchor, wheel, &joint)?;
    Ok((wheel, id))
}
