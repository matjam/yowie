//! A sphere with three morph targets, blended in the vertex shader. A clip
//! drives the weights through a `Player`; changing them costs no upload.

use std::sync::Arc;

use yowie::anim::{Channel, Clip, Curve, LoopMode, Player, Value};
use yowie::gfx::Graphics;
use yowie::gfx::scene::{Material, Mesh, MeshData, MorphTarget, Scene};
use yowie::{Color, Mat4, Vec3, vec3};

/// Names of the targets, in weight order.
pub const TARGETS: [&str; 3] = ["point", "squash", "snout"];

/// The mesh, its clip player and the weights last read from it.
pub struct Face {
    mesh: Mesh,
    player: Player,
    /// Current weights, one per target.
    pub weights: [f32; 3],
    /// Overrides the clip's weight for the second target when set.
    pub squash: Option<f32>,
}

fn target(mesh: &MeshData, delta: impl Fn(Vec3) -> Vec3) -> MorphTarget {
    MorphTarget {
        positions: mesh.vertices.iter().map(|v| delta(v.position)).collect(),
        normals: Vec::new(),
    }
}

/// The weights over four seconds: the crown rises into a point while the
/// snout breathes.
fn clip() -> Arc<Clip> {
    let curve = |keys: [(f32, f32); 3]| Curve::linear(keys);
    Arc::new(Clip::new("face").with_mode(LoopMode::PingPong).with_track(
        "weights",
        Channel::Weights(vec![
            curve([(0.0, 0.0), (2.0, 0.45), (4.0, 0.0)]),
            curve([(0.0, 0.3), (2.0, 0.0), (4.0, 0.3)]),
            curve([(0.0, 0.1), (1.0, 0.8), (2.0, 0.1)]),
        ]),
    ))
}

impl Face {
    /// Builds the sphere and its three targets and starts the clip.
    pub fn new(gfx: &Graphics) -> yowie::GameResult<Self> {
        let data = MeshData::sphere(1.0, 20, 40);
        let targets = [
            target(&data, |p| vec3(-p.x * 0.6, p.y.max(0.0) * 1.2, -p.z * 0.6)),
            target(&data, |p| vec3(p.x * 0.5, -p.y * 0.45, p.z * 0.5)),
            target(&data, |p| vec3(0.0, 0.0, p.z.max(0.0) * 0.9)),
        ];
        let mut player = Player::new();
        player.play(clip());
        Ok(Face {
            mesh: Mesh::with_morph_targets(gfx, &data, &targets)?,
            player,
            weights: [0.0; 3],
            squash: None,
        })
    }

    /// Advances the clip by `dt` seconds and reads its weights.
    pub fn update(&mut self, dt: f32) {
        self.player.advance(dt);
        let mut w = self.weights;
        self.player.evaluate(|_, v| {
            if let Value::Weights(list) = v {
                for (slot, x) in w.iter_mut().zip(list) {
                    *slot = x;
                }
            }
        });
        if let Some(s) = self.squash {
            w[1] = s;
        }
        self.weights = w;
    }

    /// Records the sphere at `at`.
    pub fn draw(&self, s: &mut Scene<'_>, at: Vec3, radius: f32) {
        let skin = Material::color(Color::rgb8(230, 180, 150), 0.0, 0.55);
        let place = Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(radius));
        s.draw_morphed(&self.mesh, &skin, place, &self.weights);
    }
}
