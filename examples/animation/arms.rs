//! Three robot arms built from boxes and spheres in code. Each arm is a
//! three-joint skeleton posed by `yowie-anim`: one plays a clip with an
//! event, one reaches for a moving target by two-bone IK, one blends two
//! clips by a pace parameter.

use yowie::anim::blend::BlendPoint1D;
use yowie::anim::ik_pose::solve_two_bone;
use yowie::anim::{
    Blend, BlendNode, BlendSpace1D, ClipSet, Joint, Player, PlayerEvent, Pose, Skeleton,
};
use yowie::gfx::scene::{Material, Mesh, Scene};
use yowie::{Color, Mat4, Transform, Vec3, vec3};

use crate::clips;

/// Length of each bone.
const BONE: f32 = 1.0;

/// Where each arm's base stands, in the order event, reach, blend.
pub const BASES: [Vec3; 3] = [
    vec3(-2.8, -0.5, 0.0),
    vec3(2.8, -0.5, 0.0),
    vec3(0.0, -0.5, -3.0),
];

/// The three arms and the state that drives them.
pub struct Arms {
    skeleton: Skeleton,
    clips: ClipSet,
    event_arm: Player,
    reach_arm: Player,
    blend: Blend,
    /// Whether the middle arm reaches by IK.
    pub ik_on: bool,
    /// Position of the blend between the swing (0) and the stride (1).
    pub pace: f32,
    target: Vec3,
    flash: f32,
    /// Distance from the middle arm's hand to its target, when IK ran.
    pub reach_error: Option<f32>,
}

impl Arms {
    /// Builds the skeleton and starts the clips.
    pub fn new() -> Self {
        let joint = |name: &str, parent: Option<usize>, y: f32| {
            Joint::new(name, parent, Transform::from_xyz(0.0, y, 0.0))
        };
        let skeleton = Skeleton::from_rest(vec![
            joint("shoulder", None, 0.0),
            joint("elbow", Some(0), BONE),
            joint("hand", Some(1), BONE),
        ])
        .unwrap_or_else(|e| unreachable!("fixed skeleton is valid: {e}"));
        let mut event_arm = Player::new();
        event_arm.play(clips::swing());
        let mut reach_arm = Player::new();
        reach_arm.play(clips::swing());
        let blend = Blend::new(BlendNode::Space1D(BlendSpace1D {
            parameter: "pace".into(),
            clips: vec![
                BlendPoint1D {
                    clip: "swing".into(),
                    at: 0.0,
                },
                BlendPoint1D {
                    clip: "stride".into(),
                    at: 1.0,
                },
            ],
        }));
        Self {
            skeleton,
            clips: clips::arm_clips(),
            event_arm,
            reach_arm,
            blend,
            ik_on: true,
            pace: 0.5,
            target: Vec3::ZERO,
            flash: 0.0,
            reach_error: None,
        }
    }

    /// Advances everything by `dt` seconds at game time `t`, and returns
    /// log lines for the events the first arm's player reported.
    pub fn update(&mut self, dt: f32, t: f32) -> Vec<String> {
        self.target = vec3(
            0.9 * (t * 1.3).cos(),
            1.0 + 0.45 * (t * 0.7).sin(),
            0.6 * (t * 1.3).sin(),
        );
        self.event_arm.advance(dt);
        self.reach_arm.advance(dt);
        self.blend.set("pace", self.pace);
        self.blend.advance(dt, &self.clips);
        self.flash = (self.flash - dt * 3.0).max(0.0);
        let mut lines = Vec::new();
        for event in self.event_arm.take_events() {
            if let PlayerEvent::Marker { name, time, clip } = event {
                self.flash = 1.0;
                lines.push(format!("event \"{name}\" at {time:.1}s of {clip}"));
            }
        }
        lines
    }

    fn reach_pose(&mut self) -> Pose {
        let mut pose = self.skeleton.pose_from_player(&self.reach_arm);
        self.reach_error = None;
        if self.ik_on {
            let pole = vec3(-2.0, BONE, 1.0);
            if solve_two_bone(&self.skeleton, &mut pose, [0, 1, 2], self.target, pole).is_ok() {
                let hand = self.skeleton.world_matrices(&pose)[2].w_axis.truncate();
                self.reach_error = Some((hand - self.target).length());
            }
        }
        pose
    }

    /// Records the three arms and the middle arm's target into `s`.
    pub fn draw(&mut self, s: &mut Scene<'_>, bone: &Mesh, ball: &Mesh) {
        let event_pose = self.skeleton.pose_from_player(&self.event_arm);
        let reach_pose = self.reach_pose();
        let blend_pose = self
            .blend
            .pose(&self.skeleton, &self.clips)
            .unwrap_or_else(|| self.skeleton.rest_pose());
        let looks = [
            (event_pose, Color::rgb8(200, 90, 80), self.flash),
            (reach_pose, Color::rgb8(90, 150, 220), 0.0),
            (blend_pose, Color::rgb8(110, 190, 120), 0.0),
        ];
        for ((pose, color, glow), base) in looks.iter().zip(BASES) {
            self.draw_arm(
                s,
                bone,
                ball,
                pose,
                *color,
                *glow,
                Mat4::from_translation(base),
            );
        }
        let goal = Material::glowing(Color::rgb8(120, 255, 160), 3.0);
        let at = BASES[1] + self.target;
        s.draw(
            ball,
            &goal,
            Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(0.14)),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_arm(
        &self,
        s: &mut Scene<'_>,
        bone: &Mesh,
        ball: &Mesh,
        pose: &Pose,
        color: Color,
        glow: f32,
        base: Mat4,
    ) {
        let world = self.skeleton.world_matrices(pose);
        let metal = Material::color(color, 0.5, 0.35);
        let joint = Material::color(Color::rgb8(205, 208, 220), 0.2, 0.5);
        for m in &world[..2] {
            let place = base * *m * Mat4::from_translation(vec3(0.0, BONE * 0.5, 0.0));
            let size = vec3(0.26, BONE, 0.26);
            s.draw(bone, &metal, place * Mat4::from_scale(size));
            s.draw(
                ball,
                &joint,
                base * *m * Mat4::from_scale(Vec3::splat(0.22)),
            );
        }
        let tip = Material {
            emissive: Color::rgb8(255, 210, 90),
            emissive_strength: 4.0 * glow,
            ..Material::color(Color::rgb8(240, 190, 90), 0.2, 0.4)
        };
        s.draw(
            ball,
            &tip,
            base * world[2] * Mat4::from_scale(Vec3::splat(0.17)),
        );
    }
}
