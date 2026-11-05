//! A humanoid ragdoll built from capsules and joints.

use std::collections::HashMap;
use std::f32::consts::PI;

use yowie_core::{Quat, Vec3};

use super::desc::{BodyDesc3D, ColliderDesc3D};
use super::joint::{JointDesc3D, JointKind3D};
use super::shape::Shape3D;
use super::world::World3D;
use crate::error::Result;
use crate::ids::{BodyId, CollisionLayers, JointId};
use crate::joint::Limits;

/// Names of the ragdoll parts, in the order they are created. The pelvis is
/// the root; every other part hangs from its parent by a joint.
pub const RAGDOLL_PARTS: [&str; 11] = [
    "pelvis",
    "spine",
    "head",
    "upper_arm_l",
    "forearm_l",
    "upper_arm_r",
    "forearm_r",
    "thigh_l",
    "shin_l",
    "thigh_r",
    "shin_r",
];

/// Length and radius of one capsule part. A zero field takes the default
/// scaled by [`RagdollSpec::height`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RagdollBone {
    /// End-to-end length of the capsule including both caps.
    pub length: f32,
    /// Radius of the capsule.
    pub radius: f32,
}

/// Parameters for [`Ragdoll3D::spawn`]. Zero fields take defaults of a
/// figure 1.8 units tall and 70 mass units.
#[derive(Debug, Clone, PartialEq)]
pub struct RagdollSpec {
    /// Point on the ground between the feet.
    pub position: Vec3,
    /// Rotation of the whole figure, which faces -Z with +X on its right.
    pub rotation: Quat,
    /// Standing height; 0 means 1.8.
    pub height: f32,
    /// Total mass shared between the parts; 0 means 70.
    pub mass: f32,
    /// Friction of every part; 0 means 0.6.
    pub friction: f32,
    /// Linear and angular damping of every part; 0 means 0.5.
    pub damping: f32,
    /// Layers of every part. `None` puts the figure on a layer of its own
    /// that meets everything except other ragdoll parts.
    pub layers: Option<CollisionLayers>,
    /// Size overrides: pelvis, spine, head, upper arm, forearm, thigh, shin.
    pub bones: [RagdollBone; 7],
}

impl Default for RagdollSpec {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            height: 0.0,
            mass: 0.0,
            friction: 0.0,
            damping: 0.0,
            layers: None,
            bones: [RagdollBone::default(); 7],
        }
    }
}

/// A spawned humanoid ragdoll.
#[derive(Debug, Clone)]
pub struct Ragdoll3D {
    /// Body of each part, by name from [`RAGDOLL_PARTS`].
    pub parts: HashMap<&'static str, BodyId>,
    /// Joint tying each part to its parent; the pelvis has none.
    pub joints: HashMap<&'static str, JointId>,
    /// The size each part was built with.
    pub bones: HashMap<&'static str, RagdollBone>,
}

struct Part {
    name: &'static str,
    bone: RagdollBone,
    centre: Vec3,
    share: f32,
}

impl Ragdoll3D {
    /// Creates the parts and joints of a ragdoll in `world`.
    ///
    /// # Errors
    ///
    /// Returns an error when a bone size is invalid, such as a negative
    /// radius.
    pub fn spawn(world: &mut World3D, spec: &RagdollSpec) -> Result<Self> {
        let scale = if spec.height > 0.0 {
            spec.height / 1.8
        } else {
            1.0
        };
        let b = |i: usize, len: f32, rad: f32| {
            let g = spec.bones[i];
            RagdollBone {
                length: if g.length > 0.0 {
                    g.length
                } else {
                    len * scale
                },
                radius: if g.radius > 0.0 {
                    g.radius
                } else {
                    rad * scale
                },
            }
        };
        let (pelvis, spine, head) = (b(0, 0.3, 0.13), b(1, 0.44, 0.16), b(2, 0.24, 0.12));
        let (upper, fore) = (b(3, 0.3, 0.05), b(4, 0.28, 0.045));
        let (thigh, shin) = (b(5, 0.45, 0.08), b(6, 0.45, 0.06));
        let mass = if spec.mass > 0.0 { spec.mass } else { 70.0 };
        let friction = if spec.friction > 0.0 {
            spec.friction
        } else {
            0.6
        };
        let damping = if spec.damping > 0.0 {
            spec.damping
        } else {
            0.5
        };
        let layers = spec
            .layers
            .unwrap_or(CollisionLayers::new(1 << 31, !(1u32 << 31)));

        let knee = shin.length;
        let hip = knee + thigh.length;
        let pelvis_y = hip + 0.1 * scale;
        let waist = pelvis_y + 0.12 * scale;
        let neck = waist + spine.length;
        let shoulder_y = neck - 0.08 * scale;
        let shoulder_x = spine.radius + upper.radius + 0.01 * scale;
        let elbow = shoulder_y - upper.length;
        let hip_x = 0.1 * scale;
        let p = |name, bone, x: f32, y: f32, share| Part {
            name,
            bone,
            centre: Vec3::new(x, y, 0.0),
            share,
        };
        let parts = [
            p("pelvis", pelvis, 0.0, pelvis_y, 0.15),
            p("spine", spine, 0.0, waist + spine.length / 2.0, 0.35),
            p("head", head, 0.0, neck + head.length / 2.0, 0.08),
            p(
                "upper_arm_l",
                upper,
                -shoulder_x,
                shoulder_y - upper.length / 2.0,
                0.03,
            ),
            p(
                "forearm_l",
                fore,
                -shoulder_x,
                elbow - fore.length / 2.0,
                0.02,
            ),
            p(
                "upper_arm_r",
                upper,
                shoulder_x,
                shoulder_y - upper.length / 2.0,
                0.03,
            ),
            p(
                "forearm_r",
                fore,
                shoulder_x,
                elbow - fore.length / 2.0,
                0.02,
            ),
            p("thigh_l", thigh, -hip_x, knee + thigh.length / 2.0, 0.1),
            p("shin_l", shin, -hip_x, shin.length / 2.0, 0.06),
            p("thigh_r", thigh, hip_x, knee + thigh.length / 2.0, 0.1),
            p("shin_r", shin, hip_x, shin.length / 2.0, 0.06),
        ];

        let mut out = Self {
            parts: HashMap::new(),
            joints: HashMap::new(),
            bones: HashMap::new(),
        };
        let mut centres = HashMap::new();
        for part in &parts {
            let radius = part.bone.radius;
            let half = (part.bone.length / 2.0 - radius).max(0.0);
            let volume = PI * radius * radius * (4.0 / 3.0 * radius + 2.0 * half);
            let density = mass * part.share / volume.max(1.0e-6);
            let desc = BodyDesc3D::dynamic()
                .at(spec.position + spec.rotation * part.centre)
                .rotated(spec.rotation)
                .damped(damping, damping);
            let id = world.add_body(&desc);
            let mut c = ColliderDesc3D::new(Shape3D::capsule(half, radius))
                .with_friction(friction)
                .with_density(density)
                .with_layers(layers);
            c.restitution = 0.0;
            world.add_collider(id, &c)?;
            out.parts.insert(part.name, id);
            out.bones.insert(part.name, part.bone);
            centres.insert(part.name, part.centre);
        }

        let mut link = |parent: &'static str, child: &'static str, at: Vec3, kind: JointKind3D| {
            let desc =
                JointDesc3D::new(kind).with_anchors(at - centres[parent], at - centres[child]);
            let id = world.add_joint(out.parts[parent], out.parts[child], &desc)?;
            out.joints.insert(child, id);
            Ok::<(), crate::error::PhysError>(())
        };
        let ball = |cone: f32, twist: f32| JointKind3D::Spherical {
            limits: [
                Some(Limits::new(-cone, cone)),
                Some(Limits::new(-twist, twist)),
                Some(Limits::new(-cone, cone)),
            ],
        };
        let hinge = |lo: f32, hi: f32| JointKind3D::Hinge {
            axis: Vec3::X,
            limits: Some(Limits::new(lo, hi)),
            motor: None,
        };
        let v = |x: f32, y: f32| Vec3::new(x, y, 0.0);
        link("pelvis", "spine", v(0.0, waist), ball(0.4, 0.4))?;
        link("spine", "head", v(0.0, neck), ball(0.7, 1.0))?;
        link(
            "spine",
            "upper_arm_l",
            v(-shoulder_x, shoulder_y),
            ball(1.5, 0.8),
        )?;
        link(
            "upper_arm_l",
            "forearm_l",
            v(-shoulder_x, elbow),
            hinge(0.0, 2.6),
        )?;
        link(
            "spine",
            "upper_arm_r",
            v(shoulder_x, shoulder_y),
            ball(1.5, 0.8),
        )?;
        link(
            "upper_arm_r",
            "forearm_r",
            v(shoulder_x, elbow),
            hinge(0.0, 2.6),
        )?;
        link("pelvis", "thigh_l", v(-hip_x, hip), ball(1.05, 0.5))?;
        link("thigh_l", "shin_l", v(-hip_x, knee), hinge(-2.6, 0.0))?;
        link("pelvis", "thigh_r", v(hip_x, hip), ball(1.05, 0.5))?;
        link("thigh_r", "shin_r", v(hip_x, knee), hinge(-2.6, 0.0))?;
        Ok(out)
    }

    /// Removes every part and joint from `world`.
    pub fn despawn(&self, world: &mut World3D) {
        for id in self.parts.values() {
            let _ = world.remove_body(*id);
        }
    }

    /// Places parts at world positions and rotations by name and zeroes
    /// their velocities. Parts missing from a map keep their transform.
    /// Use it to hand an animated character over to physics.
    pub fn pose(
        &self,
        world: &mut World3D,
        positions: &HashMap<&str, Vec3>,
        rotations: &HashMap<&str, Quat>,
    ) {
        for (name, id) in &self.parts {
            let pos = positions.get(name).copied().or_else(|| world.position(*id));
            let rot = rotations.get(name).copied().or_else(|| world.rotation(*id));
            if let (Some(p), Some(r)) = (pos, rot) {
                let _ = world.set_transform(*id, p, r.normalize());
                let _ = world.set_linear_velocity(*id, Vec3::ZERO);
                let _ = world.set_angular_velocity(*id, Vec3::ZERO);
            }
        }
    }
}
